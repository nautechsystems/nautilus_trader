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

//! Authentication state tracking for WebSocket clients.
//!
//! This module provides a robust authentication tracker that coordinates login attempts
//! and ensures each attempt produces a fresh success or failure signal before operations
//! resume. It follows a proven pattern used in production.
//!
//! # Key Features
//!
//! - **Three-state model**: `Unauthenticated`, `Authenticated`, `Failed` via `AuthState` enum.
//! - **Oneshot signaling**: Each auth attempt gets a dedicated channel for result notification.
//! - **Superseding logic**: New authentication requests cancel pending ones.
//! - **Timeout handling**: Configurable timeout for authentication responses.
//! - **Generic error mapping**: Adapters can map to their specific error types.
//! - **Auth-gated waiting**: `wait_for_authenticated()` blocks until auth completes or fails.
//!
//! # Recommended Integration Pattern
//!
//! Based on production usage, the recommended pattern is:
//!
//! 1. **Order operations**: Call `wait_for_authenticated()` before private operations.
//!    This waits for re-auth after reconnection instead of rejecting immediately.
//! 2. **Reconnection flow**: Authenticate BEFORE resubscribing to topics.
//! 3. **Event propagation**: Send auth failures through event channels to consumers.
//! 4. **State lifecycle**: Call `invalidate()` on disconnect, `succeed()`/`fail()` handle auth results.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

pub type AuthResultSender = tokio::sync::oneshot::Sender<Result<(), String>>;
pub type AuthResultReceiver = tokio::sync::oneshot::Receiver<Result<(), String>>;

/// Authentication state for a WebSocket session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum AuthState {
    /// Not authenticated (initial state, after invalidate/begin).
    #[default]
    Unauthenticated = 0,
    /// Successfully authenticated (after succeed).
    Authenticated = 1,
    /// Authentication explicitly rejected by the server (after fail).
    Failed = 2,
}

impl AuthState {
    #[inline]
    #[must_use]
    #[expect(
        clippy::match_same_arms,
        reason = "explicit variant listing is clearer than collapsing 0 with wildcard"
    )]
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Unauthenticated,
            1 => Self::Authenticated,
            2 => Self::Failed,
            _ => Self::Unauthenticated,
        }
    }

    #[inline]
    #[must_use]
    const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Generic authentication state tracker for WebSocket connections.
///
/// Coordinates authentication attempts by providing a channel-based signaling
/// mechanism. Each authentication attempt receives a dedicated oneshot channel
/// that will be resolved when the server responds.
///
/// # State Management
///
/// The tracker maintains a three-state machine:
/// - `Unauthenticated`: after `begin()`, `invalidate()`, or initial construction.
/// - `Authenticated`: after `succeed()`. Queryable via `is_authenticated()`.
/// - `Failed`: after `fail()`. Causes `wait_for_authenticated()` to return early.
///
/// # Superseding Behavior
///
/// If a new authentication attempt begins while a previous one is pending,
/// the old attempt is automatically cancelled with an error. This prevents
/// auth response race conditions during rapid reconnections.
///
/// # Thread Safety
///
/// All operations are thread-safe and can be called concurrently from multiple tasks.
#[derive(Clone, Debug)]
pub struct AuthTracker {
    tx: Arc<Mutex<Option<AuthResultSender>>>,
    state: Arc<AtomicU8>,
    state_notify: Arc<tokio::sync::Notify>,
}

impl AuthTracker {
    /// Creates a new authentication tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tx: Arc::new(Mutex::new(None)),
            state: Arc::new(AtomicU8::new(AuthState::Unauthenticated.as_u8())),
            state_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Returns the current authentication state.
    #[must_use]
    pub fn auth_state(&self) -> AuthState {
        AuthState::from_u8(self.state.load(Ordering::Acquire))
    }

