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

//! Recovery ownership and replacement attempts for one book.
//!
//! - [`BookRecoveryState`] admits one running recovery per book and cancels obsolete work.
//! - [`BookRecovery`] runs replacement attempts until the adapter accepts a snapshot or cancels the
//!   episode. A book never ends in a terminal failure state.
//! - [`BookRecoveryOutcome`] carries pending, accepted, or rejected results from the adapter's
//!   frame handling to the recovery runner.
//!
//! # Recovery Lifecycle
//!
//! The adapter claims an episode before starting work and supplies the replacement operation,
//! retry classification, and error construction. Each attempt receives a child cancellation token
//! and a closed snapshot gate. The adapter opens the gate after the intended connection confirms
//! the write, then accepts a valid snapshot through [`BookRecovery::accept`]. Write completion
//! alone never completes recovery.
//!
//! Attempts back off within a retry budget of eight attempts in 180 seconds. An exhausted budget,
//! or an error the adapter classifies as not retryable, moves the episode to attempts at a jittered
//! interval that doubles from one minute to fifteen minutes, which bounds subscription traffic for
//! a book that keeps failing. Each such attempt is bounded, so a stalled write or a disabled
//! snapshot deadline cannot end the retries. The runner logs the transition as an error, each later
//! failed attempt as a warning, and completion as info.
//!
//! # Adapters
//!
//! The adapter serializes claims and snapshot acceptance under its state lock or owning task. A
//! reconnect keeps a running episode, whose replacement may be in flight, so the reconnect can
//! neither replenish its budget nor abandon a write halfway. Removing or resetting a
//! [`BookRecoveryState`] cancels its episode; dropping an attempt cancels that attempt's child
//! token.
//!
//! Task spawning, subscription correlation, and book cache updates remain adapter-owned.

use std::{fmt::Display, future::Future, sync::Arc};

use nautilus_common::live::dst::time::{self, Duration};
use nautilus_network::{
    backoff::ExponentialBackoff,
    retry::{RetryConfig, RetryManager},
};
use tokio_util::sync::CancellationToken;

use super::snapshot::SnapshotGate;

// Includes the initial attempt
const ATTEMPTS_MAX: u32 = 8;
const ELAPSED_MAX_MS: u64 = 180_000;
const OPERATION_TIMEOUT_MS: Option<u64> = None;

const RETRY_DELAY_INITIAL_MS: u64 = 1_000;
const RETRY_DELAY_MAX_MS: u64 = 10_000;
const RETRY_BACKOFF_FACTOR: f64 = 2.0;
const RETRY_JITTER_MAX_MS: u64 = 1_000;
const RETRY_FIRST_IMMEDIATE: bool = true;

// Attempts after the retry budget, until acceptance or cancellation
const RETRY_DELAY_CEILING_INITIAL_MS: u64 = 60_000;
const RETRY_DELAY_CEILING_MAX_MS: u64 = 900_000;
const RETRY_JITTER_CEILING_MAX_MS: u64 = 5_000;

/// Outcome published by an adapter's snapshot and rejection handling.
#[derive(Debug, Clone)]
pub enum BookRecoveryOutcome<E> {
    Pending,
    Accepted,
    Rejected(E),
}

/// One recovery episode, running until a snapshot is accepted or the episode is cancelled.
#[derive(Debug)]
pub struct BookRecovery<E> {
    pub cancellation: CancellationToken,
    pub outcome: tokio::sync::watch::Sender<BookRecoveryOutcome<E>>,
    pub gate: SnapshotGate,
    reconnected: tokio::sync::Notify,
}

impl<E: Clone> Default for BookRecovery<E> {
    fn default() -> Self {
        Self {
            cancellation: CancellationToken::new(),
            outcome: tokio::sync::watch::channel(BookRecoveryOutcome::Pending).0,
            gate: SnapshotGate::default(),
            reconnected: tokio::sync::Notify::new(),
        }
    }
}

impl<E: Clone> BookRecovery<E> {
    /// Closes snapshot acceptance, returning `false` if this episode has already ended.
    #[must_use]
    pub fn begin_replacement(&self) -> bool {
        let mut gate = self.gate.lock();

        if self.cancellation.is_cancelled() || self.is_accepted() {
            return false;
        }

        gate.close();
        true
    }

