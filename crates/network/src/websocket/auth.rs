// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
//! - **Oneshot signaling**: Each auth attempt gets a dedicated channel for result notification.
//! - **Superseding logic**: New authentication requests cancel pending ones.
//! - **Timeout handling**: Configurable timeout for authentication responses.
//! - **Generic error mapping**: Adapters can map to their specific error types.
//!
//! # Recommended Integration Pattern
//!
//! Based on production usage, the recommended pattern is:
//!
//! 1. **Authentication guard**: Maintain `Arc<AtomicBool>` to track auth state separately from tracker.
//! 2. **Guard checks**: Check guard before all private operations (orders, cancels, etc.).
//! 3. **Reconnection flow**: Authenticate BEFORE resubscribing to topics.
//! 4. **Event propagation**: Send auth failures through event channels to consumers.
//! 5. **State lifecycle**: Clear guard on disconnect, set on auth success.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub type AuthResultSender = tokio::sync::oneshot::Sender<Result<(), String>>;
pub type AuthResultReceiver = tokio::sync::oneshot::Receiver<Result<(), String>>;

/// Generic authentication state tracker for WebSocket connections.
///
/// Coordinates authentication attempts by providing a channel-based signaling
/// mechanism. Each authentication attempt receives a dedicated oneshot channel
/// that will be resolved when the server responds.
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
}

impl AuthTracker {
    /// Creates a new authentication tracker.
    pub fn new() -> Self {
        Self {
            tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Begins a new authentication attempt.
    ///
    /// Returns a receiver that will be notified when authentication completes.
    /// If a previous authentication attempt is still pending, it will be cancelled
    /// with an error message indicating it was superseded.
    pub fn begin(&self) -> AuthResultReceiver {
        let (sender, receiver) = tokio::sync::oneshot::channel();

        if let Ok(mut guard) = self.tx.lock() {
            if let Some(old) = guard.take() {
                tracing::warn!("New authentication request superseding previous pending request");
                let _ = old.send(Err("Authentication attempt superseded".to_string()));
            } else {
                tracing::debug!("Starting new authentication request");
            }
            *guard = Some(sender);
        }

        receiver
    }

    /// Marks the current authentication attempt as successful.
    ///
    /// Notifies the waiting receiver with `Ok(())`. This should be called
    /// when the server sends a successful authentication response.
    ///
    /// If no authentication attempt is pending, this is a no-op.
    pub fn succeed(&self) {
        if let Ok(mut guard) = self.tx.lock()
            && let Some(sender) = guard.take()
        {
            let _ = sender.send(Ok(()));
        }
    }

    /// Marks the current authentication attempt as failed.
    ///
    /// Notifies the waiting receiver with `Err(message)`. This should be called
    /// when the server sends an authentication error response.
    ///
    /// If no authentication attempt is pending, this is a no-op.
    pub fn fail(&self, error: impl Into<String>) {
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
                // Clear the sender on timeout to prevent memory leak
                if let Ok(mut guard) = self.tx.lock() {
                    guard.take();
                }
                Err(E::from("Authentication timed out".to_string()))
            }
        }
    }
}

impl Default for AuthTracker {
    fn default() -> Self {
        Self::new()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
            _ = auth_completed.notified() => {
                // Good - auth completed
            }
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                panic!("Auth never completed");
            }
        }

        // Verify subscription completed
        tokio::select! {
            _ = subscribed.notified() => {
                // Good - subscribed
            }
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
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
}