    /// Returns whether the client is currently authenticated.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.auth_state() == AuthState::Authenticated
    }

    /// Clears the authentication state without affecting pending auth attempts.
    ///
    /// Call this on disconnect or when the connection is closed to ensure
    /// operations requiring authentication are properly guarded.
    pub fn invalidate(&self) {
        self.state
            .store(AuthState::Unauthenticated.as_u8(), Ordering::Release);
        self.state_notify.notify_waiters();
    }

    /// Begins a new authentication attempt.
    ///
    /// Returns a receiver that will be notified when authentication completes.
    /// If a previous authentication attempt is still pending, it will be cancelled
    /// with an error message indicating it was superseded.
    ///
    /// Transitions to `Unauthenticated` since a new attempt invalidates any
    /// previous status.
    #[allow(
        clippy::must_use_candidate,
        reason = "callers use this for side effects"
    )]
    pub fn begin(&self) -> AuthResultReceiver {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.state
            .store(AuthState::Unauthenticated.as_u8(), Ordering::Release);

        if let Ok(mut guard) = self.tx.lock() {
            if let Some(old) = guard.take() {
                log::warn!("New authentication request superseding previous pending request");
                let _ = old.send(Err("Authentication attempt superseded".to_string()));
            } else {
                log::debug!("Starting new authentication request");
            }
            *guard = Some(sender);
        }

        receiver
    }

    /// Marks the current authentication attempt as successful.
    ///
    /// Transitions to `Authenticated` and notifies any waiting receiver
    /// with `Ok(())`. This should be called when the server sends a successful
    /// authentication response.
    ///
    /// The state is always updated even if no receiver is waiting (e.g., after
    /// a timeout), since the server has confirmed authentication.
    pub fn succeed(&self) {
        self.state
            .store(AuthState::Authenticated.as_u8(), Ordering::Release);
        self.state_notify.notify_waiters();

        if let Ok(mut guard) = self.tx.lock()
            && let Some(sender) = guard.take()
        {
            let _ = sender.send(Ok(()));
        }
    }

    /// Marks the current authentication attempt as failed.
    ///
    /// Transitions to `Failed` and notifies any waiting receiver
    /// with `Err(message)`. This should be called when the server sends an
    /// authentication error response.
    ///
    /// The state is always updated even if no receiver is waiting, since the
    /// server has rejected authentication.
    pub fn fail(&self, error: impl Into<String>) {
        self.state
            .store(AuthState::Failed.as_u8(), Ordering::Release);
        self.state_notify.notify_waiters();
        let message = error.into();

        if let Ok(mut guard) = self.tx.lock()
            && let Some(sender) = guard.take()
        {
            let _ = sender.send(Err(message));
        }
    }

    /// Waits for the authentication result with a timeout.
    ///
    /// Returns `Ok(())` if authentication succeeds, or an error if it fails,
    /// times out, or the channel is closed.
    ///
    /// # Type Parameters
    ///
    /// - `E`: Error type that implements `From<String>` for error message conversion
    ///
    /// # Errors
    ///
    /// Returns an error in the following cases:
    /// - Authentication fails (server rejects credentials)
    /// - Authentication times out (no response within timeout duration)
    /// - Authentication channel closes unexpectedly
    /// - Authentication attempt is superseded by a new attempt
    pub async fn wait_for_result<E>(
        &self,
        timeout: Duration,
        receiver: AuthResultReceiver,
    ) -> Result<(), E>
    where
        E: From<String>,
    {
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(msg))) => Err(E::from(msg)),
            Ok(Err(_)) => Err(E::from("Authentication channel closed".to_string())),
            Err(_) => {
                // Don't clear the sender: a concurrent begin() may have replaced it,
                // and guard.take() would cancel the newer sender. The next begin()
                // call cleans up any stale sender.
                Err(E::from("Authentication timed out".to_string()))
            }
        }
    }

    /// Waits for the tracker to enter the authenticated state.
    ///
    /// Returns `true` if authenticated within the timeout, `false` if the timeout
    /// expires or authentication explicitly fails. Uses event-driven notification
    /// from `succeed()` / `fail()` / `invalidate()` to avoid polling.
    ///
    /// Returns early with `false` when `fail()` is called (e.g., the exchange
    /// rejects credentials), so callers are not blocked for the full timeout
    /// on a definitive auth rejection.
    ///
    /// This is intended for callers on a separate task who need to gate operations
    /// on authentication state (e.g., order sends that must wait for re-authentication
    /// after a WebSocket reconnection).
    pub async fn wait_for_authenticated(&self, timeout: Duration) -> bool {
        if self.is_authenticated() {
            return true;
        }

        tokio::time::timeout(timeout, async {
            loop {
                let notified = self.state_notify.notified();

                match self.auth_state() {
                    AuthState::Authenticated => return true,
                    AuthState::Failed => return false,
                    AuthState::Unauthenticated => notified.await,
                }
            }
        })
        .await
        .unwrap_or(false)
    }
}