    /// Returns whether the adapter has accepted a snapshot for this episode.
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        matches!(*self.outcome.borrow(), BookRecoveryOutcome::Accepted)
    }

    /// Returns whether this episode still owns its book: not accepted and not cancelled.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.is_accepted() && !self.cancellation.is_cancelled()
    }

    /// Returns `true` when the gate permits accepting a snapshot for this episode.
    pub fn accept(&self) -> bool {
        let gate = self.gate.lock();
        if gate.is_closed() || (self.cancellation.is_cancelled() && !self.is_accepted()) {
            return false;
        }

        self.outcome.send_replace(BookRecoveryOutcome::Accepted);
        true
    }
}

impl<E: Clone + std::error::Error> BookRecovery<E> {
    /// Replaces the subscription and waits for a snapshot until one is accepted or the episode is
    /// cancelled.
    ///
    /// Each send receives a child cancellation token. Dropping an attempt cancels queued
    /// transport work. Keeping this future alive across reconnects preserves the retry budget.
    /// A zero snapshot timeout disables only the individual snapshot deadline. Errors that
    /// `should_retry` rejects skip the rest of the budget.
    #[allow(
        clippy::missing_panics_doc,
        reason = "the ceiling backoff configuration is static and valid"
    )]
    pub async fn run<F, Fut>(
        &self,
        book: impl Display,
        snapshot_timeout: Duration,
        replace: F,
        should_retry: impl Fn(&E) -> bool,
        create_error: impl Fn(String) -> E,
        timeout_error: impl Fn() -> E,
    ) where
        F: Fn(CancellationToken, SnapshotGate) -> Fut,
        Fut: Future<Output = Result<(), E>>,
    {
        let attempt = || self.attempt(snapshot_timeout, &replace, &create_error, &timeout_error);

        let budget = RetryManager::<E>::new(RetryConfig {
            max_retries: ATTEMPTS_MAX - 1,
            initial_delay_ms: RETRY_DELAY_INITIAL_MS,
            max_delay_ms: RETRY_DELAY_MAX_MS,
            backoff_factor: RETRY_BACKOFF_FACTOR,
            jitter_ms: RETRY_JITTER_MAX_MS,
            immediate_first: RETRY_FIRST_IMMEDIATE,
            operation_timeout_ms: OPERATION_TIMEOUT_MS,
            max_elapsed_ms: Some(ELAPSED_MAX_MS),
        });

        let result = budget
            .invocation("book recovery", attempt, should_retry, |e| {
                create_error(e.to_string())
            })
            .cancellation_token(&self.cancellation)
            .execute()
            .await;

        if let Err(e) = result {
            if self.cancellation.is_cancelled() {
                return;
            }

            log::error!(
                "Book recovery for {book} did not complete within its retry budget; retrying at \
                 intervals growing from {}s to {}s until a snapshot is accepted: {e}",
                RETRY_DELAY_CEILING_INITIAL_MS / 1_000,
                RETRY_DELAY_CEILING_MAX_MS / 1_000,
            );

            // Bounds a stalled write and a disabled snapshot deadline alike
            let attempt_max =
                snapshot_timeout.max(Duration::from_millis(RETRY_DELAY_CEILING_INITIAL_MS));

            let attempts = async {
                let mut backoff = ExponentialBackoff::new(
                    Duration::from_millis(RETRY_DELAY_CEILING_INITIAL_MS),
                    Duration::from_millis(RETRY_DELAY_CEILING_MAX_MS),
                    RETRY_BACKOFF_FACTOR,
                    RETRY_JITTER_CEILING_MAX_MS,
                    false,
                )
                .expect("static ceiling backoff configuration is valid");

                loop {
                    // A reconnect ends the wait so the next attempt uses the new connection
                    tokio::select! {
                        biased;
                        () = self.reconnected.notified() => {}
                        () = time::sleep(backoff.next_duration()) => {}
                    }

                    let result = time::timeout(attempt_max, attempt())
                        .await
                        .unwrap_or_else(|_| Err(timeout_error()));

                    match result {
                        Ok(()) => return,
                        Err(e) => log::warn!("Book recovery attempt for {book} failed: {e}"),
                    }
                }
            };

            let mut outcome = self.outcome.subscribe();

            // A snapshot accepted between attempts ends the wait; no replacement write can be in
            // flight then, since acceptance requires an open gate.
            tokio::select! {
                biased;
                () = self.cancellation.cancelled() => return,
                _ = outcome.wait_for(|o| matches!(o, BookRecoveryOutcome::Accepted)) => {}
                () = attempts => {}
            }
        }

        if self.is_accepted() {
            log::info!("Book recovery completed for {book}");
        }
    }

    async fn attempt<F, Fut>(
        &self,
        snapshot_timeout: Duration,
        replace: &F,
        create_error: &impl Fn(String) -> E,
        timeout_error: &impl Fn() -> E,
    ) -> Result<(), E>
    where
        F: Fn(CancellationToken, SnapshotGate) -> Fut,
        Fut: Future<Output = Result<(), E>>,
    {
        let mut outcome = self.outcome.subscribe();

        if !self.begin_replacement() {
            return Ok(());
        }

        self.outcome.send_if_modified(|outcome| {
            if matches!(outcome, BookRecoveryOutcome::Rejected(_)) {
                *outcome = BookRecoveryOutcome::Pending;
                true
            } else {
                false
            }
        });

        let cancel = self.cancellation.child_token();
        let _guard = cancel.clone().drop_guard();
        replace(cancel, self.gate.clone()).await?;

        let wait = async {
            loop {
                match outcome.borrow_and_update().clone() {
                    BookRecoveryOutcome::Accepted => return Ok(()),
                    BookRecoveryOutcome::Rejected(e) => return Err(e),
                    BookRecoveryOutcome::Pending => {}
                }

                outcome
                    .changed()
                    .await
                    .map_err(|e| create_error(e.to_string()))?;
            }
        };

        if snapshot_timeout.is_zero() {
            wait.await
        } else {
            time::timeout(snapshot_timeout, wait)
                .await
                .unwrap_or_else(|_| Err(timeout_error()))
        }
    }
}

