// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Async task ownership and bounded shutdown for live components.
//!
//! This module keeps spawned work attached to its owner until shutdown observes a terminal result.
//! Use [`TaskGroup`] for related unit-output tasks and [`TaskSlot`] or [`SharedTaskSlot`] when one
//! task's identity or typed result belongs to the owning component. A shutdown timeout retains
//! ownership so the caller can drain the task again instead of detaching it.
//!
//! # Task groups
//!
//! [`TaskGroup`] owns related `Future<Output = ()>` tasks for one lifecycle generation.
//! [`TaskGroup::spawn`] registers each task before its future can poll. [`TaskGroup::spawn_named`]
//! additionally returns a read-only [`TaskRef`] that preserves the task's logical name, instance
//! identity, and terminal state without transferring ownership. Once
//! [`TaskGroup::begin_shutdown`] closes admission, concurrent and later spawn attempts return
//! [`TaskSpawnError`].
//!
//! # Generation-bound spawning
//!
//! [`TaskSpawner`] lets an admitted task create children in the same generation. Its cancellation
//! token signals shutdown but does not grant task ownership: child work must still pass through
//! [`TaskSpawner::spawn`]. A spawner from a closed generation cannot admit work into a replacement
//! generation.
//!
//! # Group shutdown
//!
//! [`TaskGroup::begin_shutdown`] synchronously closes admission and requests graceful cancellation.
//! [`TaskGroup::abort`] closes admission and requests forced cancellation immediately when a
//! synchronous owner cannot offer a graceful completion phase.
//! [`TaskGroup::finish_shutdown`] waits for admitted tasks, requests forced cancellation after the
//! graceful deadline, and waits again within the abort deadline. Panics, unexpected cancellation,
//! and tasks that outlive both deadlines remain observable through [`TaskShutdownError`].
//!
//! A timed-out generation stays closed and retains its tasks for another drain attempt.
//! [`TaskGroup::start_generation`] opens a replacement only after the prior generation fully
//! drains. Dropping a group requests forced cancellation but cannot await task termination, so
//! owners that need a proven shutdown must call [`TaskGroup::finish_shutdown`].
//!
//! # Partial setup rollback
//!
//! [`TaskGroupGuard`] closes its task groups and runs a synchronous rollback callback if setup
//! exits while the guard remains armed. Disarm it after setup succeeds. The owner still performs
//! the asynchronous bounded drain after a rollback.
//!
//! # Singular tasks
//!
//! [`TaskSlot`] owns one task and preserves its output type. [`SharedTaskSlot`] provides the same
//! ownership for clients that share the task across clones and serializes concurrent drain
//! attempts. [`finish_task`] and [`SharedTaskSlot::finish`] wait gracefully, abort within a second
//! bound, and retain an unfinished task if the finish future is canceled or the abort deadline
//! expires.
//!
//! # Outcomes and errors
//!
//! [`TaskJoinOutcome`] distinguishes normal completion, owner-requested abort, join failure, and an
//! incomplete task that remains owned. [`TaskSpawnError`] reports failed admission or start,
//! [`TaskGenerationError`] prevents a replacement generation from opening too early, and
//! [`TaskShutdownError`] reports group shutdown state, failures, and remaining tasks.

use std::{
    any::Any,
    error::Error,
    fmt::Display,
    future::Future,
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use futures_util::FutureExt;
use nautilus_common::live::dst::{
    task::{JoinError, JoinHandle},
    time,
};
use parking_lot::{Mutex, MutexGuard};
use tokio_util::{
    sync::CancellationToken,
    task::{TaskTracker, task_tracker::TaskTrackerToken},
};

type TaskState = AtomicU8;

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

/// A process-unique live task instance identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

impl TaskId {
    fn next() -> Self {
        let id = NEXT_TASK_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("task ID space exhausted");
        Self(id)
    }
}

impl Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A read-only reference to one task admitted by a [`TaskGroup`].
///
/// The group retains cancellation and join ownership. The task is active from admission until its
/// wrapper reaches a terminal path; active does not imply that the user future has received its
/// first poll. A task may finish before [`TaskGroup::spawn_named`] returns.
#[derive(Clone, Debug)]
pub struct TaskRef {
    identity: Arc<TaskIdentity>,
}

impl TaskRef {
    /// Returns the task's process-unique instance identifier.
    #[must_use]
    pub fn id(&self) -> TaskId {
        self.identity.id
    }

    /// Returns the task's logical name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.identity.name
    }

    /// Returns whether the task was admitted and has not reached a terminal path.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.is_finished()
    }

    /// Returns whether the task reached a terminal path.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.identity.finished.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
struct TaskIdentity {
    id: TaskId,
    name: &'static str,
    finished: AtomicBool,
}

impl TaskIdentity {
    fn new(name: &'static str) -> Self {
        Self {
            id: TaskId::next(),
            name,
            finished: AtomicBool::new(false),
        }
    }

    fn finish(&self) {
        self.finished.store(true, Ordering::Release);
    }

    fn failure(&self, failure: &str) -> String {
        format!("task '{}' ({}) {failure}", self.name, self.id)
    }
}

/// Owns a related group of cancellation-aware live tasks, one generation at a time.
///
/// Call [`Self::finish_shutdown`] to observe task failures. Dropping the group requests forced
/// cancellation but cannot asynchronously prove termination.
#[derive(Debug)]
pub struct TaskGroup {
    inner: Arc<TaskGroupInner>,
}

impl Default for TaskGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskGroup {
    /// Creates an open initial task generation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TaskGroupInner {
                generation: ArcSwap::from_pointee(TaskGeneration::new()),
                generation_lock: parking_lot::Mutex::new(()),
                drain_lock: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// Returns a capability bound to the current open generation.
    ///
    /// # Errors
    ///
    /// Returns an error after shutdown begins.
    pub fn spawner(&self) -> Result<TaskSpawner, TaskSpawnError> {
        let generation = self.inner.current();
        if !generation.is_open() {
            return Err(TaskSpawnError::CLOSED);
        }
        Ok(TaskSpawner { generation })
    }

    /// Returns a non-authoritative cancellation signal for the current generation.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.current().cancellation.child_token()
    }

    /// Registers `future` before allowing it to poll.
    ///
    /// # Errors
    ///
    /// Returns an error after shutdown begins.
    pub fn spawn<F>(&self, future: F) -> Result<(), TaskSpawnError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.inner.current().spawn(future)
    }

    // panics-doc-ok (transitive via task ID allocation)
    /// Registers a named `future` before allowing it to poll.
    ///
    /// The returned reference observes identity and terminal state without owning cancellation or
    /// joining. The task may reach a terminal state before this method returns.
    ///
    /// # Errors
    ///
    /// Returns an error after shutdown begins.
    ///
    /// # Panics
    ///
    /// Panics if the process exhausts the `u64` task identifier space.
    pub fn spawn_named<F>(&self, name: &'static str, future: F) -> Result<TaskRef, TaskSpawnError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.inner.current().spawn_named(name, future)
    }

    /// Closes admission and cancels the current generation.
    pub fn begin_shutdown(&self) {
        self.inner.begin_shutdown();
    }

    /// Closes admission and requests immediate forced cancellation.
    pub fn abort(&self) {
        self.inner.begin_shutdown().force.cancel();
    }

    /// Drains the closed generation within graceful and forced bounds.
    ///
    /// # Errors
    ///
    /// Returns an error if admission is open, a task fails unexpectedly, or forced completion
    /// reaches its deadline. Timed-out tasks remain tracked and reopening stays disabled.
    pub async fn finish_shutdown(
        &self,
        graceful_timeout: Duration,
        abort_timeout: Duration,
    ) -> Result<(), TaskShutdownError> {
        let generation = self.inner.current();
        if generation.phase() == TaskGroupPhase::Open {
            return Err(TaskShutdownError::StillOpen);
        }

        let started = time::Instant::now();
        let Some(graceful_deadline) = started.checked_add(graceful_timeout) else {
            return Err(generation.timeout_error());
        };

        let Some(abort_deadline) = graceful_deadline.checked_add(abort_timeout) else {
            return Err(generation.timeout_error());
        };

        let lock_timeout = abort_deadline.saturating_duration_since(time::Instant::now());
        let Ok(_drain_lock) = time::timeout(lock_timeout, self.inner.drain_lock.lock()).await
        else {
            return Err(generation.timeout_error());
        };

        match generation.phase() {
            TaskGroupPhase::Open => return Err(TaskShutdownError::StillOpen),
            TaskGroupPhase::Drained => return generation.complete_shutdown(),
            TaskGroupPhase::Closing => {}
        }

        generation.tasks.close();
        generation.cancellation.cancel();
        let graceful_remaining = graceful_deadline.saturating_duration_since(time::Instant::now());
        if time::timeout(graceful_remaining, generation.tasks.wait())
            .await
            .is_ok()
        {
            return generation.complete_shutdown();
        }

        generation.force.cancel();
        let abort_remaining = abort_deadline.saturating_duration_since(time::Instant::now());
        if time::timeout(abort_remaining, generation.tasks.wait())
            .await
            .is_err()
        {
            let incomplete = generation.tasks.len();
            if incomplete == 0 {
                return generation.complete_shutdown();
            }
            return Err(TaskShutdownError::Timeout {
                failures: generation.take_failures(),
                incomplete,
            });
        }

        generation.complete_shutdown()
    }

    /// Opens a fresh generation after the prior generation fully drains.
    ///
    /// # Errors
    ///
    /// Returns an error while the current generation remains open or owns tasks.
    pub fn start_generation(&self) -> Result<(), TaskGenerationError> {
        self.inner.start_generation()
    }

    /// Returns whether no tasks remain owned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.current().tasks.is_empty()
    }

    /// Returns whether every tracked task has finished.
    #[must_use]
    pub fn all_finished(&self) -> bool {
        self.is_empty()
    }

    /// Returns whether the current generation accepts tasks.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.current().is_open()
    }

    /// Returns the number of tasks currently owned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.current().tasks.len()
    }
}

