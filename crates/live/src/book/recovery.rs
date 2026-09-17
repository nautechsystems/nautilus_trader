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

//! Recovery ownership and bounded replacement attempts for one book.
//!
//! - [`BookRecoveryState`] admits one recovery owner, rejects stale failure reports, and cancels
//!   obsolete work. Terminal failure suppresses new claims until the adapter resets the state.
//! - [`BookRecovery`] runs replacement attempts, waits for an accepted snapshot, and applies
//!   backoff, attempt limits, and a total elapsed-time budget.
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
//! # Adapters
//!
//! The adapter serializes claims, snapshot acceptance, and failure reporting under its state lock
//! or owning task. It retains the same episode across reconnects to preserve the remaining budget.
//! Removing or resetting its [`BookRecoveryState`] cancels the episode; dropping an attempt cancels
//! that attempt's child token.
//!
//! Task spawning, subscription correlation, and book cache updates remain adapter-owned.

use std::{future::Future, sync::Arc};

use nautilus_common::live::dst::time::{self, Duration};
use nautilus_network::retry::{RetryConfig, RetryManager};
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

/// Outcome published by an adapter's snapshot and rejection handling.
#[derive(Debug, Clone)]
pub enum BookRecoveryOutcome<E> {
    Pending,
    Accepted,
    Rejected(E),
}

/// One recovery episode, retained across reconnects until a snapshot or terminal failure.
#[derive(Debug)]
pub struct BookRecovery<E> {
    pub cancellation: CancellationToken,
    pub outcome: tokio::sync::watch::Sender<BookRecoveryOutcome<E>>,
    pub gate: SnapshotGate,
}