/// Owns a book's recovery episode under its adapter's state lock.
#[derive(Debug)]
pub struct BookRecoveryState<E> {
    recovery: Option<Arc<BookRecovery<E>>>,
}

impl<E> Default for BookRecoveryState<E> {
    fn default() -> Self {
        Self { recovery: None }
    }
}

impl<E: Clone> BookRecoveryState<E> {
    /// Claims one recovery episode, refusing while another is running.
    pub fn claim(&mut self) -> Option<Arc<BookRecovery<E>>> {
        if self.is_running() {
            return None;
        }

        self.reset();
        let recovery = Arc::new(BookRecovery::default());
        self.recovery = Some(Arc::clone(&recovery));
        Some(recovery)
    }

    /// Returns the current episode.
    #[must_use]
    pub fn current(&self) -> Option<&Arc<BookRecovery<E>>> {
        self.recovery.as_ref()
    }

    /// Returns whether a running episode owns the book.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.recovery.as_ref().is_some_and(|r| r.is_running())
    }

    /// Keeps a running episode, whose replacement may be in flight, and restarts any other.
    ///
    /// A kept episode waiting between retries after its budget attempts again at once; a reconnect
    /// during an attempt ends the next wait instead.
    pub fn reset_on_reconnect(&mut self) {
        if let Some(recovery) = self
            .recovery
            .as_ref()
            .filter(|recovery| recovery.is_running())
        {
            recovery.reconnected.notify_one();
        } else {
            self.reset();
        }
    }

    /// Cancels the current episode.
    pub fn reset(&mut self) {
        if let Some(recovery) = self.recovery.take() {
            recovery.cancellation.cancel();
        }
    }
}