impl Drop for TaskGroup {
    fn drop(&mut self) {
        self.abort();
    }
}

/// A generation-bound capability for cancellation-aware child tasks.
#[derive(Clone, Debug)]
pub struct TaskSpawner {
    generation: Arc<TaskGeneration>,
}

impl TaskSpawner {
    /// Returns a non-authoritative cancellation signal for this generation.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.generation.cancellation.child_token()
    }

    /// Registers `future` before allowing it to poll.
    ///
    /// # Errors
    ///
    /// Returns an error when this spawner no longer belongs to the open generation.
    pub fn spawn<F>(&self, future: F) -> Result<(), TaskSpawnError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.generation.spawn(future)
    }

    // panics-doc-ok (transitive via task ID allocation)
    /// Registers a named `future` before allowing it to poll.
    ///
    /// The returned reference observes identity and terminal state without owning cancellation or
    /// joining. The task may reach a terminal state before this method returns.
    ///
    /// # Errors
    ///
    /// Returns an error when this spawner no longer belongs to the open generation.
    ///
    /// # Panics
    ///
    /// Panics if the process exhausts the `u64` task identifier space.
    pub fn spawn_named<F>(&self, name: &'static str, future: F) -> Result<TaskRef, TaskSpawnError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.generation.spawn_named(name, future)
    }
}

/// Closes task groups and runs synchronous rollback when dropped while armed.
pub struct TaskGroupGuard<F: FnOnce()> {
    groups: Vec<Arc<TaskGroupInner>>,
    rollback: Option<F>,
}

impl<F: FnOnce()> std::fmt::Debug for TaskGroupGuard<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TaskGroupGuard))
            .field("groups", &self.groups.len())
            .field("armed", &self.rollback.is_some())
            .finish_non_exhaustive()
    }
}

impl<F: FnOnce()> TaskGroupGuard<F> {
    /// Arms rollback for `groups`.
    #[must_use]
    pub fn new(groups: &[&TaskGroup], rollback: F) -> Self {
        Self {
            groups: groups
                .iter()
                .map(|group| Arc::clone(&group.inner))
                .collect(),
            rollback: Some(rollback),
        }
    }

    /// Disarms the guard without running rollback.
    pub fn disarm(mut self) {
        self.rollback.take();
    }
}

impl<F: FnOnce()> Drop for TaskGroupGuard<F> {
    fn drop(&mut self) {
        if let Some(rollback) = self.rollback.take() {
            for group in &self.groups {
                group.begin_shutdown();
            }
            rollback();
        }
    }
}

/// A task admission failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSpawnError(&'static str);

impl TaskSpawnError {
    const CLOSED: Self = Self("task group admission is closed");
    const START_FAILED: Self = Self("task stopped before its start gate opened");
}

impl Display for TaskSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl Error for TaskSpawnError {}

/// Indicates that the prior task group generation has not fully drained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskGenerationError(&'static str);

impl TaskGenerationError {
    const NOT_DRAINED: Self = Self("prior task group generation has not fully drained");
}

impl Display for TaskGenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl Error for TaskGenerationError {}

/// A bounded task shutdown failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskShutdownError {
    /// Shutdown was requested before admission closed.
    StillOpen,
    /// All handles drained, but at least one join failed unexpectedly.
    Join(Vec<String>),
    /// Forced completion reached its deadline with tasks still owned.
    Timeout {
        /// Join failures observed before the deadline.
        failures: Vec<String>,
        /// Tasks still owned after the deadline.
        incomplete: usize,
    },
}

impl Display for TaskShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StillOpen => f.write_str("task group admission is still open"),
            Self::Join(failures) => write!(
                f,
                "task shutdown observed join failures: {}",
                failures.join("; ")
            ),
            Self::Timeout {
                failures,
                incomplete,
            } => {
                write!(
                    f,
                    "task shutdown timed out with {incomplete} task(s) still owned"
                )?;

                if !failures.is_empty() {
                    write!(f, ": join failures: {}", failures.join("; "))?;
                }
                Ok(())
            }
        }
    }
}

impl Error for TaskShutdownError {}

/// The observed result of bounded shutdown for one explicitly singular task.
#[derive(Debug)]
#[must_use]
pub enum TaskJoinOutcome<T> {
    /// The task returned before either shutdown deadline.
    Completed(T),
    /// The task was canceled by the forced-abort phase.
    Aborted,
    /// The task failed before or after forced abort.
    Failed(JoinError),
    /// The task did not finish within the supplied bounds and remains owned.
    Incomplete,
}

/// Owns one typed task and its forced-abort state across bounded drain attempts.
///
/// Call [`finish_task`] to observe the join outcome. Dropping the slot requests abort but cannot
/// asynchronously prove termination.
#[derive(Debug)]
pub struct TaskSlot<T> {
    handle: Option<JoinHandle<T>>,
    abort_requested: bool,
}

impl<T> Default for TaskSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> TaskSlot<T> {
    /// Creates an empty task slot.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            handle: None,
            abort_requested: false,
        }
    }

    /// Creates a task slot owning `handle`.
    #[must_use]
    pub const fn from_handle(handle: JoinHandle<T>) -> Self {
        Self {
            handle: Some(handle),
            abort_requested: false,
        }
    }

    /// Spawns and stores a task before its future can be polled.
    ///
    /// # Errors
    ///
    /// Returns an error if the task stops before its start gate opens. The terminal handle remains
    /// owned so its join outcome stays observable.
    ///
    /// # Panics
    ///
    /// Panics if the slot already owns a task.
    #[expect(
        clippy::panic_in_result_fn,
        reason = "an occupied slot is a caller invariant violation"
    )]
    pub fn spawn<F>(&mut self, future: F) -> Result<(), TaskSpawnError>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        assert!(self.handle.is_none(), "task slot is already occupied");
        let (handle, start) = spawn_gated(future);
        self.handle = Some(handle);
        self.abort_requested = false;
        start.send(()).map_err(|()| TaskSpawnError::START_FAILED)
    }

    /// Returns whether the slot owns a task.
    #[must_use]
    pub const fn is_some(&self) -> bool {
        self.handle.is_some()
    }

    /// Returns whether the slot is empty.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        self.handle.is_none()
    }

    /// Returns the owned task handle, when present.
    #[must_use]
    pub const fn as_ref(&self) -> Option<&JoinHandle<T>> {
        self.handle.as_ref()
    }

    /// Stores a task in an empty slot.
    ///
    /// # Panics
    ///
    /// Aborts `handle` and panics if the slot already owns a task.
    pub fn insert(&mut self, handle: JoinHandle<T>) {
        if self.handle.is_some() {
            handle.abort();
            panic!("task slot is already occupied");
        }
        self.handle = Some(handle);
        self.abort_requested = false;
    }

    /// Requests task cancellation and records it as owner-initiated.
    pub fn abort(&mut self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.abort();
            self.abort_requested = true;
        }
    }

    fn complete(&mut self, result: Result<T, JoinError>) -> TaskJoinOutcome<T> {
        let outcome = match result {
            Ok(output) => TaskJoinOutcome::Completed(output),
            Err(e) if e.is_cancelled() && self.abort_requested => TaskJoinOutcome::Aborted,
            Err(e) => TaskJoinOutcome::Failed(e),
        };
        self.handle.take();
        self.abort_requested = false;
        outcome
    }
}

impl<T> Drop for TaskSlot<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.abort();
        }
    }
}

/// Owns one typed task shared by cloned clients.
///
/// Concurrent finish calls are serialized within the caller's graceful and abort durations.
/// Canceling a finish or reaching the forced-completion deadline retains the task for another drain
/// attempt. Dropping the owner requests abort but cannot asynchronously prove termination.
#[derive(Debug)]
pub struct SharedTaskSlot<T> {
    state: Mutex<SharedTaskState<T>>,
    drain_lock: tokio::sync::Mutex<()>,
    owned: AtomicBool,
}

#[derive(Debug)]
struct SharedTaskState<T> {
    slot: TaskSlot<T>,
    abort: CancellationToken,
    abort_requested: bool,
    draining: bool,
}

impl<T> SharedTaskState<T> {
    fn try_reserve_drain(&mut self) -> Option<(TaskSlot<T>, CancellationToken, bool)> {
        if self.draining {
            return None;
        }
        self.draining = true;
        Some((
            std::mem::take(&mut self.slot),
            self.abort.clone(),
            self.abort_requested,
        ))
    }
}