impl<E: Clone> Default for BookRecovery<E> {
    fn default() -> Self {
        Self {
            cancellation: CancellationToken::new(),
            outcome: tokio::sync::watch::channel(BookRecoveryOutcome::Pending).0,
            gate: SnapshotGate::default(),
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
    /// Replaces the subscription and waits for a snapshot, with bounded retries.
    ///
    /// Each send receives a child cancellation token. Dropping an attempt cancels queued
    /// transport work. Keeping this future alive across reconnects preserves the attempt limit
    /// and total elapsed-time budget.
    /// A zero snapshot timeout disables only the individual snapshot deadline.
    ///
    /// # Errors
    ///
    /// Returns the terminal adapter error, cancellation, or exhausted retry budget.
    pub async fn run<F, Fut>(
        &self,
        snapshot_timeout: Duration,
        replace: F,
        should_retry: impl Fn(&E) -> bool,
        create_error: impl Fn(String) -> E,
        timeout_error: impl Fn() -> E,
    ) -> Result<(), E>
    where
        F: Fn(CancellationToken, SnapshotGate) -> Fut,
        Fut: Future<Output = Result<(), E>>,
    {
        let manager = RetryManager::<E>::new(RetryConfig {
            max_retries: ATTEMPTS_MAX - 1,
            initial_delay_ms: RETRY_DELAY_INITIAL_MS,
            max_delay_ms: RETRY_DELAY_MAX_MS,
            backoff_factor: RETRY_BACKOFF_FACTOR,
            jitter_ms: RETRY_JITTER_MAX_MS,
            immediate_first: RETRY_FIRST_IMMEDIATE,
            operation_timeout_ms: OPERATION_TIMEOUT_MS,
            max_elapsed_ms: Some(ELAPSED_MAX_MS),
        });

        manager
            .invocation(
                "book recovery",
                || async {
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
                },
                should_retry,
                |e| create_error(e.to_string()),
            )
            .cancellation_token(&self.cancellation)
            .execute()
            .await
    }
}

/// Owns a book's recovery and terminal suppression under its adapter's state lock.
#[derive(Debug)]
pub struct BookRecoveryState<E> {
    recovery: Option<Arc<BookRecovery<E>>>,
    failed: bool,
}

impl<E> Default for BookRecoveryState<E> {
    fn default() -> Self {
        Self {
            recovery: None,
            failed: false,
        }
    }
}

impl<E: Clone> BookRecoveryState<E> {
    /// Claims one recovery episode, refusing duplicate or failed work.
    pub fn claim(&mut self) -> Option<Arc<BookRecovery<E>>> {
        if self.failed || self.recovery.as_ref().is_some_and(|r| !r.is_accepted()) {
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

    /// Returns whether exhausted or permanent failure suppresses book output.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        self.failed
    }

    /// Returns `true` when failure is recorded for the current, incomplete owner.
    ///
    /// A stale or accepted owner returns `false`. Passing `None` records failure unconditionally.
    pub fn fail(&mut self, owner: Option<&Arc<BookRecovery<E>>>) -> bool {
        if let Some(owner) = owner
            && (!self
                .recovery
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, owner))
                || owner.is_accepted())
        {
            return false;
        }

        self.reset();
        self.failed = true;
        true
    }

    /// Cancels obsolete work and clears terminal suppression for an explicit restart.
    pub fn reset(&mut self) {
        if let Some(recovery) = self.recovery.take() {
            recovery.cancellation.cancel();
        }

        self.failed = false;
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
    fn replacement_owner_cannot_fail_or_accept_after_reset() {
        let mut state = BookRecoveryState::<SendError>::default();
        let old = state.claim().unwrap();
        assert!(state.claim().is_none());
        state.reset();
        let current = state.claim().unwrap();

        assert!(old.cancellation.is_cancelled());
        assert!(!old.accept());
        assert!(!state.fail(Some(&old)));
        assert!(!state.is_failed());
        assert!(Arc::ptr_eq(state.current().unwrap(), &current));
        assert!(current.accept());
        assert!(!state.fail(Some(&current)));
    }

    #[rstest]
    fn gate_and_failure_suppress_snapshots_until_explicit_restart() {
        let mut state = BookRecoveryState::<SendError>::default();
        let recovery = state.claim().unwrap();
        assert!(recovery.begin_replacement());
        assert!(!recovery.accept());
        assert!(state.fail(Some(&recovery)));
        recovery.gate.open();

        assert!(!recovery.accept());
        assert!(state.is_failed());
        assert!(state.claim().is_none());
        state.reset();
        assert!(state.claim().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn missing_snapshots_exhaust_exactly_eight_attempts() {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);

        let result = recovery
            .run(
                Duration::from_secs(1),
                |_, gate| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    gate.open();
                    async { Ok(()) }
                },
                |_| true,
                SendError::BrokenPipe,
                || SendError::Timeout,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 8);
        assert!(!recovery.is_accepted());
    }

    #[tokio::test(start_paused = true)]
    async fn elapsed_budget_cancels_pending_send() {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);
        let child = parking_lot::Mutex::new(None);
        let started = time::Instant::now();

        let result = recovery
            .run(
                Duration::from_secs(1),
                |cancel, _| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    *child.lock() = Some(cancel);
                    std::future::pending::<Result<(), SendError>>()
                },
                |_| true,
                SendError::BrokenPipe,
                || SendError::Timeout,
            )
            .await;

        assert_eq!(
            result.unwrap_err().to_string(),
            "send failed: broken pipe (Retry budget exceeded (1/8))"
        );
        assert_eq!(started.elapsed(), Duration::from_secs(180));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(child.lock().as_ref().unwrap().is_cancelled());
    }

    #[tokio::test(start_paused = true)]
    async fn snapshot_during_send_completes_without_retry() {
        let recovery = BookRecovery::<SendError>::default();
        let attempts = AtomicUsize::new(0);

        let result = recovery
            .run(
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

        assert!(result.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(recovery.is_accepted());
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_owner_cancels_pending_attempt() {
        let mut state = BookRecoveryState::<SendError>::default();
        let recovery = state.claim().unwrap();
        let child = parking_lot::Mutex::new(None);

        let operation = recovery.run(
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
            result = &mut operation => panic!("unexpected completion: {result:?}"),
            () = tokio::task::yield_now() => {},
        }
        drop(state);
        assert!(operation.await.is_err());
        assert!(child.lock().as_ref().unwrap().is_cancelled());
    }
}