impl<E> Drop for BookRecoveryState<E> {
    fn drop(&mut self) {
        if let Some(recovery) = &self.recovery {
            recovery.cancellation.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use nautilus_network::error::SendError;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn replacement_owner_cannot_accept_after_reset() {
        let mut state = BookRecoveryState::<SendError>::default();
        let old = state.claim().unwrap();
        let duplicate = state.claim();
        state.reset();
        let current = state.claim().unwrap();

        assert!(duplicate.is_none());
        assert!(old.cancellation.is_cancelled());
        assert!(!old.accept());
        assert!(Arc::ptr_eq(state.current().unwrap(), &current));
        assert!(current.accept());
        assert!(!state.is_running());
    }

    #[rstest]
    fn closed_gate_suppresses_snapshot_until_opened() {
        let mut state = BookRecoveryState::<SendError>::default();
        let recovery = state.claim().unwrap();
        assert!(recovery.begin_replacement());

        let while_closed = recovery.accept();
        recovery.gate.open();
        let after_open = recovery.accept();

        assert!(!while_closed);
        assert!(after_open);
        assert!(state.claim().is_some());
    }

    #[rstest]
    fn claim_replaces_cancelled_episode() {
        let mut state = BookRecoveryState::<SendError>::default();
        let first = state.claim().unwrap();
        first.cancellation.cancel();

        let second = state.claim().unwrap();

        assert!(!Arc::ptr_eq(&first, &second));
        assert!(state.is_running());
    }

    #[rstest]
    fn reconnect_keeps_running_episode() {
        let mut state = BookRecoveryState::<SendError>::default();
        let recovery = state.claim().unwrap();
        assert!(recovery.begin_replacement());

        state.reset_on_reconnect();

        assert!(Arc::ptr_eq(state.current().unwrap(), &recovery));
        assert!(!recovery.cancellation.is_cancelled());
    }

    #[rstest]
    fn reconnect_resets_cancelled_episode() {
        let mut state = BookRecoveryState::<SendError>::default();
        let recovery = state.claim().unwrap();
        recovery.cancellation.cancel();

        state.reset_on_reconnect();

        assert!(state.current().is_none());
    }

    #[rstest]
    fn reconnect_resets_accepted_episode() {
        let mut state = BookRecoveryState::<SendError>::default();
        let recovery = state.claim().unwrap();
        assert!(recovery.accept());

        state.reset_on_reconnect();

        assert!(state.current().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn missing_snapshots_spend_budget_then_retry_at_ceiling() {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);
        let started = time::Instant::now();

        recovery
            .run(
                "BOOK",
                Duration::from_secs(1),
                |_, gate| {
                    let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    gate.open();

                    // The tenth attempt is the second at the ceiling interval
                    if attempt == 10 {
                        assert!(recovery.accept());
                    }

                    async { Ok(()) }
                },
                |_| true,
                SendError::BrokenPipe,
                || SendError::Timeout,
            )
            .await;

        assert_eq!(attempts.load(Ordering::SeqCst), 10);
        assert!(recovery.is_accepted());
        assert!(started.elapsed() >= Duration::from_secs(180));
    }

    #[tokio::test(start_paused = true)]
    async fn elapsed_budget_cancels_pending_send_and_continues() {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);
        let child = parking_lot::Mutex::new(None);

        let result = time::timeout(
            Duration::from_secs(200),
            recovery.run(
                "BOOK",
                Duration::from_secs(1),
                |cancel, _| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    *child.lock() = Some(cancel);
                    std::future::pending::<Result<(), SendError>>()
                },
                |_| true,
                SendError::BrokenPipe,
                || SendError::Timeout,
            ),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(recovery.is_running());
        assert!(child.lock().as_ref().unwrap().is_cancelled());
    }

    #[tokio::test(start_paused = true)]
    async fn non_retryable_error_moves_to_ceiling() {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);

        let result = time::timeout(
            Duration::from_secs(59),
            recovery.run(
                "BOOK",
                Duration::from_secs(1),
                |_, _| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    async { Err(SendError::Closed) }
                },
                |_| false,
                SendError::BrokenPipe,
                || SendError::Timeout,
            ),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(recovery.is_running());
    }

    // Neither a disabled snapshot deadline nor a write that never completes may end the retries
    #[rstest]
    #[case::disabled_snapshot_deadline(Duration::ZERO, false)]
    #[case::stalled_write(Duration::from_secs(10), true)]
    #[tokio::test(start_paused = true)]
    async fn ceiling_attempts_are_bounded_and_continue(
        #[case] snapshot_timeout: Duration,
        #[case] stall_write: bool,
    ) {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);

        let result = time::timeout(
            Duration::from_secs(700),
            recovery.run(
                "BOOK",
                snapshot_timeout,
                |_, gate| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    let stall = stall_write;
                    async move {
                        if stall {
                            std::future::pending::<()>().await;
                        }

                        gate.open();
                        Ok(())
                    }
                },
                |_| true,
                SendError::BrokenPipe,
                || SendError::Timeout,
            ),
        )
        .await;