impl<T> Default for SharedTaskSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SharedTaskSlot<T> {
    /// Creates an empty shared task slot.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SharedTaskState {
                slot: TaskSlot::new(),
                abort: CancellationToken::new(),
                abort_requested: false,
                draining: false,
            }),
            drain_lock: tokio::sync::Mutex::const_new(()),
            owned: AtomicBool::new(false),
        }
    }

    /// Returns whether the slot owns no task, including while a drain is pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.owned.load(Ordering::Acquire)
    }

    /// Returns whether the owned task has finished without being joined.
    ///
    /// Returns `false` while a drain owns the handle and its state is unavailable.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        if self.is_empty() {
            return false;
        }

        let state = self.state.lock();
        !state.draining && state.slot.as_ref().is_some_and(JoinHandle::is_finished)
    }

    /// Stores a task in an empty slot.
    ///
    /// # Panics
    ///
    /// Aborts `handle` and panics if the slot already owns a task or is draining one.
    pub fn insert(&self, handle: JoinHandle<T>) {
        self.insert_slot(TaskSlot::from_handle(handle));
    }

    // panics-doc-ok (transitive via insert on an occupied or draining slot)
    /// Spawns and stores a task before its future can be polled.
    ///
    /// # Errors
    ///
    /// Returns an error if the task stops before its start gate opens. The terminal handle remains
    /// owned so its join outcome stays observable.
    ///
    /// # Panics
    ///
    /// Panics if the slot already owns a task or is draining one.
    pub fn spawn<F>(&self, future: F) -> Result<(), TaskSpawnError>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let (handle, start) = spawn_gated(future);
        self.insert(handle);
        start.send(()).map_err(|()| TaskSpawnError::START_FAILED)
    }

    fn insert_slot(&self, slot: TaskSlot<T>) {
        assert!(
            self.try_insert_slot(slot).is_ok(),
            "shared task slot is already occupied"
        );
    }

    /// Transfers a task slot into this owner if it is empty.
    ///
    /// An empty input slot is always accepted. Returns a nonempty input unchanged when this owner
    /// already has a task or is draining one.
    ///
    /// # Errors
    ///
    /// Returns the input slot when this owner already has a task or is draining one.
    pub fn try_insert_slot(&self, slot: TaskSlot<T>) -> Result<(), TaskSlot<T>> {
        if slot.is_none() {
            return Ok(());
        }

        let mut state = self.state.lock();
        if self.owned.load(Ordering::Acquire) || state.slot.is_some() || state.draining {
            return Err(slot);
        }
        self.owned.store(true, Ordering::Release);
        state.slot = slot;
        state.abort = CancellationToken::new();
        state.abort_requested = false;
        Ok(())
    }

    /// Requests task cancellation and records it as owner-initiated.
    ///
    /// # Panics
    ///
    /// Panics if the shared task slot changes while cancellation temporarily drains it.
    pub fn abort(&self) {
        let (mut slot, abort, moved_slot) = {
            let mut state = self.state.lock();
            state.abort_requested = true;
            let moved_slot = !state.draining && state.slot.is_some();
            if moved_slot {
                state.draining = true;
            }
            let slot = if moved_slot {
                std::mem::take(&mut state.slot)
            } else {
                TaskSlot::new()
            };
            (slot, state.abort.clone(), moved_slot)
        };

        slot.abort();
        abort.cancel();

        if moved_slot {
            let mut state = self.state.lock();
            assert!(
                state.slot.is_none(),
                "shared task slot changed while aborting"
            );
            state.slot = slot;
            state.draining = false;
        }
    }

    /// Gracefully joins the task, then aborts and joins it within a second bound.
    ///
    /// If another caller holds the drain lock through both bounds, returns
    /// [`TaskJoinOutcome::Incomplete`] when a task remains owned.
    pub async fn finish(
        &self,
        graceful_timeout: Duration,
        abort_timeout: Duration,
    ) -> Option<TaskJoinOutcome<T>> {
        let started = time::Instant::now();
        let Some(graceful_deadline) = started.checked_add(graceful_timeout) else {
            return self.incomplete_outcome();
        };
        let Some(abort_deadline) = graceful_deadline.checked_add(abort_timeout) else {
            return self.incomplete_outcome();
        };
        let lock_timeout = abort_deadline.saturating_duration_since(time::Instant::now());
        let Ok(_drain_lock) = time::timeout(lock_timeout, self.drain_lock.lock()).await else {
            return self.incomplete_outcome();
        };

        let reserve_timeout = abort_deadline.saturating_duration_since(time::Instant::now());
        let Ok((slot, abort, abort_requested)) = time::timeout(reserve_timeout, async {
            loop {
                if let Some(reservation) = self.state.lock().try_reserve_drain() {
                    break reservation;
                }
                nautilus_common::live::dst::task::yield_now().await;
            }
        })
        .await
        else {
            return self.incomplete_outcome();
        };

        if slot.is_none() {
            let mut state = self.state.lock();
            self.owned.store(false, Ordering::Release);
            state.draining = false;
            return None;
        }

        let mut draining = SharedTaskDrain { owner: self, slot };
        let aborting = abort_requested || abort.is_cancelled();
        if aborting {
            draining.slot.abort();
        }

        let graceful_remaining = graceful_deadline.saturating_duration_since(time::Instant::now());
        let outcome = if aborting {
            let abort_remaining = abort_deadline.saturating_duration_since(time::Instant::now());
            finish_task(&mut draining.slot, Duration::ZERO, abort_remaining).await
        } else {
            let graceful = {
                let abort_remaining = abort_deadline
                    .saturating_duration_since(graceful_deadline.max(time::Instant::now()));
                let finish = finish_task(&mut draining.slot, graceful_remaining, abort_remaining);
                tokio::pin!(finish);
                tokio::select! {
                    biased;
                    outcome = &mut finish => Some(outcome),
                    () = abort.cancelled() => None,
                }
            };

            if let Some(outcome) = graceful {
                outcome
            } else {
                draining.slot.abort();
                let abort_remaining =
                    abort_deadline.saturating_duration_since(time::Instant::now());
                finish_task(&mut draining.slot, Duration::ZERO, abort_remaining).await
            }
        };
        drop(draining);
        outcome
    }

    fn incomplete_outcome(&self) -> Option<TaskJoinOutcome<T>> {
        (!self.is_empty()).then_some(TaskJoinOutcome::Incomplete)
    }
}

impl<T> Drop for SharedTaskSlot<T> {
    fn drop(&mut self) {
        self.state.get_mut().slot.abort();
    }
}

struct SharedTaskDrain<'a, T> {
    owner: &'a SharedTaskSlot<T>,
    slot: TaskSlot<T>,
}

impl<T> Drop for SharedTaskDrain<'_, T> {
    fn drop(&mut self) {
        let owns_task = self.slot.is_some();
        let mut abort_applied = false;

        loop {
            let mut state = self.owner.state.lock();
            if owns_task && !abort_applied && (state.abort_requested || state.abort.is_cancelled())
            {
                drop(state);
                self.slot.abort();
                abort_applied = true;
                continue;
            }
            assert!(
                state.slot.is_none(),
                "shared task slot changed while draining"
            );
            state.slot = std::mem::take(&mut self.slot);
            state.draining = false;
            self.owner.owned.store(owns_task, Ordering::Release);
            break;
        }
    }
}

