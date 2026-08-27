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

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
};

use nautilus_model::identifiers::AccountId;

use super::metrics::{RunnerMetrics, RunnerMetricsSnapshot};

const STOP_REQUESTED: u8 = 1 << 7;
const STATE_MASK: u8 = !STOP_REQUESTED;

/// Lifecycle state of the `LiveNode` runner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.live",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.live")
)]
pub enum NodeState {
    #[default]
    Idle = 0,
    Starting = 1,
    Running = 2,
    ShuttingDown = 3,
    Stopped = 4,
}

impl NodeState {
    /// Creates a `NodeState` from its `u8` representation.
    ///
    /// # Panics
    ///
    /// Panics if the value is not a valid `NodeState` discriminant (0-4).
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Idle,
            1 => Self::Starting,
            2 => Self::Running,
            3 => Self::ShuttingDown,
            4 => Self::Stopped,
            _ => panic!("Invalid NodeState value"),
        }
    }

    /// Returns the `u8` representation of this state.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Returns whether the state is `Running`.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

/// Determines which lifecycle responsibilities the node owns while running.
///
/// Both modes run the same event loop. The mode only decides whether the node installs process
/// signal handlers, which a host application must own for itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeRunMode {
    /// The node owns the thread it runs on and installs its own signal handlers.
    #[default]
    Owned,
    /// A host event loop drives the node, and the host owns signal handling and shutdown.
    Hosted,
}

impl NodeRunMode {
    /// Returns whether the node installs process signal handlers in this mode.
    #[must_use]
    pub const fn owns_signals(self) -> bool {
        matches!(self, Self::Owned)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunningTransition {
    Entered,
    StopRequested,
    Invalid(u8),
}

#[derive(Debug)]
struct RegistrationState {
    registered: HashSet<AccountId>,
    lifecycle_generation: u64,
    stopped: bool,
}

#[derive(Debug)]
pub(super) struct RegistrationTracker {
    state: Mutex<RegistrationState>,
    changed: tokio::sync::watch::Sender<u64>,
}

impl RegistrationTracker {
    fn new() -> Self {
        let (changed, _) = tokio::sync::watch::channel(0);
        Self {
            state: Mutex::new(RegistrationState {
                registered: HashSet::new(),
                lifecycle_generation: 0,
                stopped: false,
            }),
            changed,
        }
    }

    pub(super) fn seed(&self, account_ids: impl IntoIterator<Item = AccountId>) {
        let mut state = self.state.lock().expect("registration tracker poisoned");
        let previous_len = state.registered.len();
        state.registered.extend(account_ids);
        if state.registered.len() != previous_len {
            self.notify();
        }
    }

    pub(super) fn mark_registered(&self, account_id: AccountId) {
        let mut state = self.state.lock().expect("registration tracker poisoned");
        if state.registered.insert(account_id) {
            self.notify();
        }
    }

    pub(super) fn set_starting(&self) {
        let mut state = self.state.lock().expect("registration tracker poisoned");
        state.lifecycle_generation = state
            .lifecycle_generation
            .checked_add(1)
            .expect("registration lifecycle generation overflowed");
        state.stopped = false;
        self.notify();
    }

    pub(super) fn set_stopped(&self) {
        let mut state = self.state.lock().expect("registration tracker poisoned");
        state.stopped = true;
        self.notify();
    }

    // Disposal invalidates the cache's account rows, so the registered
    // predicate is cleared with the stop rather than retained across it.
    pub(super) fn set_disposed(&self) {
        let mut state = self.state.lock().expect("registration tracker poisoned");
        state.registered.clear();
        state.stopped = true;
        self.notify();
    }

    fn notify(&self) {
        self.changed.send_modify(|generation| {
            *generation = generation
                .checked_add(1)
                .expect("registration change generation overflowed");
        });
    }
}

/// A thread-safe handle to control a `LiveNode` from other threads.
///
/// This allows stopping and querying the node's state without requiring the
/// node itself to be Send + Sync.
#[derive(Clone, Debug)]
pub struct LiveNodeHandle {
    control: Arc<AtomicU8>,
    pub(crate) metrics: Arc<RunnerMetrics>,
    registration: Option<Arc<RegistrationTracker>>,
}

impl Default for LiveNodeHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveNodeHandle {
    /// Creates a new handle with default (`Idle`) state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            control: Arc::new(AtomicU8::new(NodeState::Idle.as_u8())),
            metrics: Arc::new(RunnerMetrics::default()),
            registration: None,
        }
    }