        // The budget ends at 180s; ceiling attempts start near 240s and 420s, each bounded
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert!(recovery.is_running());
    }

    #[tokio::test(start_paused = true)]
    async fn snapshot_between_ceiling_attempts_completes_run() {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);
        let started = time::Instant::now();

        let run = recovery.run(
            "BOOK",
            Duration::from_secs(1),
            |_, gate| {
                attempts.fetch_add(1, Ordering::SeqCst);
                gate.open();
                async { Err(SendError::Closed) }
            },
            |_| false,
            SendError::BrokenPipe,
            || SendError::Timeout,
        );

        let late_snapshot = async {
            time::sleep(Duration::from_secs(10)).await;
            assert!(recovery.accept());
        };

        tokio::join!(run, late_snapshot);

        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(recovery.is_accepted());
        assert_eq!(started.elapsed(), Duration::from_secs(10));
    }

    // A reconnect ends the wait at the ceiling so the kept episode retries on the new connection
    #[tokio::test(start_paused = true)]
    async fn reconnect_wakes_episode_waiting_at_ceiling() {
        let mut state = BookRecoveryState::<SendError>::default();
        let recovery = state.claim().unwrap();
        let attempts = AtomicUsize::new(0);

        let run = recovery.run(
            "BOOK",
            Duration::from_secs(1),
            |_, _| {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Err(SendError::Closed) }
            },
            |_| false,
            SendError::BrokenPipe,
            || SendError::Timeout,
        );

        let reconnect = async {
            time::sleep(Duration::from_secs(10)).await;
            let before = attempts.load(Ordering::SeqCst);
            state.reset_on_reconnect();
            time::sleep(Duration::from_secs(1)).await;
            let after = attempts.load(Ordering::SeqCst);
            recovery.cancellation.cancel();
            (before, after)
        };

        let ((), (before, after)) = tokio::join!(run, reconnect);

        assert_eq!(before, 1);
        assert_eq!(after, 2);
        assert!(Arc::ptr_eq(state.current().unwrap(), &recovery));
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_at_ceiling_ends_run() {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);

        let run = recovery.run(
            "BOOK",
            Duration::from_secs(1),
            |_, _| {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Err(SendError::Closed) }
            },
            |_| false,
            SendError::BrokenPipe,
            || SendError::Timeout,
        );

        let cancel = async {
            time::sleep(Duration::from_secs(30)).await;
            recovery.cancellation.cancel();
        };

        tokio::join!(run, cancel);

        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(!recovery.is_running());
        assert!(!recovery.is_accepted());
    }

    #[tokio::test(start_paused = true)]
    async fn snapshot_during_send_completes_without_retry() {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);

        recovery
            .run(
                "BOOK",
                Duration::from_secs(1),
                |_, gate| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    assert!(!recovery.accept());
                    gate.open();
                    assert!(recovery.accept());
                    async { Ok(()) }
                },
                |_| true,
                SendError::BrokenPipe,
                || SendError::Timeout,
            )
            .await;

        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(recovery.is_accepted());
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_owner_cancels_pending_attempt() {
        let mut state = BookRecoveryState::<SendError>::default();
        let recovery = state.claim().unwrap();
        let child = parking_lot::Mutex::new(None);

        let operation = recovery.run(
            "BOOK",
            Duration::ZERO,
            |cancel, _| {
                *child.lock() = Some(cancel);
                std::future::pending::<Result<(), SendError>>()
            },
            |_| true,
            SendError::BrokenPipe,
            || SendError::Timeout,
        );

        tokio::pin!(operation);
        tokio::select! {
            () = &mut operation => panic!("unexpected completion"),
            () = tokio::task::yield_now() => {},
        }
        drop(state);
        operation.await;

        assert!(child.lock().as_ref().unwrap().is_cancelled());
    }
}