/// Gracefully joins one task, then aborts and joins it within a second bound.
///
/// This preserves typed task results. The handle and forced-abort state remain in `slot` while the
/// function is pending, so canceling the finish future cannot detach the task or lose its expected
/// cancellation provenance. A terminal join clears the slot, while a second timeout leaves the
/// incomplete task in place for another drain attempt.
pub async fn finish_task<T>(
    slot: &mut TaskSlot<T>,
    graceful_timeout: Duration,
    abort_timeout: Duration,
) -> Option<TaskJoinOutcome<T>> {
    let graceful_result = {
        let handle = slot.handle.as_mut()?;
        time::timeout(graceful_timeout, handle).await
    };

    match graceful_result {
        Ok(result) => Some(slot.complete(result)),
        Err(_) => {
            slot.abort();
            let abort_result = {
                let handle = slot.handle.as_mut()?;
                time::timeout(abort_timeout, handle).await
            };

            match abort_result {
                Ok(result) => Some(slot.complete(result)),
                Err(_) => Some(TaskJoinOutcome::Incomplete),
            }
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskGroupPhase {
    Open,
    Closing,
    Drained,
}

impl TaskGroupPhase {
    fn load(value: &TaskState) -> Self {
        match value.load(Ordering::Acquire) {
            0 => Self::Open,
            1 => Self::Closing,
            2 => Self::Drained,
            value => unreachable!("invalid task group phase {value}"),
        }
    }
}

#[derive(Debug)]
struct TaskGeneration {
    phase: TaskState,
    admission_lock: parking_lot::Mutex<()>,
    cancellation: CancellationToken,
    force: CancellationToken,
    tasks: TaskTracker,
    failures: Mutex<Vec<String>>,
}

impl TaskGeneration {
    fn new() -> Self {
        Self {
            phase: TaskState::new(TaskGroupPhase::Open as u8),
            admission_lock: parking_lot::Mutex::new(()),
            cancellation: CancellationToken::new(),
            force: CancellationToken::new(),
            tasks: TaskTracker::new(),
            failures: Mutex::new(Vec::new()),
        }
    }

    fn phase(&self) -> TaskGroupPhase {
        TaskGroupPhase::load(&self.phase)
    }

    fn is_open(&self) -> bool {
        self.phase() == TaskGroupPhase::Open
    }

    fn close_admission(&self) {
        let _guard = self.admission_lock.lock();
        let _ = self
            .phase
            .try_update(Ordering::AcqRel, Ordering::Acquire, |phase| match phase {
                value
                    if value == TaskGroupPhase::Open as u8
                        || value == TaskGroupPhase::Drained as u8 =>
                {
                    Some(TaskGroupPhase::Closing as u8)
                }
                value if value == TaskGroupPhase::Closing as u8 => None,
                value => unreachable!("invalid task group phase {value}"),
            });
    }

    fn cancel(&self) {
        self.tasks.close();
        self.cancellation.cancel();
    }

    fn spawn<F>(self: &Arc<Self>, future: F) -> Result<(), TaskSpawnError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let registration = match self.register_task(None) {
            Ok(registration) => registration,
            Err(e) => {
                drop(future);
                return Err(e);
            }
        };

        self.spawn_registered(registration, future);

        Ok(())
    }

    fn spawn_named<F>(
        self: &Arc<Self>,
        name: &'static str,
        future: F,
    ) -> Result<TaskRef, TaskSpawnError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let identity = Arc::new(TaskIdentity::new(name));
        let task = TaskRef {
            identity: Arc::clone(&identity),
        };
        let registration = match self.register_task(Some(identity)) {
            Ok(registration) => registration,
            Err(e) => {
                drop(future);
                return Err(e);
            }
        };

        self.spawn_registered(registration, future);

        Ok(task)
    }

    fn spawn_registered<F>(&self, registration: TaskRegistration, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let force = self.force.clone();

        spawn(async move {
            let result = AssertUnwindSafe(async move {
                tokio::select! {
                    biased;
                    () = force.cancelled() => {}
                    () = future => {}
                }
            })
            .catch_unwind()
            .await;
            registration.complete(result.err());
        });
    }

    fn register_task(
        self: &Arc<Self>,
        identity: Option<Arc<TaskIdentity>>,
    ) -> Result<TaskRegistration, TaskSpawnError> {
        let _guard = self.admission_lock.lock();

        if !self.is_open() {
            return Err(TaskSpawnError::CLOSED);
        }

        let token = self.tasks.token();
        Ok(TaskRegistration::new(Arc::clone(self), token, identity))
    }

    fn record_failure(&self, failure: String) {
        self.lock_failures().push(failure);
    }

    fn take_failures(&self) -> Vec<String> {
        std::mem::take(&mut *self.lock_failures())
    }

    fn timeout_error(&self) -> TaskShutdownError {
        TaskShutdownError::Timeout {
            failures: self.lock_failures().clone(),
            incomplete: self.tasks.len(),
        }
    }

    fn complete_shutdown(&self) -> Result<(), TaskShutdownError> {
        self.phase
            .store(TaskGroupPhase::Drained as u8, Ordering::Release);
        let failures = self.take_failures();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(TaskShutdownError::Join(failures))
        }
    }

    fn lock_failures(&self) -> MutexGuard<'_, Vec<String>> {
        self.failures.lock()
    }
}

struct TaskRegistration {
    generation: Arc<TaskGeneration>,
    _token: TaskTrackerToken,
    identity: Option<Arc<TaskIdentity>>,
    terminal: bool,
}

impl TaskRegistration {
    fn new(
        generation: Arc<TaskGeneration>,
        token: TaskTrackerToken,
        identity: Option<Arc<TaskIdentity>>,
    ) -> Self {
        Self {
            generation,
            _token: token,
            identity,
            terminal: false,
        }
    }

    fn complete(mut self, panic: Option<Box<dyn Any + Send>>) {
        self.terminal = true;
        self.finish();

        if let Some(panic) = panic {
            let failure = format!("panicked: {}", panic_message(panic.as_ref()));
            self.record_failure(&failure);
        }
    }

    fn finish(&self) {
        if let Some(identity) = &self.identity {
            identity.finish();
        }
    }

    fn record_failure(&self, failure: &str) {
        let failure = self.identity.as_ref().map_or_else(
            || format!("task {failure}"),
            |identity| identity.failure(failure),
        );
        self.generation.record_failure(failure);
    }
}

impl Drop for TaskRegistration {
    fn drop(&mut self) {
        self.finish();

        if !self.terminal && !self.generation.force.is_cancelled() {
            self.record_failure("was canceled unexpectedly");
        }
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "non-string panic payload"
    }
}

#[derive(Debug)]
struct TaskGroupInner {
    generation: ArcSwap<TaskGeneration>,
    generation_lock: parking_lot::Mutex<()>,
    drain_lock: tokio::sync::Mutex<()>,
}

impl TaskGroupInner {
    fn current(&self) -> Arc<TaskGeneration> {
        self.generation.load_full()
    }

    fn begin_shutdown(&self) -> Arc<TaskGeneration> {
        let generation = {
            let _guard = self.generation_lock.lock();
            let generation = self.current();
            generation.close_admission();
            generation
        };
        generation.cancel();
        generation
    }

    fn start_generation(&self) -> Result<(), TaskGenerationError> {
        let _guard = self.generation_lock.lock();
        let current = self.current();
        if current.phase() != TaskGroupPhase::Drained || !current.tasks.is_empty() {
            return Err(TaskGenerationError::NOT_DRAINED);
        }

        self.generation.store(Arc::new(TaskGeneration::new()));
        Ok(())
    }
}

fn spawn_gated<F>(future: F) -> (JoinHandle<F::Output>, tokio::sync::oneshot::Sender<()>)
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let (start, wait) = tokio::sync::oneshot::channel();
    let handle = spawn(async move {
        wait.await.expect("task start gate sender dropped");
        future.await
    });
    (handle, start)
}

#[cfg(all(feature = "simulation", madsim))]
fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    nautilus_common::live::dst::task::spawn(future)
}

#[cfg(not(all(feature = "simulation", madsim)))]
fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    nautilus_common::live::get_runtime().spawn(future)
}

#[cfg(test)]
mod tests {
    use std::{
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        task::{Context, Poll, Wake, Waker},
    };

    use nautilus_common::live::dst::task;
    use rstest::rstest;

    use super::*;

    const TEST_TIMEOUT: Duration = Duration::from_secs(1);