impl Default for AuthTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };

    use rstest::rstest;

    use super::*;

    #[derive(Debug, PartialEq)]
    struct TestError(String);

    impl From<String> for TestError {
        fn from(msg: String) -> Self {
            Self(msg)
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_successful_authentication() {
        let tracker = AuthTracker::new();
        let rx = tracker.begin();

        tracker.succeed();

        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;

        assert!(result.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn test_failed_authentication() {
        let tracker = AuthTracker::new();
        let rx = tracker.begin();

        tracker.fail("Invalid credentials");

        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;

        assert_eq!(
            result.unwrap_err(),
            TestError("Invalid credentials".to_string())
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_authentication_timeout() {
        let tracker = AuthTracker::new();
        let rx = tracker.begin();

        // Don't call succeed or fail - let it timeout

        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_millis(50), rx).await;

        assert_eq!(
            result.unwrap_err(),
            TestError("Authentication timed out".to_string())
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_begin_supersedes_previous_sender() {
        let tracker = AuthTracker::new();

        let first = tracker.begin();
        let second = tracker.begin();

        // First receiver should get superseded error
        let result = first.await.expect("oneshot closed unexpectedly");
        assert_eq!(result, Err("Authentication attempt superseded".to_string()));

        // Second attempt should succeed
        tracker.succeed();
        let result: Result<(), TestError> = tracker
            .wait_for_result(Duration::from_secs(1), second)
            .await;

        assert!(result.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn test_succeed_without_pending_auth() {
        let tracker = AuthTracker::new();

        // Calling succeed without begin should not panic
        tracker.succeed();
    }

    #[rstest]
    #[tokio::test]
    async fn test_fail_without_pending_auth() {
        let tracker = AuthTracker::new();

        // Calling fail without begin should not panic
        tracker.fail("Some error");
    }

    #[rstest]
    #[tokio::test]
    async fn test_multiple_sequential_authentications() {
        let tracker = AuthTracker::new();

        // First auth succeeds
        let rx1 = tracker.begin();
        tracker.succeed();
        let result1: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx1).await;
        assert!(result1.is_ok());

        // Second auth fails
        let rx2 = tracker.begin();
        tracker.fail("Credentials expired");
        let result2: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx2).await;
        assert_eq!(
            result2.unwrap_err(),
            TestError("Credentials expired".to_string())
        );

        // Third auth succeeds
        let rx3 = tracker.begin();
        tracker.succeed();
        let result3: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx3).await;
        assert!(result3.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn test_channel_closed_before_result() {
        let tracker = AuthTracker::new();
        let rx = tracker.begin();

        // Drop the tracker's sender by starting a new auth
        tracker.begin();

        // Original receiver should get channel closed error
        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;

        assert_eq!(
            result.unwrap_err(),
            TestError("Authentication attempt superseded".to_string())
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_concurrent_auth_attempts() {
        let tracker = Arc::new(AuthTracker::new());
        let mut handles = vec![];

        // Spawn 10 concurrent auth attempts
        for i in 0..10 {
            let tracker_clone = Arc::clone(&tracker);
            let handle = tokio::spawn(async move {
                let rx = tracker_clone.begin();

                // Only the last one should succeed
                if i == 9 {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    tracker_clone.succeed();
                }

                let result: Result<(), TestError> = tracker_clone
                    .wait_for_result(Duration::from_secs(1), rx)
                    .await;

                (i, result)
            });
            handles.push(handle);
        }

        let mut successes = 0;
        let mut superseded = 0;

        for handle in handles {
            let (i, result) = handle.await.unwrap();
            match result {
                Ok(()) => {
                    // Only task 9 should succeed
                    assert_eq!(i, 9);
                    successes += 1;
                }
                Err(TestError(msg)) if msg.contains("superseded") => {
                    superseded += 1;
                }
                Err(e) => panic!("Unexpected error: {e:?}"),
            }
        }

        assert_eq!(successes, 1);
        assert_eq!(superseded, 9);
    }

    #[rstest]
    fn test_default_trait() {
        let _tracker = AuthTracker::default();
    }

    #[rstest]
    #[tokio::test]
    async fn test_clone_trait() {
        let tracker = AuthTracker::new();
        let cloned = tracker.clone();

        // Verify cloned instance shares state with original (Arc behavior)
        let rx = tracker.begin();
        cloned.succeed(); // Succeed via clone affects original
        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_debug_trait() {
        let tracker = AuthTracker::new();
        let debug_str = format!("{tracker:?}");
        assert!(debug_str.contains("AuthTracker"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_timeout_clears_sender() {
        let tracker = AuthTracker::new();

        // Start auth that will timeout
        let rx1 = tracker.begin();
        let result1: Result<(), TestError> = tracker
            .wait_for_result(Duration::from_millis(50), rx1)
            .await;
        assert_eq!(
            result1.unwrap_err(),
            TestError("Authentication timed out".to_string())
        );

        // Verify sender was cleared - new auth should work
        let rx2 = tracker.begin();
        tracker.succeed();
        let result2: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx2).await;
        assert!(result2.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn test_fail_clears_sender() {
        let tracker = AuthTracker::new();

        // Auth fails
        let rx1 = tracker.begin();
        tracker.fail("Bad credentials");
        let result1: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx1).await;
        assert!(result1.is_err());

        // Verify sender was cleared - new auth should work
        let rx2 = tracker.begin();
        tracker.succeed();
        let result2: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx2).await;
        assert!(result2.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn test_succeed_clears_sender() {
        let tracker = AuthTracker::new();

        // Auth succeeds
        let rx1 = tracker.begin();
        tracker.succeed();
        let result1: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx1).await;
        assert!(result1.is_ok());

        // Verify sender was cleared - new auth should work
        let rx2 = tracker.begin();
        tracker.succeed();
        let result2: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx2).await;
        assert!(result2.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn test_rapid_begin_succeed_cycles() {
        let tracker = AuthTracker::new();

        // Rapidly cycle through auth attempts
        for _ in 0..100 {
            let rx = tracker.begin();
            tracker.succeed();
            let result: Result<(), TestError> =
                tracker.wait_for_result(Duration::from_secs(1), rx).await;
            assert!(result.is_ok());
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_double_succeed_is_safe() {
        let tracker = AuthTracker::new();
        let rx = tracker.begin();

        // Call succeed twice
        tracker.succeed();
        tracker.succeed(); // Second call should be no-op

        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;
        assert!(result.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn test_double_fail_is_safe() {
        let tracker = AuthTracker::new();
        let rx = tracker.begin();

        // Call fail twice
        tracker.fail("Error 1");
        tracker.fail("Error 2"); // Second call should be no-op

        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;
        assert_eq!(
            result.unwrap_err(),
            TestError("Error 1".to_string()) // Should be first error
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_succeed_after_fail_is_ignored() {
        let tracker = AuthTracker::new();
        let rx = tracker.begin();

        tracker.fail("Auth failed");
        tracker.succeed(); // This should be no-op

        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;
        assert!(result.is_err()); // Should still be error
    }

    #[rstest]
    #[tokio::test]
    async fn test_fail_after_succeed_is_ignored() {
        let tracker = AuthTracker::new();
        let rx = tracker.begin();

        tracker.succeed();
        tracker.fail("Auth failed"); // This should be no-op

        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;
        assert!(result.is_ok()); // Should still be success
    }

    /// Simulates a reconnect flow where authentication must complete before resubscription.
    ///
    /// This is an integration-style test that verifies:
    /// 1. On reconnect, authentication starts first
    /// 2. Subscription logic waits for auth to complete
    /// 3. Subscriptions only proceed after successful auth
    #[rstest]
    #[tokio::test]
    async fn test_reconnect_flow_waits_for_auth() {
        let tracker = Arc::new(AuthTracker::new());
        let subscribed = Arc::new(tokio::sync::Notify::new());
        let auth_completed = Arc::new(tokio::sync::Notify::new());

        // Simulate reconnect handler
        let tracker_reconnect = Arc::clone(&tracker);
        let subscribed_reconnect = Arc::clone(&subscribed);
        let auth_completed_reconnect = Arc::clone(&auth_completed);

        let reconnect_task = tokio::spawn(async move {
            // Step 1: Begin authentication
            let rx = tracker_reconnect.begin();

            // Step 2: Spawn resubscription task that waits for auth
            let tracker_resub = Arc::clone(&tracker_reconnect);
            let subscribed_resub = Arc::clone(&subscribed_reconnect);
            let auth_completed_resub = Arc::clone(&auth_completed_reconnect);

            let resub_task = tokio::spawn(async move {
                // Wait for auth to complete
                let result: Result<(), TestError> = tracker_resub
                    .wait_for_result(Duration::from_secs(5), rx)
                    .await;

                if result.is_ok() {
                    auth_completed_resub.notify_one();
                    // Simulate resubscription
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    subscribed_resub.notify_one();
                }
            });

            resub_task.await.unwrap();
        });

        // Simulate server auth response after delay
        tokio::time::sleep(Duration::from_millis(100)).await;
        tracker.succeed();

        // Wait for reconnect flow to complete
        reconnect_task.await.unwrap();

        // Verify auth completed before subscription
        tokio::select! {
            () = auth_completed.notified() => {
                // Good - auth completed
            }
            () = tokio::time::sleep(Duration::from_secs(1)) => {
                panic!("Auth never completed");
            }
        }

        // Verify subscription completed
        tokio::select! {
            () = subscribed.notified() => {
                // Good - subscribed
            }
            () = tokio::time::sleep(Duration::from_secs(1)) => {
                panic!("Subscription never completed");
            }
        }
    }

    /// Verifies that failed authentication prevents resubscription in reconnect flow.
    #[rstest]
    #[tokio::test]
    async fn test_reconnect_flow_blocks_on_auth_failure() {
        let tracker = Arc::new(AuthTracker::new());
        let subscribed = Arc::new(AtomicBool::new(false));

        let tracker_reconnect = Arc::clone(&tracker);
        let subscribed_reconnect = Arc::clone(&subscribed);

        let reconnect_task = tokio::spawn(async move {
            let rx = tracker_reconnect.begin();

            // Spawn resubscription task that waits for auth
            let tracker_resub = Arc::clone(&tracker_reconnect);
            let subscribed_resub = Arc::clone(&subscribed_reconnect);

            let resub_task = tokio::spawn(async move {
                let result: Result<(), TestError> = tracker_resub
                    .wait_for_result(Duration::from_secs(5), rx)
                    .await;

                // Only subscribe if auth succeeds
                if result.is_ok() {
                    subscribed_resub.store(true, Ordering::Relaxed);
                }
            });

            resub_task.await.unwrap();
        });

        // Simulate server auth failure
        tokio::time::sleep(Duration::from_millis(50)).await;
        tracker.fail("Invalid credentials");

        // Wait for reconnect flow to complete
        reconnect_task.await.unwrap();

        // Verify subscription never happened
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!subscribed.load(Ordering::Relaxed));
    }

    /// Tests state machine transitions exhaustively.
    #[rstest]
    #[tokio::test]
    async fn test_state_machine_transitions() {
        let tracker = AuthTracker::new();

        // Transition 1: Initial -> Pending (begin)
        let rx1 = tracker.begin();

        // Transition 2: Pending -> Success (succeed)
        tracker.succeed();
        let result1: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx1).await;
        assert!(result1.is_ok());

        // Transition 3: Success -> Pending (begin again)
        let rx2 = tracker.begin();

        // Transition 4: Pending -> Failure (fail)
        tracker.fail("Error");
        let result2: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx2).await;
        assert!(result2.is_err());

        // Transition 5: Failure -> Pending (begin again)
        let rx3 = tracker.begin();

        // Transition 6: Pending -> Timeout
        let result3: Result<(), TestError> = tracker
            .wait_for_result(Duration::from_millis(50), rx3)
            .await;
        assert_eq!(
            result3.unwrap_err(),
            TestError("Authentication timed out".to_string())
        );

        // Transition 7: Timeout -> Pending (begin again)
        let rx4 = tracker.begin();

        // Transition 8: Pending -> Superseded (begin interrupts)
        let rx5 = tracker.begin();
        let result4: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx4).await;
        assert_eq!(
            result4.unwrap_err(),
            TestError("Authentication attempt superseded".to_string())
        );

        // Final success to clean up
        tracker.succeed();
        let result5: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx5).await;
        assert!(result5.is_ok());
    }

    /// Verifies no memory leaks from orphaned senders.
    #[rstest]
    #[tokio::test]
    async fn test_no_sender_leaks() {
        let tracker = AuthTracker::new();

        for _ in 0..100 {
            let rx = tracker.begin();
            let _result: Result<(), TestError> =
                tracker.wait_for_result(Duration::from_millis(1), rx).await;
        }

        let rx = tracker.begin();
        tracker.succeed();
        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;
        assert!(result.is_ok());
    }

    /// Tests concurrent success/fail calls don't cause panics.
    #[rstest]
    #[tokio::test]
    async fn test_concurrent_succeed_fail_calls() {
        let tracker = Arc::new(AuthTracker::new());
        let rx = tracker.begin();

        let mut handles = vec![];

        // Spawn many tasks trying to succeed
        for _ in 0..50 {
            let tracker_clone = Arc::clone(&tracker);
            handles.push(tokio::spawn(async move {
                tracker_clone.succeed();
            }));
        }

        // Spawn many tasks trying to fail
        for _ in 0..50 {
            let tracker_clone = Arc::clone(&tracker);
            handles.push(tokio::spawn(async move {
                tracker_clone.fail("Error");
            }));
        }

        // Wait for all tasks
        for handle in handles {
            handle.await.unwrap();
        }

        // Should get either success or failure, but not panic
        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_secs(1), rx).await;
        // Don't care which outcome, just that it doesn't panic
        let _ = result;
    }

    #[rstest]
    fn test_is_authenticated_initial_state() {
        let tracker = AuthTracker::new();
        assert!(!tracker.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_is_authenticated_after_succeed() {
        let tracker = AuthTracker::new();
        assert!(!tracker.is_authenticated());

        let _rx = tracker.begin();
        assert!(!tracker.is_authenticated());

        tracker.succeed();
        assert!(tracker.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_is_authenticated_after_fail() {
        let tracker = AuthTracker::new();
        let _rx = tracker.begin();
        tracker.fail("error");
        assert!(!tracker.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_invalidate_clears_auth_state() {
        let tracker = AuthTracker::new();
        let _rx = tracker.begin();
        tracker.succeed();
        assert!(tracker.is_authenticated());

        tracker.invalidate();
        assert!(!tracker.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_begin_clears_auth_state() {
        let tracker = AuthTracker::new();
        let _rx1 = tracker.begin();
        tracker.succeed();
        assert!(tracker.is_authenticated());

        let _rx2 = tracker.begin();
        assert!(!tracker.is_authenticated());
    }

    #[rstest]
    fn test_is_authenticated_shared_across_clones() {
        let tracker = AuthTracker::new();
        let cloned = tracker.clone();

        let _rx = tracker.begin();
        tracker.succeed();

        assert!(cloned.is_authenticated());
    }

    #[rstest]
    fn test_invalidate_shared_across_clones() {
        let tracker = AuthTracker::new();
        let cloned = tracker.clone();

        let _rx = tracker.begin();
        tracker.succeed();
        assert!(tracker.is_authenticated());

        cloned.invalidate();
        assert!(!tracker.is_authenticated());
    }

    #[rstest]
    fn test_succeed_without_begin_still_updates_auth_state() {
        let tracker = AuthTracker::new();
        assert!(!tracker.is_authenticated());

        // State updates even without begin() to handle late responses after timeout
        tracker.succeed();
        assert!(tracker.is_authenticated());
    }

    #[rstest]
    fn test_fail_without_begin_still_updates_auth_state() {
        let tracker = AuthTracker::new();
        tracker.succeed();
        assert!(tracker.is_authenticated());

        // State updates even without begin() to handle late responses
        tracker.fail("error");
        assert!(!tracker.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_auth_state_false_after_timeout_until_late_response() {
        let tracker = AuthTracker::new();
        let rx = tracker.begin();
        assert!(!tracker.is_authenticated());

        let result: Result<(), TestError> =
            tracker.wait_for_result(Duration::from_millis(10), rx).await;

        assert!(result.is_err());
        assert!(!tracker.is_authenticated());

        // Late response after timeout still updates state
        tracker.succeed();
        assert!(tracker.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_for_authenticated_already_authenticated() {
        let tracker = AuthTracker::new();
        let _rx = tracker.begin();
        tracker.succeed();

        assert!(
            tracker
                .wait_for_authenticated(Duration::from_millis(50))
                .await
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_for_authenticated_succeeds_after_delay() {
        let tracker = AuthTracker::new();
        let _rx = tracker.begin();

        let tracker_clone = tracker.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            tracker_clone.succeed();
        });

        assert!(tracker.wait_for_authenticated(Duration::from_secs(1)).await);
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_for_authenticated_returns_false_on_failure() {
        let tracker = AuthTracker::new();
        let _rx = tracker.begin();

        let tracker_clone = tracker.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            tracker_clone.fail("rejected");
        });

        let start = tokio::time::Instant::now();
        let result = tracker.wait_for_authenticated(Duration::from_secs(5)).await;
        let elapsed = start.elapsed();

        assert!(!result);
        assert!(elapsed < Duration::from_secs(1));
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_for_authenticated_times_out() {
        let tracker = AuthTracker::new();
        let _rx = tracker.begin();

        assert!(
            !tracker
                .wait_for_authenticated(Duration::from_millis(50))
                .await
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_for_authenticated_begin_clears_failed() {
        let tracker = AuthTracker::new();
        let _rx = tracker.begin();
        tracker.fail("first attempt");

        assert!(
            !tracker
                .wait_for_authenticated(Duration::from_millis(10))
                .await
        );

        // begin() clears the failed flag, allowing a fresh wait
        let _rx = tracker.begin();

        let tracker_clone = tracker.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            tracker_clone.succeed();
        });

        assert!(tracker.wait_for_authenticated(Duration::from_secs(1)).await);
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_for_authenticated_invalidate_does_not_return_false() {
        let tracker = AuthTracker::new();
        let _rx = tracker.begin();

        let tracker_clone = tracker.clone();

        tokio::spawn(async move {
            // invalidate wakes the loop but should not cause early false return
            tokio::time::sleep(Duration::from_millis(20)).await;
            tracker_clone.invalidate();
            // then succeed shortly after
            tokio::time::sleep(Duration::from_millis(20)).await;
            tracker_clone.succeed();
        });

        assert!(tracker.wait_for_authenticated(Duration::from_secs(1)).await);
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_for_authenticated_concurrent_waiters() {
        let tracker = Arc::new(AuthTracker::new());
        let _rx = tracker.begin();

        let mut handles = vec![];

        for _ in 0..10 {
            let t = Arc::clone(&tracker);
            handles.push(tokio::spawn(async move {
                t.wait_for_authenticated(Duration::from_secs(1)).await
            }));
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
        tracker.succeed();

        for handle in handles {
            assert!(handle.await.unwrap());
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_for_authenticated_not_authenticated_initially() {
        let tracker = AuthTracker::new();

        // Not authenticated, no begin() called, no failed flag set
        // Should time out
        assert!(
            !tracker
                .wait_for_authenticated(Duration::from_millis(50))
                .await
        );
    }
}

#[cfg(test)]
mod proptest_tests {
    use std::{sync::Arc, time::Duration};

    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    proptest! {
        /// Verifies that any sequence of begin/succeed/fail/invalidate calls
        /// leaves the tracker in a consistent state where `is_authenticated`
        /// agrees with the last state-setting call.
        #[rstest]
        fn test_state_consistency_after_random_operations(
            ops in proptest::collection::vec(0u8..4, 1..50)
        ) {
            let tracker = AuthTracker::new();
            let mut expected_auth = false;

            for op in &ops {
                match op {
                    0 => {
                        let _rx = tracker.begin();
                        expected_auth = false;
                    }
                    1 => {
                        tracker.succeed();
                        expected_auth = true;
                    }
                    2 => {
                        tracker.fail("test");
                        expected_auth = false;
                    }
                    3 => {
                        tracker.invalidate();
                        expected_auth = false;
                    }
                    _ => unreachable!(),
                }
            }

            prop_assert_eq!(tracker.is_authenticated(), expected_auth);
        }

        /// Verifies that begin() always clears the failed flag regardless of
        /// prior state, so a new auth attempt starts clean.
        #[rstest]
        fn test_begin_always_clears_failed(
            prior_ops in proptest::collection::vec(0u8..4, 0..20)
        ) {
            let tracker = AuthTracker::new();

            for op in &prior_ops {
                match op {
                    0 => { let _rx = tracker.begin(); }
                    1 => tracker.succeed(),
                    2 => tracker.fail("test"),
                    3 => tracker.invalidate(),
                    _ => unreachable!(),
                }
            }

            let _rx = tracker.begin();
            // After begin(), state is Unauthenticated
            prop_assert_eq!(tracker.auth_state(), AuthState::Unauthenticated);
        }

        /// Verifies that succeed() always transitions to Authenticated,
        /// regardless of prior state.
        #[rstest]
        fn test_succeed_always_sets_authenticated(
            prior_ops in proptest::collection::vec(0u8..4, 0..20)
        ) {
            let tracker = AuthTracker::new();

            for op in &prior_ops {
                match op {
                    0 => { let _rx = tracker.begin(); }
                    1 => tracker.succeed(),
                    2 => tracker.fail("test"),
                    3 => tracker.invalidate(),
                    _ => unreachable!(),
                }
            }

            tracker.succeed();
            prop_assert_eq!(tracker.auth_state(), AuthState::Authenticated);
        }
    }

    /// Verifies that `wait_for_authenticated` returns within a bounded time
    /// when `succeed()` or `fail()` is called, regardless of the timeout value.
    #[rstest]
    #[tokio::test]
    async fn test_wait_responds_within_bounded_time() {
        for auth_result in [true, false] {
            let tracker = Arc::new(AuthTracker::new());
            let _rx = tracker.begin();

            let tracker_clone = Arc::clone(&tracker);

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;

                if auth_result {
                    tracker_clone.succeed();
                } else {
                    tracker_clone.fail("rejected");
                }
            });

            let start = tokio::time::Instant::now();
            let result = tracker
                .wait_for_authenticated(Duration::from_secs(10))
                .await;
            let elapsed = start.elapsed();

            assert_eq!(result, auth_result);
            assert!(
                elapsed < Duration::from_millis(500),
                "wait_for_authenticated took {elapsed:?} for auth_result={auth_result}"
            );
        }
    }
}