    pub(crate) fn attached() -> Self {
        Self {
            registration: Some(Arc::new(RegistrationTracker::new())),
            ..Self::new()
        }
    }

    pub(super) fn registration_tracker(&self) -> Arc<RegistrationTracker> {
        Arc::clone(
            self.registration
                .as_ref()
                .expect("live node handle must be attached"),
        )
    }

    pub(crate) fn set_starting(&self) {
        if let Some(registration) = &self.registration {
            registration.set_starting();
        }
        self.set_state(NodeState::Starting);
    }

    pub(crate) fn set_shutting_down(&self) {
        self.set_state(NodeState::ShuttingDown);
    }

    pub(crate) fn set_stopped(&self) {
        self.set_state(NodeState::Stopped);
    }

    pub(crate) fn close_registration_tracker(&self) {
        if let Some(registration) = &self.registration {
            registration.set_stopped();
        }
    }

    pub(crate) fn dispose_registration_tracker(&self) {
        if let Some(registration) = &self.registration {
            registration.set_disposed();
        }
    }

    pub(super) fn try_set_running(&self) -> RunningTransition {
        match self.control.compare_exchange(
            NodeState::Starting.as_u8(),
            NodeState::Running.as_u8(),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => RunningTransition::Entered,
            Err(control) if control == (NodeState::Starting.as_u8() | STOP_REQUESTED) => {
                RunningTransition::StopRequested
            }
            Err(control) => RunningTransition::Invalid(control),
        }
    }

    fn set_state(&self, state: NodeState) {
        let _ = self
            .control
            .try_update(Ordering::AcqRel, Ordering::Acquire, |control| {
                Some((control & STOP_REQUESTED) | state.as_u8())
            });
    }

    /// Returns the current node state.
    #[must_use]
    pub fn state(&self) -> NodeState {
        NodeState::from_u8(self.control.load(Ordering::Acquire) & STATE_MASK)
    }

    /// Returns whether the node should stop.
    #[must_use]
    pub fn should_stop(&self) -> bool {
        self.control.load(Ordering::Acquire) & STOP_REQUESTED != 0
    }

    /// Returns whether the node is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.state().is_running()
    }

    /// Returns a by-value snapshot of `LiveNode::run` dispatch metrics after startup.
    #[must_use]
    pub fn metrics_snapshot(&self) -> RunnerMetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Waits until the account is registered in the node cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle is unattached or the node stops before registration.
    pub async fn await_account_registered(&self, account_id: AccountId) -> anyhow::Result<()> {
        let Some(registration) = &self.registration else {
            anyhow::bail!("Cannot await account registration with an unattached node handle");
        };
        let mut changed = registration.changed.subscribe();
        let lifecycle_generation = {
            let state = registration
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("Account registration tracker is unavailable"))?;

            if state.registered.contains(&account_id) {
                return Ok(());
            }

            if matches!(self.state(), NodeState::Idle | NodeState::Stopped) {
                anyhow::bail!("Cannot await account registration while node is not running");
            }

            if state.stopped {
                anyhow::bail!(
                    "Node stopped before account {account_id} was registered in lifecycle {}",
                    state.lifecycle_generation,
                );
            }

            state.lifecycle_generation
        };

        loop {
            changed.changed().await.map_err(|_| {
                anyhow::anyhow!(
                    "Account registration tracker closed before {account_id} registered"
                )
            })?;

            {
                let state = registration
                    .state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Account registration tracker is unavailable"))?;

                if state.registered.contains(&account_id) {
                    return Ok(());
                }

                if state.stopped || state.lifecycle_generation != lifecycle_generation {
                    anyhow::bail!(
                        "Node stopped before account {account_id} was registered in lifecycle {lifecycle_generation}",
                    );
                }
            }
        }
    }

    /// Signals the node to stop.
    pub fn stop(&self) {
        self.control.fetch_or(STOP_REQUESTED, Ordering::AcqRel);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EngineConnectionStatus {
    Connected,
    TimedOut,
    StopRequested,
    ShutdownRequested,
}

impl EngineConnectionStatus {
    pub(super) const fn abort_reason(self) -> Option<&'static str> {
        match self {
            Self::Connected | Self::TimedOut => None,
            Self::StopRequested => Some("Stop signal received during startup"),
            Self::ShutdownRequested => Some("Shutdown signal received during startup"),
        }
    }
}