    #[rstest]
    fn task_group_guard_closes_all_groups_and_runs_rollback_until_disarmed() {
        let first = Arc::new(TaskGroup::new());
        let second = Arc::new(TaskGroup::new());
        let rolled_back = Arc::new(AtomicBool::new(false));
        let first_on_drop = Arc::clone(&first);
        let second_on_drop = Arc::clone(&second);
        let rolled_back_on_drop = Arc::clone(&rolled_back);

        drop(TaskGroupGuard::new(&[&first, &second], move || {
            assert!(!first_on_drop.is_open());
            assert!(!second_on_drop.is_open());
            rolled_back_on_drop.store(true, Ordering::Release);
        }));

        assert!(!first.is_open());
        assert!(!second.is_open());
        assert!(rolled_back.load(Ordering::Acquire));

        let group = Arc::new(TaskGroup::new());
        let rolled_back = Arc::new(AtomicBool::new(false));
        let rolled_back_on_drop = Arc::clone(&rolled_back);
        TaskGroupGuard::new(&[&group], move || {
            rolled_back_on_drop.store(true, Ordering::Release);
        })
        .disarm();

        assert!(group.is_open());
        assert!(!rolled_back.load(Ordering::Acquire));
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn task_is_registered_before_first_poll() {
        let group = TaskGroup::new();
        let observed = Arc::new(AtomicUsize::new(0));
        let observed_task = Arc::clone(&observed);
        let generation = group.inner.current();

        group
            .spawn(async move {
                observed_task.store(generation.tasks.len(), Ordering::Release);
            })
            .expect("spawn");

        time::timeout(TEST_TIMEOUT, async {
            while observed.load(Ordering::Acquire) == 0 {
                task::yield_now().await;
            }
        })
        .await
        .expect("task should poll");

        group.begin_shutdown();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");
        assert_eq!(observed.load(Ordering::Acquire), 1);
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn named_tasks_expose_distinct_identity_without_transferring_ownership() {
        let group = TaskGroup::new();
        let first = group
            .spawn_named("dispatch", std::future::pending())
            .expect("spawn first task");
        let second = group
            .spawner()
            .expect("task spawner")
            .spawn_named("dispatch", std::future::pending())
            .expect("spawn second task");
        let first_observer = first.clone();
        drop(first);

        assert_eq!(first_observer.name(), "dispatch");
        assert_eq!(second.name(), "dispatch");
        assert_ne!(first_observer.id(), second.id());
        assert!(first_observer.is_active());
        assert!(second.is_active());
        assert!(!first_observer.is_finished());
        assert!(!second.is_finished());
        assert_eq!(group.len(), 2);

        group.abort();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");

        assert!(!first_observer.is_active());
        assert!(!second.is_active());
        assert!(first_observer.is_finished());
        assert!(second.is_finished());
        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn named_task_ids_are_unique_across_groups_and_generations() {
        let first_group = TaskGroup::new();
        let first = first_group
            .spawn_named("dispatch", std::future::pending())
            .expect("spawn first task");
        first_group.abort();
        first_group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("first generation shutdown");
        first_group
            .start_generation()
            .expect("replacement generation");

        let second = first_group
            .spawn_named("dispatch", std::future::pending())
            .expect("spawn replacement task");
        let second_group = TaskGroup::new();
        let third = second_group
            .spawn_named("dispatch", std::future::pending())
            .expect("spawn other group task");

        first_group.abort();
        first_group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("replacement generation shutdown");
        second_group.abort();
        second_group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("other group shutdown");

        assert_ne!(first.id(), second.id());
        assert_ne!(first.id(), third.id());
        assert_ne!(second.id(), third.id());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn dropping_named_task_ref_preserves_group_ownership() {
        let group = TaskGroup::new();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (request_tx, request_rx) = tokio::sync::oneshot::channel();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let task = group
            .spawn_named("dispatch", async move {
                let _ = started_tx.send(());
                let _ = request_rx.await;
                let _ = response_tx.send(());
                std::future::pending::<()>().await;
            })
            .expect("spawn task");
        started_rx.await.expect("task should start");

        drop(task);
        request_tx.send(()).expect("task should remain owned");
        time::timeout(TEST_TIMEOUT, response_rx)
            .await
            .expect("task should respond")
            .expect("task should remain active");

        assert_eq!(group.len(), 1);

        group.abort();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");

        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn named_task_observes_normal_completion() {
        let group = TaskGroup::new();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let task = group
            .spawn_named("finite", async move {
                let _ = release_rx.await;
            })
            .expect("spawn task");

        release_tx.send(()).expect("task should be waiting");
        time::timeout(TEST_TIMEOUT, async {
            while !task.is_finished() {
                task::yield_now().await;
            }
        })
        .await
        .expect("task should finish");

        group.begin_shutdown();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");

        assert!(!task.is_active());
        assert!(task.is_finished());
        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn named_task_panic_includes_identity() {
        let group = TaskGroup::new();
        let task = group
            .spawn_named("panicking", async {
                panic!("task panic");
            })
            .expect("spawn task");

        group.begin_shutdown();
        let error = group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect_err("panic should be reported");
        let TaskShutdownError::Join(failures) = error else {
            panic!("expected join failure");
        };

        assert_eq!(
            failures,
            [format!(
                "task 'panicking' ({}) panicked: task panic",
                task.id()
            )]
        );
        assert!(task.is_finished());
        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn named_task_forced_before_first_poll_becomes_finished() {
        let group = TaskGroup::new();
        let generation = group.inner.current();
        let identity = Arc::new(TaskIdentity::new("not-polled"));
        let task = TaskRef {
            identity: Arc::clone(&identity),
        };
        let registration = generation
            .register_task(Some(identity))
            .expect("registration");
        let polled = Arc::new(AtomicBool::new(false));
        let polled_task = Arc::clone(&polled);

        assert!(task.is_active());
        assert!(!task.is_finished());

        group.abort();
        generation.spawn_registered(registration, async move {
            polled_task.store(true, Ordering::Release);
        });

        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");

        assert!(!polled.load(Ordering::Acquire));
        assert!(!task.is_active());
        assert!(task.is_finished());
        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn named_registration_drop_after_forced_cancellation_is_not_reported() {
        let group = TaskGroup::new();
        let generation = group.inner.current();
        let identity = Arc::new(TaskIdentity::new("canceled"));
        let task = TaskRef {
            identity: Arc::clone(&identity),
        };
        let registration = generation
            .register_task(Some(identity))
            .expect("registration");

        group.abort();
        drop(registration);
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("forced cancellation should not be reported");

        assert!(!task.is_active());
        assert!(task.is_finished());
        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn rejected_named_task_drops_future_without_polling() {
        let group = Arc::new(TaskGroup::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let polled = Arc::new(AtomicBool::new(false));
        group.begin_shutdown();

        let result = group.spawn_named(
            "rejected",
            ReentrantDropFuture {
                group: Arc::clone(&group),
                dropped: Arc::clone(&dropped),
                polled: Arc::clone(&polled),
            },
        );

        assert!(matches!(result, Err(TaskSpawnError::CLOSED)));
        assert!(dropped.load(Ordering::Acquire));
        assert!(!polled.load(Ordering::Acquire));
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shutdown_closes_admission_before_future_poll() {
        let group = TaskGroup::new();
        let polled = Arc::new(AtomicBool::new(false));
        let polled_task = Arc::clone(&polled);

        group.begin_shutdown();
        let result = group.spawn(async move {
            polled_task.store(true, Ordering::Release);
        });

        assert!(matches!(group.spawner(), Err(TaskSpawnError::CLOSED)));
        assert_eq!(result, Err(TaskSpawnError::CLOSED));
        assert!(!polled.load(Ordering::Acquire));
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn rejected_future_can_reenter_group_on_drop() {
        let group = Arc::new(TaskGroup::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let polled = Arc::new(AtomicBool::new(false));
        group.begin_shutdown();

        let result = group.spawn(ReentrantDropFuture {
            group: Arc::clone(&group),
            dropped: Arc::clone(&dropped),
            polled: Arc::clone(&polled),
        });

        assert_eq!(result, Err(TaskSpawnError::CLOSED));
        assert!(dropped.load(Ordering::Acquire));
        assert!(!polled.load(Ordering::Acquire));
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");
    }

    #[rstest]
    fn shutdown_cancellation_wake_can_reenter_group() {
        let group = Arc::new(TaskGroup::new());
        let cancellation = group.cancellation_token();
        let woke = Arc::new(AtomicBool::new(false));
        let waker = Waker::from(Arc::new(ReentrantWake {
            group: Arc::clone(&group),
            woke: Arc::clone(&woke),
        }));
        let mut context = Context::from_waker(&waker);
        let mut cancelled = Box::pin(cancellation.cancelled());

        assert!(matches!(
            Pin::as_mut(&mut cancelled).poll(&mut context),
            Poll::Pending
        ));
        group.begin_shutdown();

        assert!(woke.load(Ordering::Acquire));
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn stale_spawner_cannot_spawn_into_new_generation() {
        let group = TaskGroup::new();
        let old = group.spawner().expect("old spawner");
        let old_generation = Arc::clone(&old.generation);
        group.begin_shutdown();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");

        assert_eq!(old.spawn(async {}), Err(TaskSpawnError::CLOSED));
        assert!(matches!(
            old.spawn_named("stale", async {}),
            Err(TaskSpawnError::CLOSED)
        ));
        assert!(old_generation.tasks.is_empty());

        group.start_generation().expect("new generation");
        let current = group.spawner().expect("current spawner");

        current
            .spawn_named("current", async {})
            .expect("current spawn");

        group.begin_shutdown();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shutdown_allows_graceful_cancellation_cleanup() {
        let group = TaskGroup::new();
        let cancellation = group
            .spawner()
            .expect("task group spawner")
            .cancellation_token();
        let cleaned = Arc::new(AtomicBool::new(false));
        let cleaned_task = Arc::clone(&cleaned);
        group
            .spawn(async move {
                cancellation.cancelled().await;
                cleaned_task.store(true, Ordering::Release);
            })
            .expect("spawn");

        group.begin_shutdown();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");

        assert!(cleaned.load(Ordering::Acquire));
        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shutdown_forces_abort_and_observes_canceled_join() {
        let group = TaskGroup::new();
        let (future, started_rx, dropped) = pending_with_drop_signal();

        group.spawn(future).expect("spawn");
        started_rx.await.expect("task should start");

        group.begin_shutdown();
        group
            .finish_shutdown(Duration::ZERO, TEST_TIMEOUT)
            .await
            .expect("shutdown");

        assert!(dropped.load(Ordering::Acquire));
        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn abort_closes_admission_and_cancels_tasks() {
        let group = TaskGroup::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let drop_signal = DropSignal(Arc::clone(&dropped));
        group
            .spawn(async move {
                let _drop_signal = drop_signal;
                std::future::pending::<()>().await;
            })
            .expect("spawn");

        group.abort();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");

        assert!(!group.is_open());
        assert!(group.is_empty());
        assert!(dropped.load(Ordering::Acquire));
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn dropping_task_group_aborts_owned_tasks() {
        let group = TaskGroup::new();
        let (future, started_rx, dropped) = pending_with_drop_signal();

        group.spawn(future).expect("spawn");
        started_rx.await.expect("task should start");

        drop(group);
        wait_for_drop(&dropped).await;
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn unexpected_task_cancellation_is_reported() {
        let group = TaskGroup::new();
        group.spawn(std::future::pending()).expect("spawn");
        let generation = group.inner.current();
        let registration =
            TaskRegistration::new(Arc::clone(&generation), generation.tasks.token(), None);
        drop(registration);
        group.begin_shutdown();

        let error = group
            .finish_shutdown(Duration::ZERO, TEST_TIMEOUT)
            .await
            .expect_err("unexpected cancellation should be reported");
        let TaskShutdownError::Join(failures) = error else {
            panic!("expected join failure");
        };
        assert_eq!(failures, ["task was canceled unexpectedly"]);
        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn named_unexpected_cancellation_includes_identity() {
        let group = TaskGroup::new();
        group.spawn(std::future::pending()).expect("spawn");
        let generation = group.inner.current();
        let identity = Arc::new(TaskIdentity::new("canceled"));
        let task = TaskRef {
            identity: Arc::clone(&identity),
        };
        let registration = generation
            .register_task(Some(identity))
            .expect("registration");
        drop(registration);
        group.begin_shutdown();

        let error = group
            .finish_shutdown(Duration::ZERO, TEST_TIMEOUT)
            .await
            .expect_err("unexpected cancellation should be reported");
        let TaskShutdownError::Join(failures) = error else {
            panic!("expected join failure");
        };

        assert_eq!(
            failures,
            [format!(
                "task 'canceled' ({}) was canceled unexpectedly",
                task.id()
            )]
        );
        assert!(task.is_finished());
        assert!(group.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn late_child_is_rejected_after_parent_observes_shutdown() {
        let group = TaskGroup::new();
        let spawner = group.spawner().expect("spawner");
        let cancellation = spawner.cancellation_token();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        group
            .spawn(async move {
                cancellation.cancelled().await;

                let _ = result_tx.send(spawner.spawn(async {}));
            })
            .expect("spawn parent");

        group.begin_shutdown();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");

        assert_eq!(
            result_rx.await.expect("late spawn result"),
            Err(TaskSpawnError::CLOSED),
        );
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shutdown_drains_accepted_registration_and_rejects_late_registration() {
        let group = TaskGroup::new();
        let generation = group.inner.current();
        let registration = generation.register_task(None).expect("registration");

        group.begin_shutdown();
        assert!(matches!(
            generation.register_task(None),
            Err(TaskSpawnError::CLOSED)
        ));
        assert_eq!(group.len(), 1);

        let error = group
            .finish_shutdown(Duration::ZERO, Duration::ZERO)
            .await
            .expect_err("accepted registration should prevent completed shutdown");
        assert!(matches!(
            error,
            TaskShutdownError::Timeout { incomplete: 1, .. }
        ));
        assert_eq!(
            group.start_generation(),
            Err(TaskGenerationError::NOT_DRAINED)
        );

        registration.complete(None);
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("retry shutdown");
        group.start_generation().expect("new generation");
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn generation_cannot_reopen_before_finish_shutdown() {
        let group = TaskGroup::new();
        group.begin_shutdown();

        assert!(matches!(
            group.start_generation(),
            Err(TaskGenerationError::NOT_DRAINED),
        ));

        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("shutdown");
        group.start_generation().expect("new generation");
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shutdown_after_drain_requires_another_finish_before_reopening() {
        let group = TaskGroup::new();
        group.begin_shutdown();
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("initial shutdown");

        group.begin_shutdown();

        assert_eq!(
            group.start_generation(),
            Err(TaskGenerationError::NOT_DRAINED),
        );

        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("repeated shutdown");
        group.start_generation().expect("new generation");
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn canceled_finish_preserves_tracked_task() {
        let group = Arc::new(TaskGroup::new());
        group.spawn(std::future::pending()).expect("spawn");
        group.begin_shutdown();
        let finishing_group = Arc::clone(&group);
        let finish = task::spawn(async move {
            finishing_group
                .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
                .await
        });

        time::timeout(TEST_TIMEOUT, async {
            while group.inner.drain_lock.try_lock().is_ok() {
                task::yield_now().await;
            }
        })
        .await
        .expect("finisher should begin draining");
        finish.abort();
        let _ = finish.await;

        assert_eq!(group.len(), 1);
        assert!(!group.is_empty());

        group
            .finish_shutdown(Duration::ZERO, TEST_TIMEOUT)
            .await
            .expect("retry shutdown");
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test]
    async fn canceled_finish_preserves_observed_join_failures() {
        let group = TaskGroup::new();
        let (panicking_tx, panicking_rx) = tokio::sync::oneshot::channel();
        group
            .spawn(async move {
                let _ = panicking_tx.send(());
                panic!("task panic");
            })
            .expect("spawn panicking task");
        panicking_rx.await.expect("panicking task should start");
        group
            .spawn(std::future::pending())
            .expect("spawn pending task");
        group.begin_shutdown();

        loop {
            let finish = group.finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT);
            tokio::pin!(finish);
            tokio::select! {
                biased;
                outcome = &mut finish => panic!("finish completed unexpectedly: {outcome:?}"),
                () = task::yield_now() => {}
            }

            if group.inner.current().lock_failures().len() == 1 {
                break;
            }
        }

        let error = group
            .finish_shutdown(Duration::ZERO, TEST_TIMEOUT)
            .await
            .expect_err("panic should remain observable");
        let TaskShutdownError::Join(failures) = error else {
            panic!("expected join failure");
        };
        assert_eq!(failures.len(), 1);
        assert!(group.is_empty());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn canceled_finish_preserves_forced_abort_classification() {
        let group = TaskGroup::new();
        let generation = group.inner.current();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        group
            .spawn(async move {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                task::yield_now().await;
            })
            .expect("spawn blocking task");
        started_rx.await.expect("blocking task should start");
        group.begin_shutdown();

        loop {
            {
                let finish = group.finish_shutdown(Duration::ZERO, TEST_TIMEOUT);
                tokio::pin!(finish);
                tokio::select! {
                    biased;
                    outcome = &mut finish => panic!("finish completed unexpectedly: {outcome:?}"),
                    () = task::yield_now() => {}
                }
            }

            if generation.force.is_cancelled() {
                break;
            }
        }

        assert!(generation.force.is_cancelled());
        release_tx
            .send(())
            .expect("blocking task should be waiting");
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("retry shutdown");
        assert!(group.is_empty());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_finish_respects_its_own_shutdown_bound() {
        let group = Arc::new(TaskGroup::new());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        group
            .spawn(async move {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
            })
            .expect("spawn blocking task");
        started_rx.await.expect("blocking task should start");
        group.begin_shutdown();
        let finishing_group = Arc::clone(&group);
        let finish = task::spawn(async move {
            finishing_group
                .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
                .await
        });

        time::timeout(TEST_TIMEOUT, async {
            while group.inner.drain_lock.try_lock().is_ok() {
                task::yield_now().await;
            }
        })
        .await
        .expect("first finisher should hold the drain lock");

        let error = group
            .finish_shutdown(Duration::ZERO, Duration::ZERO)
            .await
            .expect_err("second finisher should exhaust its own bound");
        let TaskShutdownError::Timeout { incomplete, .. } = error else {
            panic!("expected shutdown timeout");
        };
        assert_eq!(incomplete, 1);

        release_tx
            .send(())
            .expect("blocking task should be waiting");
        finish
            .await
            .expect("first finisher should join")
            .expect("first shutdown should complete");
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn queued_finish_remains_bound_to_original_generation() {
        let group = TaskGroup::new();
        group.begin_shutdown();
        let generation = group.inner.current();
        let drain_lock = group.inner.drain_lock.lock().await;
        let mut finish = Box::pin(group.finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT));

        assert!(matches!(
            futures_util::poll!(finish.as_mut()),
            Poll::Pending,
        ));

        generation
            .complete_shutdown()
            .expect("original generation should drain");
        group.start_generation().expect("replacement generation");
        drop(drain_lock);

        finish
            .await
            .expect("queued finish should observe the original generation");
        assert!(group.is_open());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn forced_timeout_retains_task_and_in_flight_registration() {
        let group = Arc::new(TaskGroup::new());
        let generation = group.inner.current();
        let registration =
            TaskRegistration::new(Arc::clone(&generation), generation.tasks.token(), None);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        group
            .spawn(async move {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
            })
            .expect("spawn blocking task");
        started_rx.await.expect("blocking task should start");
        group.begin_shutdown();

        let finishing_group = Arc::clone(&group);
        let finish = task::spawn(async move {
            finishing_group
                .finish_shutdown(Duration::ZERO, TEST_TIMEOUT)
                .await
        });
        time::timeout(TEST_TIMEOUT, async {
            while !generation.force.is_cancelled() {
                task::yield_now().await;
            }
        })
        .await
        .expect("finisher should request forced cancellation");

        let error = finish
            .await
            .expect("finisher should join")
            .expect_err("blocking task should exceed abort deadline");
        let TaskShutdownError::Timeout { incomplete, .. } = error else {
            panic!("expected shutdown timeout");
        };
        assert_eq!(incomplete, 2);
        assert_eq!(group.len(), 2);

        release_tx.send(()).expect("release blocking task");
        registration.complete(None);
        group
            .finish_shutdown(Duration::ZERO, TEST_TIMEOUT)
            .await
            .expect("retry shutdown");
        assert!(group.is_empty());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stuck_head_does_not_hide_later_join_failure() {
        let group = TaskGroup::new();
        let generation = group.inner.current();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        group
            .spawn(async move {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
            })
            .expect("spawn blocking task");
        started_rx.await.expect("blocking task should start");
        group
            .spawn(async {
                panic!("later task panic");
            })
            .expect("spawn panicking task");
        time::timeout(TEST_TIMEOUT, async {
            while generation.lock_failures().is_empty() {
                task::yield_now().await;
            }
        })
        .await
        .expect("panicking task should finish");
        group.begin_shutdown();

        let error = group
            .finish_shutdown(Duration::ZERO, Duration::from_millis(10))
            .await
            .expect_err("blocking task should exceed abort deadline");
        let TaskShutdownError::Timeout {
            failures,
            incomplete,
        } = error
        else {
            panic!("expected shutdown timeout");
        };
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("later task panic"));
        assert_eq!(incomplete, 1);
        assert_eq!(group.len(), 1);

        release_tx.send(()).expect("release blocking task");
        group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("retry shutdown");
        assert!(group.is_empty());
    }

    #[rstest]
    #[case(Duration::MAX, Duration::ZERO)]
    #[case(Duration::ZERO, Duration::MAX)]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn unrepresentable_deadline_retains_owned_task(
        #[case] graceful_timeout: Duration,
        #[case] abort_timeout: Duration,
    ) {
        let group = TaskGroup::new();
        group.spawn(std::future::pending()).expect("spawn");
        group.begin_shutdown();

        let error = group
            .finish_shutdown(graceful_timeout, abort_timeout)
            .await
            .expect_err("deadline should be rejected");

        assert!(matches!(
            error,
            TaskShutdownError::Timeout { incomplete: 1, .. }
        ));
        assert_eq!(group.len(), 1);
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn unrepresentable_deadline_reports_open_group() {
        let group = TaskGroup::new();

        let error = group
            .finish_shutdown(Duration::MAX, Duration::ZERO)
            .await
            .expect_err("open group should reject shutdown completion");

        assert!(matches!(error, TaskShutdownError::StillOpen));
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test]
    async fn panic_join_is_reported_after_group_drains() {
        let group = TaskGroup::new();
        group
            .spawn(async {
                panic!("task panic");
            })
            .expect("spawn");
        group.begin_shutdown();

        let error = group
            .finish_shutdown(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect_err("panic should be reported");

        let TaskShutdownError::Join(failures) = error else {
            panic!("expected join failure");
        };
        assert_eq!(failures, ["task panicked: task panic"]);
        assert!(group.is_empty());
        group.start_generation().expect("group should reopen");
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shared_task_spawn_reports_finished_and_preserves_typed_result() {
        let slot = SharedTaskSlot::new();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();

        slot.spawn(async move {
            let _ = started_tx.send(());
            let _ = release_rx.await;
            42
        })
        .expect("spawn");
        started_rx.await.expect("task should start");

        assert!(!slot.is_finished());
        release_tx.send(()).expect("task should be waiting");

        time::timeout(TEST_TIMEOUT, async {
            while !slot.is_finished() {
                task::yield_now().await;
            }
        })
        .await
        .expect("task should finish");

        assert!(!slot.is_empty());
        assert!(slot.is_finished());
        let outcome = slot
            .finish(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task should be present");
        let TaskJoinOutcome::Completed(value) = outcome else {
            panic!("expected completed task");
        };
        assert_eq!(value, 42);
        assert!(slot.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shared_task_abort_interrupts_active_finish() {
        let slot = Arc::new(SharedTaskSlot::new());
        slot.insert(task::spawn(std::future::pending::<u32>()));
        let finishing_slot = Arc::clone(&slot);
        let finish =
            task::spawn(async move { finishing_slot.finish(TEST_TIMEOUT, TEST_TIMEOUT).await });

        while !slot.state.lock().draining {
            task::yield_now().await;
        }
        slot.abort();

        let outcome = time::timeout(TEST_TIMEOUT, finish)
            .await
            .expect("abort should wake the active finish")
            .expect("finisher should join")
            .expect("task should be present");
        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn concurrent_shared_finish_respects_its_own_bound() {
        let slot = Arc::new(SharedTaskSlot::new());
        slot.insert(task::spawn(std::future::pending::<u32>()));
        let finishing_slot = Arc::clone(&slot);
        let finish = task::spawn(async move {
            finishing_slot
                .finish(Duration::from_secs(10), TEST_TIMEOUT)
                .await
        });

        time::timeout(TEST_TIMEOUT, async {
            while slot.drain_lock.try_lock().is_ok() {
                task::yield_now().await;
            }
        })
        .await
        .expect("first finisher should hold the drain lock");

        let second = time::timeout(TEST_TIMEOUT, slot.finish(Duration::ZERO, Duration::ZERO)).await;
        slot.abort();
        let first = time::timeout(TEST_TIMEOUT, finish)
            .await
            .expect("abort should wake the first finisher")
            .expect("first finisher should join")
            .expect("task should be present");
        let second = second
            .expect("second finisher should respect its own bound")
            .expect("task should remain owned");

        assert!(matches!(first, TaskJoinOutcome::Aborted));
        assert!(matches!(second, TaskJoinOutcome::Incomplete));
        assert!(slot.is_empty());
    }

    #[rstest]
    #[case(Duration::MAX, Duration::ZERO)]
    #[case(Duration::ZERO, Duration::MAX)]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn unrepresentable_shared_deadline_retains_owned_task(
        #[case] graceful_timeout: Duration,
        #[case] abort_timeout: Duration,
    ) {
        let slot = SharedTaskSlot::new();
        slot.insert(task::spawn(std::future::pending::<()>()));

        let outcome = slot
            .finish(graceful_timeout, abort_timeout)
            .await
            .expect("task should remain present");

        assert!(matches!(outcome, TaskJoinOutcome::Incomplete));
        assert!(!slot.is_empty());

        slot.abort();
        let outcome = slot
            .finish(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task should remain present");
        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shared_task_abort_does_not_leak_into_next_generation() {
        let slot = SharedTaskSlot::new();
        slot.insert(task::spawn(std::future::pending::<u32>()));
        slot.abort();
        let first = slot
            .finish(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("first task");
        assert!(matches!(first, TaskJoinOutcome::Aborted));

        slot.insert(task::spawn(async { 42 }));
        let second = slot
            .finish(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("second task");
        let TaskJoinOutcome::Completed(value) = second else {
            panic!("expected completed second-generation task");
        };
        assert_eq!(value, 42);
        assert!(slot.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn canceled_shared_finish_restores_owned_task() {
        let slot = Arc::new(SharedTaskSlot::new());
        slot.insert(task::spawn(std::future::pending::<()>()));
        let finishing_slot = Arc::clone(&slot);
        let finish =
            task::spawn(async move { finishing_slot.finish(TEST_TIMEOUT, TEST_TIMEOUT).await });

        while !slot.state.lock().draining {
            task::yield_now().await;
        }
        finish.abort();
        let _ = finish.await;

        assert!(!slot.is_empty());
        let outcome = slot
            .finish(Duration::ZERO, TEST_TIMEOUT)
            .await
            .expect("restored task");
        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_empty());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_task_timeout_preserves_owned_typed_result() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let slot = SharedTaskSlot::new();
        slot.insert(tokio::task::spawn_blocking(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
            42
        }));
        started_rx.await.expect("blocking task should start");

        let outcome = slot
            .finish(Duration::ZERO, Duration::ZERO)
            .await
            .expect("task should be present");

        if !matches!(outcome, TaskJoinOutcome::Incomplete) {
            let _ = release_tx.send(());
            panic!("expected incomplete task, was {outcome:?}");
        }
        assert!(!slot.is_empty());

        release_tx
            .send(())
            .expect("blocking task should be waiting");
        let outcome = slot
            .finish(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task should be present");
        let TaskJoinOutcome::Completed(value) = outcome else {
            panic!("expected completed task after retry, was {outcome:?}");
        };

        assert_eq!(value, 42);
        assert!(slot.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shared_task_rejects_insertion_during_empty_drain_reservation() {
        let slot = SharedTaskSlot::new();
        let (reserved, _, _) = slot
            .state
            .lock()
            .try_reserve_drain()
            .expect("empty slot should reserve");

        let candidate = TaskSlot::from_handle(task::spawn(async {}));

        let rejected = slot
            .try_insert_slot(candidate)
            .expect_err("draining slot should reject insertion");

        assert!(rejected.is_some());
        assert!(slot.is_empty());
        let mut state = slot.state.lock();
        state.slot = reserved;
        state.draining = false;
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shared_task_try_insert_returns_task_when_occupied() {
        let slot = SharedTaskSlot::new();
        slot.insert(task::spawn(std::future::pending::<()>()));
        let candidate = TaskSlot::from_handle(task::spawn(async {}));

        let mut rejected = slot
            .try_insert_slot(candidate)
            .expect_err("occupied slot should reject insertion");
        let outcome = finish_task(&mut rejected, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("rejected task should remain present");

        assert!(matches!(outcome, TaskJoinOutcome::Completed(())));
        assert!(rejected.is_none());
        assert!(!slot.is_empty());

        slot.abort();
        let outcome = slot
            .finish(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("original task should remain present");
        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn shared_task_insert_aborts_rejected_task_before_panicking() {
        let slot = SharedTaskSlot::new();
        slot.insert(task::spawn(std::future::pending::<()>()));
        let (future, started_rx, dropped) = pending_with_drop_signal();

        let candidate = task::spawn(future);
        started_rx.await.expect("candidate task should start");

        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| slot.insert(candidate)))
            .expect_err("occupied slot should panic");
        wait_for_drop(&dropped).await;

        assert_eq!(
            panic_message(panic.as_ref()),
            "shared task slot is already occupied",
        );
        assert!(!slot.is_empty());
        slot.abort();
        let outcome = slot
            .finish(TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("original task should be present");
        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_empty());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn dropping_shared_task_slot_aborts_owned_task() {
        let slot = SharedTaskSlot::new();
        let (future, started_rx, dropped) = pending_with_drop_signal();

        slot.insert(task::spawn(future));
        started_rx.await.expect("task should start");

        drop(slot);
        wait_for_drop(&dropped).await;
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn singular_task_preserves_typed_result() {
        let mut slot = TaskSlot::new();
        slot.spawn(async { 42 }).expect("spawn");
        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task present");

        let TaskJoinOutcome::Completed(value) = outcome else {
            panic!("expected completed task");
        };
        assert_eq!(value, 42);
        assert!(slot.is_none());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn task_slot_insert_aborts_rejected_task_before_panicking() {
        let mut slot = TaskSlot::from_handle(task::spawn(std::future::pending::<()>()));
        let (future, started_rx, dropped) = pending_with_drop_signal();

        let candidate = task::spawn(future);
        started_rx.await.expect("candidate task should start");

        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| slot.insert(candidate)))
            .expect_err("occupied slot should panic");
        wait_for_drop(&dropped).await;

        assert_eq!(
            panic_message(panic.as_ref()),
            "task slot is already occupied"
        );
        assert!(slot.is_some());
        slot.abort();
        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("original task should be present");
        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_none());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn task_slot_spawn_rejects_occupied_slot_without_detaching_original() {
        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_task = Arc::clone(&dropped);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();

        let mut slot = TaskSlot::from_handle(task::spawn(async move {
            let _drop = DropSignal(dropped_task);
            let _ = started_tx.send(());
            let _ = release_rx.await;
            42
        }));
        started_rx.await.expect("original task should start");

        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
            slot.spawn(std::future::pending::<u32>())
        }));
        let _ = release_tx.send(());
        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task should remain present");
        wait_for_drop(&dropped).await;
        let panic = panic.expect_err("occupied slot should panic");

        let TaskJoinOutcome::Completed(value) = outcome else {
            panic!("expected original task to complete, was {outcome:?}");
        };
        assert_eq!(
            panic_message(panic.as_ref()),
            "task slot is already occupied"
        );
        assert_eq!(value, 42);
        assert!(dropped.load(Ordering::Acquire));
        assert!(slot.is_none());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn dropping_task_slot_aborts_owned_task() {
        let (future, started_rx, dropped) = pending_with_drop_signal();

        let slot = TaskSlot::from_handle(task::spawn(future));
        started_rx.await.expect("task should start");

        drop(slot);
        wait_for_drop(&dropped).await;
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn singular_task_is_joined_after_forced_abort() {
        let dropped = Arc::new(AtomicBool::new(false));
        let signal = DropSignal(Arc::clone(&dropped));
        let mut slot = TaskSlot::from_handle(task::spawn(async move {
            let _signal = signal;
            std::future::pending::<()>().await;
        }));

        let outcome = finish_task(&mut slot, Duration::ZERO, TEST_TIMEOUT)
            .await
            .expect("task present");

        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(dropped.load(Ordering::Acquire));
        assert!(slot.is_none());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn singular_task_joins_after_abort_with_paused_time() {
        let mut slot = TaskSlot::from_handle(task::spawn(std::future::pending::<()>()));

        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task present");

        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_none());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn singular_task_timeout_preserves_owned_typed_result() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut slot = TaskSlot::from_handle(tokio::task::spawn_blocking(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
            42
        }));
        started_rx.await.expect("blocking task should start");

        let outcome = finish_task(&mut slot, Duration::ZERO, Duration::ZERO)
            .await
            .expect("task present");

        if !matches!(outcome, TaskJoinOutcome::Incomplete) {
            let _ = release_tx.send(());
            panic!("expected incomplete task, was {outcome:?}");
        }
        assert!(slot.is_some());

        release_tx
            .send(())
            .expect("blocking task should be waiting");
        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task present");

        let value = match outcome {
            TaskJoinOutcome::Completed(value) => value,
            other => panic!("expected completed task after retry, was {other:?}"),
        };
        assert_eq!(value, 42);
        assert!(slot.is_none());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn singular_task_retry_preserves_forced_abort_classification() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();

        let mut slot = TaskSlot::from_handle(task::spawn(async move {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
            task::yield_now().await;
        }));
        started_rx.await.expect("blocking task should start");

        let outcome = finish_task(&mut slot, Duration::ZERO, Duration::ZERO)
            .await
            .expect("task present");
        assert!(matches!(outcome, TaskJoinOutcome::Incomplete));
        assert!(slot.is_some());

        release_tx
            .send(())
            .expect("blocking task should be waiting");
        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task present");

        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_none());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn singular_task_reports_unexpected_cancellation() {
        let handle = task::spawn(std::future::pending::<()>());
        handle.abort();
        let mut slot = TaskSlot::from_handle(handle);

        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task present");

        let TaskJoinOutcome::Failed(error) = outcome else {
            panic!("expected failed task");
        };
        assert!(error.is_cancelled());
        assert!(slot.is_none());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test]
    async fn singular_task_reports_panicked_join() {
        let mut slot = TaskSlot::from_handle(task::spawn(async {
            panic!("task panic");
        }));

        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task should be present");
        let TaskJoinOutcome::Failed(error) = outcome else {
            panic!("expected failed task");
        };

        assert!(error.is_panic());
        assert!(slot.is_none());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn canceled_singular_finish_preserves_owner_slot() {
        let mut slot = TaskSlot::from_handle(task::spawn(std::future::pending::<()>()));

        {
            let finish = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT);
            tokio::pin!(finish);
            tokio::select! {
                outcome = &mut finish => panic!("finish completed unexpectedly: {outcome:?}"),
                () = task::yield_now() => {}
            }
        }

        assert!(slot.is_some());
        let outcome = finish_task(&mut slot, Duration::ZERO, TEST_TIMEOUT)
            .await
            .expect("task present");
        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_none());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn canceled_singular_abort_wait_preserves_owner_slot() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut slot = TaskSlot::from_handle(tokio::task::spawn_blocking(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
            42
        }));
        started_rx.await.expect("blocking task should start");

        {
            let finish = finish_task(&mut slot, Duration::ZERO, TEST_TIMEOUT);
            tokio::pin!(finish);
            tokio::select! {
                outcome = &mut finish => panic!("finish completed unexpectedly: {outcome:?}"),
                () = task::yield_now() => {}
            }
        }

        assert!(slot.is_some());
        release_tx
            .send(())
            .expect("blocking task should be waiting");
        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task present");

        let TaskJoinOutcome::Completed(value) = outcome else {
            panic!("expected completed task after canceled abort wait, was {outcome:?}");
        };
        assert_eq!(value, 42);
        assert!(slot.is_none());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn canceled_singular_abort_wait_preserves_abort_classification() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();

        let mut slot = TaskSlot::from_handle(task::spawn(async move {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
            task::yield_now().await;
        }));
        started_rx.await.expect("blocking task should start");

        while !slot.abort_requested {
            {
                let finish = finish_task(&mut slot, Duration::ZERO, TEST_TIMEOUT);
                tokio::pin!(finish);
                tokio::select! {
                    biased;
                    outcome = &mut finish => panic!("finish completed unexpectedly: {outcome:?}"),
                    () = task::yield_now() => {}
                }
            }
        }

        assert!(slot.is_some());
        release_tx
            .send(())
            .expect("blocking task should be waiting");
        let outcome = finish_task(&mut slot, TEST_TIMEOUT, TEST_TIMEOUT)
            .await
            .expect("task present");

        assert!(matches!(outcome, TaskJoinOutcome::Aborted));
        assert!(slot.is_none());
    }

    fn pending_with_drop_signal() -> (
        impl Future<Output = ()>,
        tokio::sync::oneshot::Receiver<()>,
        Arc<AtomicBool>,
    ) {
        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_task = Arc::clone(&dropped);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let future = async move {
            let _drop = DropSignal(dropped_task);
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        };

        (future, started_rx, dropped)
    }

    async fn wait_for_drop(dropped: &AtomicBool) {
        time::timeout(TEST_TIMEOUT, async {
            while !dropped.load(Ordering::Acquire) {
                task::yield_now().await;
            }
        })
        .await
        .expect("task should be dropped");
    }

    struct DropSignal(Arc<AtomicBool>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    struct ReentrantDropFuture {
        group: Arc<TaskGroup>,
        dropped: Arc<AtomicBool>,
        polled: Arc<AtomicBool>,
    }

    struct ReentrantWake {
        group: Arc<TaskGroup>,
        woke: Arc<AtomicBool>,
    }

    impl Wake for ReentrantWake {
        fn wake(self: Arc<Self>) {
            let _ = self.group.is_open();
            self.woke.store(true, Ordering::Release);
        }
    }

    impl Future for ReentrantDropFuture {
        type Output = ();

        fn poll(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            self.polled.store(true, Ordering::Release);
            std::task::Poll::Pending
        }
    }

    impl Drop for ReentrantDropFuture {
        fn drop(&mut self) {
            let _ = self.group.is_open();
            self.dropped.store(true, Ordering::Release);
        }
    }
}
