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

//! Retry policy for asynchronous network operations.

use std::{fmt::Display, future::Future, marker::PhantomData, time::Duration};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{backoff::ExponentialBackoff, dst};

/// Configuration for retry behavior.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (total attempts = 1 initial + `max_retries`).
    pub max_retries: u32,
    /// Initial delay between retries in milliseconds.
    pub initial_delay_ms: u64,
    /// Maximum delay between retries in milliseconds.
    pub max_delay_ms: u64,
    /// Backoff multiplier factor.
    pub backoff_factor: f64,
    /// Maximum jitter in milliseconds to add to delays.
    pub jitter_ms: u64,
    /// Optional timeout for individual operations in milliseconds. `None` disables the timeout.
    pub operation_timeout_ms: Option<u64>,
    /// Whether the first retry occurs without delay.
    ///
    /// Connection operations typically enable this, while HTTP and order operations typically
    /// retain a delay.
    pub immediate_first: bool,
    /// Optional maximum total elapsed time across all attempts and retry delays in milliseconds.
    /// When set, this deadline also bounds an in-flight operation.
    pub max_elapsed_ms: Option<u64>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1_000,
            max_delay_ms: 10_000,
            backoff_factor: 2.0,
            jitter_ms: 100,
            operation_timeout_ms: Some(30_000),
            immediate_first: false,
            max_elapsed_ms: None,
        }
    }
}

/// A failure synthesized by retry machinery.
///
/// This type describes the retry control path only. It does not indicate whether an operation was
/// transmitted or applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetryError {
    /// The cancellation token was set.
    Canceled,
    /// A single operation attempt exceeded its configured timeout.
    OperationTimeout {
        /// Configured timeout for each attempt in milliseconds.
        timeout_ms: u64,
    },
    /// The total elapsed-time budget was exhausted.
    ElapsedBudgetExceeded {
        /// One-based attempt position when the budget was exhausted.
        attempt: u32,
        /// Maximum number of attempts allowed by the retry configuration.
        max_attempts: u32,
        /// Last operation error when budget exhaustion followed a failed attempt.
        last_error: Option<String>,
    },
    /// The retry configuration could not create a backoff state.
    InvalidConfiguration {
        /// Configuration validation error.
        message: String,
    },
}

impl Display for RetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canceled => write!(f, "canceled"),
            Self::OperationTimeout { timeout_ms } => {
                write!(f, "Timed out after {timeout_ms}ms")
            }
            Self::ElapsedBudgetExceeded {
                attempt,
                max_attempts,
                last_error,
            } => {
                write!(f, "Retry budget exceeded ({attempt}/{max_attempts})")?;
                if let Some(last_error) = last_error {
                    write!(f, ": last error: {last_error}")?;
                }
                Ok(())
            }
            Self::InvalidConfiguration { message } => {
                write!(f, "Invalid configuration: {message}")
            }
        }
    }
}

impl std::error::Error for RetryError {}

/// A stateless, thread-safe retry manager for network operations.
///
/// Each execution maintains independent backoff and elapsed-time state.
#[derive(Clone, Debug)]
pub struct RetryManager<E> {
    config: RetryConfig,
    _phantom: PhantomData<E>,
}

#[bon::bon]
impl<E> RetryManager<E>
where
    E: std::error::Error,
{
    /// Creates a new retry manager with the given configuration.
    #[must_use]
    pub const fn new(config: RetryConfig) -> Self {
        Self {
            config,
            _phantom: PhantomData,
        }
    }

    /// Creates a retry budget error with attempt context.
    #[inline(always)]
    fn budget_exceeded_error(&self, attempt: u32, last_error: Option<String>) -> RetryError {
        RetryError::ElapsedBudgetExceeded {
            attempt: attempt.saturating_add(1),
            max_attempts: self.config.max_retries.saturating_add(1),
            last_error,
        }
    }

    /// Returns a builder for a retry-managed invocation.
    ///
    /// Set `retry_delay` to derive a minimum delay from an operation error. The retry loop uses the
    /// greater of this minimum and the configured exponential backoff. Retry delays do not consume
    /// the per-operation timeout. If the effective delay cannot fit within the remaining elapsed
    /// budget, the original operation error is returned.
    ///
    /// Set `cancellation_token` to cancel the operation. Cancellation is checked at three points:
    ///
    /// - Before each operation attempt.
    /// - During operation execution through `tokio::select!`.
    /// - During retry delays.
    ///
    /// Cancellation mid-execution takes effect immediately by dropping the in-flight
    /// operation future. For non-idempotent operations (e.g. an order already on the
    /// wire) the outcome of the abandoned attempt is unknown to the caller.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// - The operation returns a non-retryable error or exhausts the configured retries.
    /// - An operation timeout terminates retry execution.
    /// - The total elapsed-time budget expires.
    /// - The backoff state cannot be created from the configuration.
    /// - Cancellation is requested.
    #[expect(
        clippy::type_complexity,
        reason = "bon needs one concrete optional callback type for omitted retry delays"
    )]
    #[builder(finish_fn = execute)]
    pub async fn invocation<F, Fut, T>(
        &self,
        #[builder(start_fn)] operation_name: &str,
        #[builder(start_fn)] operation: F,
        #[builder(start_fn)] should_retry: impl Fn(&E) -> bool,
        #[builder(start_fn)] create_error: impl Fn(RetryError) -> E,
        retry_delay: Option<&(dyn Fn(&E) -> Option<Duration> + Sync)>,
        cancellation_token: Option<&CancellationToken>,
    ) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        self.execute_retry_loop(
            operation_name,
            operation,
            should_retry,
            |e| retry_delay.and_then(|retry_delay| retry_delay(e)),
            create_error,
            cancellation_token,
        )
        .await
    }

    async fn execute_retry_loop<F, Fut, T>(
        &self,
        operation_name: &str,
        mut operation: F,
        should_retry: impl Fn(&E) -> bool,
        retry_delay: impl Fn(&E) -> Option<Duration>,
        create_error: impl Fn(RetryError) -> E,
        cancellation_token: Option<&CancellationToken>,
    ) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(self.config.initial_delay_ms),
            Duration::from_millis(self.config.max_delay_ms),
            self.config.backoff_factor,
            self.config.jitter_ms,
            self.config.immediate_first,
        )
        .map_err(|e| {
            create_error(RetryError::InvalidConfiguration {
                message: e.to_string(),
            })
        })?;

        let mut attempt = 0;
        let start_time = dst::time::Instant::now();
        let max_elapsed = self.config.max_elapsed_ms.map(Duration::from_millis);
        let deadline = max_elapsed.and_then(|duration| start_time.checked_add(duration));
        let mut last_delayed_error = None;

        loop {
            if let Some(token) = cancellation_token
                && token.is_cancelled()
            {
                log::debug!("Operation '{operation_name}' canceled after {attempt} attempts");
                return Err(create_error(RetryError::Canceled));
            }

            if let Some(max_elapsed) = max_elapsed {
                let elapsed = start_time.elapsed();
                if elapsed >= max_elapsed {
                    if let Some(e) = last_delayed_error {
                        return Err(e);
                    }
                    return Err(create_error(self.budget_exceeded_error(attempt, None)));
                }
            }
            last_delayed_error = None;

            let attempt_future = async {
                let result = match (self.config.operation_timeout_ms, cancellation_token) {
                    (Some(timeout_ms), Some(token)) => {
                        tokio::select! {
                            biased;
                            result = dst::time::timeout(Duration::from_millis(timeout_ms), operation()) => result,
                            () = token.cancelled() => {
                                log::debug!("Operation '{operation_name}' canceled during execution");
                                return Err(create_error(RetryError::Canceled));
                            }
                        }
                    }
                    (Some(timeout_ms), None) => {
                        dst::time::timeout(Duration::from_millis(timeout_ms), operation()).await
                    }
                    (None, Some(token)) => tokio::select! {
                        biased;
                        result = operation() => Ok(result),
                        () = token.cancelled() => {
                            log::debug!("Operation '{operation_name}' canceled during execution");
                            return Err(create_error(RetryError::Canceled));
                        }
                    },
                    (None, None) => Ok(operation().await),
                };
                Ok(result)
            };
            let result = if let Some(deadline) = deadline {
                tokio::select! {
                    biased;
                    () = dst::time::sleep_until(deadline) => {
                        if cancellation_token.is_some_and(CancellationToken::is_cancelled) {
                            log::debug!("Operation '{operation_name}' canceled during execution");
                            return Err(create_error(RetryError::Canceled));
                        }
                        return Err(create_error(self.budget_exceeded_error(attempt, None)));
                    }
                    result = attempt_future => result,
                }
            } else {
                attempt_future.await
            }?;

            let (e, minimum_delay, timed_out) = match result {
                Ok(Ok(success)) => {
                    if attempt > 0 {
                        log::trace!(
                            "Operation '{operation_name}' succeeded after {} attempts",
                            attempt + 1
                        );
                    }
                    return Ok(success);
                }
                Ok(Err(e)) => {
                    let minimum_delay = retry_delay(&e);
                    (e, minimum_delay, false)
                }
                Err(_) => (
                    create_error(RetryError::OperationTimeout {
                        timeout_ms: self.config.operation_timeout_ms.unwrap_or(0),
                    }),
                    None,
                    true,
                ),
            };

            if !should_retry(&e) {
                if timed_out {
                    log::trace!("Operation '{operation_name}' non-retryable timeout: {e}");
                } else {
                    log::trace!("Operation '{operation_name}' non-retryable error: {e}");
                }
                return Err(e);
            }

            if attempt >= self.config.max_retries {
                if timed_out {
                    log::trace!(
                        "Operation '{operation_name}' retries exhausted after timeout ({} attempts): {e}",
                        attempt + 1
                    );
                } else {
                    log::trace!(
                        "Operation '{operation_name}' retries exhausted after {} attempts: {e}",
                        attempt + 1
                    );
                }
                return Err(e);
            }

            let mut delay = backoff.next_duration();

            if let Some(minimum_delay) = minimum_delay {
                delay = delay.max(minimum_delay);
            }

            if let Some(max_elapsed_ms) = self.config.max_elapsed_ms {
                let elapsed = start_time.elapsed();
                let remaining = Duration::from_millis(max_elapsed_ms).saturating_sub(elapsed);

                if remaining.is_zero() {
                    if minimum_delay.is_some() {
                        return Err(e);
                    }
                    return Err(create_error(
                        self.budget_exceeded_error(attempt, Some(e.to_string())),
                    ));
                }

                if minimum_delay.is_some() && delay >= remaining {
                    return Err(e);
                }
                delay = delay.min(remaining);
            }

            debug_assert!(
                minimum_delay.is_none_or(|minimum_delay| delay >= minimum_delay),
                "retry delay must honor the error-provided minimum"
            );

            if timed_out {
                log::trace!(
                    "Operation '{operation_name}' attempt {} timed out, retrying in {}ms: {e}",
                    attempt + 1,
                    delay.as_millis()
                );
            } else {
                log::trace!(
                    "Operation '{operation_name}' attempt {} failed, retrying in {}ms: {e}",
                    attempt + 1,
                    delay.as_millis()
                );
            }

            // Yield even on zero-delay to avoid busy-wait loop
            if delay.is_zero() {
                tokio::task::yield_now().await;

                if minimum_delay.is_some() {
                    last_delayed_error = Some(e);
                }
                attempt += 1;
                continue;
            }

            if let Some(token) = cancellation_token {
                tokio::select! {
                    biased;
                    () = dst::time::sleep(delay) => {},
                    () = token.cancelled() => {
                        log::debug!("Operation '{operation_name}' canceled during retry delay (attempt {})", attempt + 1);
                        return Err(create_error(RetryError::Canceled));
                    }
                }
            } else {
                dst::time::sleep(delay).await;
            }

            if minimum_delay.is_some() {
                last_delayed_error = Some(e);
            }

            attempt += 1;
        }
    }
}

/// Convenience function to create a retry manager with default configuration.
#[must_use]
pub fn create_default_retry_manager<E>() -> RetryManager<E>
where
    E: std::error::Error,
{
    RetryManager::new(RetryConfig::default())
}

/// Convenience function to create a retry manager for HTTP operations.
#[must_use]
pub const fn create_http_retry_manager<E>() -> RetryManager<E>
where
    E: std::error::Error,
{
    let config = RetryConfig {
        max_retries: 3,
        initial_delay_ms: 1_000,
        max_delay_ms: 10_000,
        backoff_factor: 2.0,
        jitter_ms: 1_000,
        operation_timeout_ms: Some(60_000), // 60s for HTTP requests
        immediate_first: false,
        max_elapsed_ms: Some(180_000), // 3 minutes total budget
    };
    RetryManager::new(config)
}

/// Convenience function to create a retry manager for WebSocket operations.
#[must_use]
pub const fn create_websocket_retry_manager<E>() -> RetryManager<E>
where
    E: std::error::Error,
{
    let config = RetryConfig {
        max_retries: 5,
        initial_delay_ms: 1_000,
        max_delay_ms: 10_000,
        backoff_factor: 2.0,
        jitter_ms: 1_000,
        operation_timeout_ms: Some(30_000), // 30s for WebSocket operations
        immediate_first: true,
        max_elapsed_ms: Some(120_000), // 2 minutes total budget
    };
    RetryManager::new(config)
}

#[cfg(test)]
mod test_utils {
    use super::RetryError;

    #[derive(Debug, thiserror::Error)]
    pub(super) enum TestError {
        #[error("Retryable error: {0}")]
        Retryable(String),
        #[error("Non-retryable error: {0}")]
        NonRetryable(String),
        #[error("Timeout error: {0}")]
        Timeout(RetryError),
    }

    pub(super) fn should_retry_test_error(error: &TestError) -> bool {
        matches!(error, TestError::Retryable(_))
    }

    pub(super) fn create_test_error(error: RetryError) -> TestError {
        TestError::Timeout(error)
    }
}

// Retry tests run under both real tokio (`#[tokio::test]`, paused-clock when
// the test relies on virtual time advance) and madsim (`#[madsim::test]`,
// virtual time always paused). `tokio::time::advance` has no direct madsim
// equivalent, so explicit clock advances route through `advance_clock` below;
// time reads and sleeps go through the `dst::time` re-export so they pick up
// the runtime-appropriate clock. madsim auto-advances virtual time when all
// tasks block, but `yield_until`-style busy-yield loops keep the runtime
// non-idle, so explicit advances are still needed where they were before.
#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    };

    #[cfg(all(feature = "simulation", madsim))]
    use madsim::task::{spawn, yield_now};
    use rstest::rstest;
    #[cfg(not(all(feature = "simulation", madsim)))]
    use tokio::task::{spawn, yield_now};

    use super::{test_utils::*, *};
    use crate::dst::time;

    const MAX_WAIT_ITERS: usize = 10_000;
    const MAX_ADVANCE_ITERS: usize = 10_000;

    #[cfg(all(feature = "simulation", madsim))]
    pub(crate) async fn advance_clock(d: Duration) {
        madsim::time::advance(d);
        madsim::task::yield_now().await;
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    pub(crate) async fn advance_clock(d: Duration) {
        tokio::time::advance(d).await;
    }

    pub(crate) async fn yield_until<F>(mut condition: F)
    where
        F: FnMut() -> bool,
    {
        for _ in 0..MAX_WAIT_ITERS {
            if condition() {
                return;
            }
            yield_now().await;
        }

        panic!("yield_until timed out waiting for condition");
    }

    pub(crate) async fn advance_until<F>(mut condition: F)
    where
        F: FnMut() -> bool,
    {
        for _ in 0..MAX_ADVANCE_ITERS {
            if condition() {
                return;
            }
            advance_clock(Duration::from_millis(1)).await;
            yield_now().await;
        }

        panic!("advance_until timed out waiting for condition");
    }

    #[rstest]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay_ms, 1_000);
        assert_eq!(config.max_delay_ms, 10_000);
        // `allow` not `expect`: nightly clippy does not fire `float_cmp` inside `assert_eq!`
        #[allow(clippy::float_cmp, reason = "test asserts the default backoff factor")]
        {
            assert_eq!(config.backoff_factor, 2.0);
        }
        assert_eq!(config.jitter_ms, 100);
        assert_eq!(config.operation_timeout_ms, Some(30_000));
        assert!(!config.immediate_first);
        assert_eq!(config.max_elapsed_ms, None);
    }

    #[rstest]
    #[case::canceled(RetryError::Canceled, "canceled")]
    #[case::operation_timeout(
        RetryError::OperationTimeout { timeout_ms: 250 },
        "Timed out after 250ms"
    )]
    #[case::elapsed_budget(
        RetryError::ElapsedBudgetExceeded {
            attempt: 2,
            max_attempts: 4,
            last_error: None,
        },
        "Retry budget exceeded (2/4)"
    )]
    #[case::elapsed_budget_with_last_error(
        RetryError::ElapsedBudgetExceeded {
            attempt: 3,
            max_attempts: 5,
            last_error: Some("network unavailable".to_string()),
        },
        "Retry budget exceeded (3/5): last error: network unavailable"
    )]
    #[case::invalid_configuration(
        RetryError::InvalidConfiguration {
            message: "delay_initial must be non-zero".to_string(),
        },
        "Invalid configuration: delay_initial must be non-zero"
    )]
    fn test_retry_error_display(#[case] error: RetryError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_invalid_configuration_reason() {
        let manager = RetryManager::new(RetryConfig {
            initial_delay_ms: 0,
            ..RetryConfig::default()
        });

        let error = manager
            .invocation(
                "test_invalid_configuration",
                || async { Ok::<i32, TestError>(42) },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await
            .unwrap_err();

        let TestError::Timeout(reason) = error else {
            panic!("expected invalid configuration, was {error}");
        };
        assert_eq!(
            reason,
            RetryError::InvalidConfiguration {
                message: "delay_initial must be non-zero".to_string(),
            }
        );
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_retry_manager_success_first_attempt() {
        let manager = RetryManager::new(RetryConfig::default());

        let result = manager
            .invocation(
                "test_operation",
                || async { Ok::<i32, TestError>(42) },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        assert_eq!(result.unwrap(), 42);
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_retry_manager_non_retryable_error() {
        let manager = RetryManager::new(RetryConfig::default());

        let result = manager
            .invocation(
                "test_operation",
                || async { Err::<i32, TestError>(TestError::NonRetryable("test".to_string())) },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::NonRetryable(_)));
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_retry_manager_retryable_error_exhausted() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let result = manager
            .invocation(
                "test_operation",
                || async { Err::<i32, TestError>(TestError::Retryable("test".to_string())) },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Retryable(_)));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_error_retry_delay_runs_outside_operation_timeout() {
        let config = RetryConfig {
            max_retries: 1,
            initial_delay_ms: 10,
            max_delay_ms: 10,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(50),
            immediate_first: false,
            max_elapsed_ms: Some(500),
        };
        let manager = RetryManager::new(config);
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();
        let start = time::Instant::now();

        let result = manager
            .invocation(
                "test_error_delay",
                move || {
                    let attempts = attempts_clone.clone();
                    async move {
                        if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                            Err(TestError::Retryable("rate limit".to_string()))
                        } else {
                            Ok(42)
                        }
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .retry_delay(&|_| Some(Duration::from_millis(200)))
            .execute()
            .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        #[cfg(not(all(feature = "simulation", madsim)))]
        assert_eq!(start.elapsed(), Duration::from_millis(200));
        #[cfg(all(feature = "simulation", madsim))]
        assert!(
            start.elapsed() >= Duration::from_millis(200)
                && start.elapsed() < Duration::from_millis(201)
        );
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_error_retry_delay_observes_cancellation() {
        let config = RetryConfig {
            max_retries: 1,
            initial_delay_ms: 10,
            max_delay_ms: 10,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(50),
            immediate_first: false,
            max_elapsed_ms: Some(500),
        };
        let manager = RetryManager::new(config);
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();
        let token = CancellationToken::new();
        let cancel = token.clone();

        spawn(async move {
            time::sleep(Duration::from_millis(100)).await;
            cancel.cancel();
        });

        let error = manager
            .invocation(
                "test_error_delay_cancellation",
                move || {
                    let attempts = attempts_clone.clone();
                    async move {
                        attempts.fetch_add(1, Ordering::SeqCst);
                        Err::<i32, TestError>(TestError::Retryable("rate limit".to_string()))
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .retry_delay(&|_| Some(Duration::from_millis(200)))
            .cancellation_token(&token)
            .execute()
            .await
            .unwrap_err();

        assert!(matches!(error, TestError::Timeout(RetryError::Canceled)));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_error_retry_delay_over_budget_returns_original_error() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            max_delay_ms: 10,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(50),
            immediate_first: false,
            max_elapsed_ms: Some(100),
        };
        let manager = RetryManager::new(config);
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let error = manager
            .invocation(
                "test_error_delay_budget",
                move || {
                    let attempts = attempts_clone.clone();
                    async move {
                        attempts.fetch_add(1, Ordering::SeqCst);
                        Err::<i32, TestError>(TestError::Retryable("rate limit".to_string()))
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .retry_delay(&|_| Some(Duration::from_millis(200)))
            .execute()
            .await
            .unwrap_err();

        let TestError::Retryable(message) = error else {
            panic!("expected original retryable error, was {error}");
        };
        assert_eq!(message, "rate limit");
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_error_retry_delay_overshoot_returns_original_error() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            max_delay_ms: 10,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(20),
            immediate_first: false,
            max_elapsed_ms: Some(100),
        };
        let manager = RetryManager::new(config);
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();
        let attempts_wait = attempts.clone();

        let handle = spawn(async move {
            manager
                .invocation(
                    "test_error_delay_overshoot",
                    move || {
                        let attempts = attempts_clone.clone();
                        async move {
                            attempts.fetch_add(1, Ordering::SeqCst);
                            Err::<i32, TestError>(TestError::Retryable("rate limit".to_string()))
                        }
                    },
                    should_retry_test_error,
                    create_test_error,
                )
                .retry_delay(&|_| Some(Duration::from_millis(50)))
                .execute()
                .await
        });

        yield_until(|| attempts_wait.load(Ordering::SeqCst) == 1).await;
        advance_clock(Duration::from_millis(100)).await;

        let error = handle.await.unwrap().unwrap_err();
        let TestError::Retryable(message) = error else {
            panic!("expected original retryable error, was {error}");
        };
        assert_eq!(message, "rate limit");
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_timeout_path() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(50),
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let result = manager
            .invocation(
                "test_timeout",
                || async {
                    time::sleep(Duration::from_millis(100)).await;
                    Ok::<i32, TestError>(42)
                },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        let TestError::Timeout(reason) = result.unwrap_err() else {
            panic!("expected operation timeout");
        };
        assert_eq!(reason, RetryError::OperationTimeout { timeout_ms: 50 });
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_max_elapsed_time_budget() {
        let config = RetryConfig {
            max_retries: 10,
            initial_delay_ms: 50,
            max_delay_ms: 100,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: Some(200),
        };
        let manager = RetryManager::new(config);

        let start = time::Instant::now();
        let result = manager
            .invocation(
                "test_budget",
                || async { Err::<i32, TestError>(TestError::Retryable("test".to_string())) },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        let elapsed = start.elapsed();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
        assert!(elapsed.as_millis() >= 150);
        assert!(elapsed.as_millis() < 1000);
    }

    #[rstest]
    #[case::without_operation_timeout(None)]
    #[case::at_operation_timeout(Some(100))]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_max_elapsed_bounds_in_flight_attempt(#[case] operation_timeout_ms: Option<u64>) {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            max_delay_ms: 20,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms,
            immediate_first: false,
            max_elapsed_ms: Some(100),
        };
        let manager = RetryManager::new(config);
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = Arc::clone(&attempts);
        let completed = Arc::new(AtomicBool::new(false));
        let completed_clone = Arc::clone(&completed);
        let start = time::Instant::now();

        let error = manager
            .invocation(
                "test_in_flight_budget",
                move || {
                    let attempts = Arc::clone(&attempts_clone);
                    let completed = Arc::clone(&completed_clone);
                    async move {
                        attempts.fetch_add(1, Ordering::SeqCst);
                        time::sleep(Duration::from_secs(1)).await;
                        completed.store(true, Ordering::SeqCst);
                        Ok::<i32, TestError>(42)
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await
            .unwrap_err();

        let TestError::Timeout(reason) = error else {
            panic!("expected retry budget timeout, was {error}");
        };
        assert_eq!(
            reason,
            RetryError::ElapsedBudgetExceeded {
                attempt: 1,
                max_attempts: 4,
                last_error: None,
            }
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(!completed.load(Ordering::SeqCst));
        #[cfg(not(all(feature = "simulation", madsim)))]
        assert_eq!(start.elapsed(), Duration::from_millis(100));
        #[cfg(all(feature = "simulation", madsim))]
        assert!(
            start.elapsed() >= Duration::from_millis(100)
                && start.elapsed() < Duration::from_millis(101)
        );
    }

    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_max_elapsed_bounds_later_in_flight_attempt() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            max_delay_ms: 10,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: Some(100),
        };
        let manager = RetryManager::new(config);
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = Arc::clone(&attempts);

        let error = manager
            .invocation(
                "test_later_in_flight_budget",
                move || {
                    let attempt = attempts_clone.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if attempt == 0 {
                            Err::<i32, TestError>(TestError::Retryable("first".to_string()))
                        } else {
                            std::future::pending().await
                        }
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await
            .unwrap_err();

        let TestError::Timeout(reason) = error else {
            panic!("expected retry budget timeout, was {error}");
        };
        assert_eq!(
            reason,
            RetryError::ElapsedBudgetExceeded {
                attempt: 2,
                max_attempts: 4,
                last_error: None,
            }
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_cancellation_takes_precedence_when_total_deadline_is_ready() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            max_delay_ms: 10,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: Some(100),
        };
        let manager = RetryManager::new(config);
        let token = CancellationToken::new();
        let mut operation = Box::pin(
            manager
                .invocation(
                    "test_cancellation_at_deadline",
                    std::future::pending::<Result<i32, TestError>>,
                    should_retry_test_error,
                    create_test_error,
                )
                .cancellation_token(&token)
                .execute(),
        );

        assert!(futures_util::poll!(&mut operation).is_pending());
        advance_clock(Duration::from_millis(100)).await;
        token.cancel();

        let error = operation.await.unwrap_err();
        let TestError::Timeout(reason) = error else {
            panic!("expected cancellation timeout, was {error}");
        };
        assert_eq!(reason, RetryError::Canceled);
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_budget_exceeded_message_format() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 10,
            max_delay_ms: 20,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: Some(35),
        };
        let manager = RetryManager::new(config);

        let result = manager
            .invocation(
                "test_budget_msg",
                || async { Err::<i32, TestError>(TestError::Retryable("test".to_string())) },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        assert!(error_msg.contains("Retry budget exceeded"));
        assert!(error_msg.contains("/6)"));

        let prefix = "Timeout error: Retry budget exceeded (";
        let nums = error_msg
            .strip_circumfix(prefix, ")")
            .or_else(|| error_msg.strip_circumfix(prefix, "): last error: Retryable error: test"))
            .expect("error message should match retry budget format");
        let parts: Vec<&str> = nums.split('/').collect();
        assert_eq!(parts.len(), 2);
        let current: u32 = parts[0].parse().unwrap();
        let total: u32 = parts[1].parse().unwrap();

        assert_eq!(total, 6, "Total should be max_retries + 1");
        assert!(current <= total, "Current attempt should not exceed total");
        assert!(current >= 1, "Current attempt should be at least 1");
    }

    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_budget_exceeded_edge_cases() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 50,
            max_delay_ms: 100,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: Some(100),
        };
        let manager = RetryManager::new(config);

        let attempt_count = Arc::new(AtomicU32::new(0));
        let count_clone = attempt_count.clone();

        let handle = spawn(async move {
            manager
                .invocation(
                    "test_first_attempt",
                    move || {
                        let count = count_clone.clone();
                        async move {
                            count.fetch_add(1, Ordering::SeqCst);
                            Err::<i32, TestError>(TestError::Retryable("test".to_string()))
                        }
                    },
                    should_retry_test_error,
                    create_test_error,
                )
                .execute()
                .await
        });

        // Wait for first attempt
        yield_until(|| attempt_count.load(Ordering::SeqCst) >= 1).await;

        // Advance past budget to trigger check at loop start before second attempt
        advance_clock(Duration::from_millis(101)).await;
        yield_now().await;

        let result = handle.await.unwrap();
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        // Budget check happens at loop start, so shows (2/3) = "starting 2nd of 3 attempts"
        assert!(
            error_msg.contains("(2/3)"),
            "Expected (2/3) but got: {error_msg}"
        );
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_budget_exceeded_no_overflow() {
        let config = RetryConfig {
            max_retries: u32::MAX,
            initial_delay_ms: 10,
            max_delay_ms: 20,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: Some(1),
        };
        let manager = RetryManager::new(config);

        let result = manager
            .invocation(
                "test_overflow",
                || async { Err::<i32, TestError>(TestError::Retryable("test".to_string())) },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        // Should saturate at u32::MAX instead of wrapping to 0
        assert!(error_msg.contains("Retry budget exceeded"));
        assert!(error_msg.contains(&format!("/{}", u32::MAX)));
    }

    #[rstest]
    fn test_http_retry_manager_config() {
        let manager = create_http_retry_manager::<TestError>();
        assert_eq!(manager.config.max_retries, 3);
        assert!(!manager.config.immediate_first);
        assert_eq!(manager.config.max_elapsed_ms, Some(180_000));
    }

    #[rstest]
    fn test_websocket_retry_manager_config() {
        let manager = create_websocket_retry_manager::<TestError>();
        assert_eq!(manager.config.max_retries, 5);
        assert!(manager.config.immediate_first);
        assert_eq!(manager.config.max_elapsed_ms, Some(120_000));
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_timeout_respects_retry_predicate() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(50),
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        // Test with retry predicate that rejects timeouts
        let should_not_retry_timeouts = |error: &TestError| !matches!(error, TestError::Timeout(_));

        let result = manager
            .invocation(
                "test_timeout_non_retryable",
                || async {
                    time::sleep(Duration::from_millis(100)).await;
                    Ok::<i32, TestError>(42)
                },
                should_not_retry_timeouts,
                create_test_error,
            )
            .execute()
            .await;

        // Should fail immediately without retries since timeout is non-retryable
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_timeout_retries_when_predicate_allows() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(50),
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        // Test with retry predicate that allows timeouts
        let should_retry_timeouts = |error: &TestError| matches!(error, TestError::Timeout(_));

        let start = time::Instant::now();
        let result = manager
            .invocation(
                "test_timeout_retryable",
                || async {
                    time::sleep(Duration::from_millis(100)).await;
                    Ok::<i32, TestError>(42)
                },
                should_retry_timeouts,
                create_test_error,
            )
            .execute()
            .await;

        let elapsed = start.elapsed();

        // Should fail after retries (not immediately)
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
        // Should have taken time for retries (at least 2 timeouts + delays)
        assert!(elapsed.as_millis() > 80); // More than just one timeout
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_successful_retry_after_failures() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let result = manager
            .invocation(
                "test_eventual_success",
                move || {
                    let counter = counter_clone.clone();
                    async move {
                        let attempts = counter.fetch_add(1, Ordering::SeqCst);
                        if attempts < 2 {
                            Err(TestError::Retryable("temporary failure".to_string()))
                        } else {
                            Ok(42)
                        }
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
    }

    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_immediate_first_retry() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 100,
            max_delay_ms: 200,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: true,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let attempt_times = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let times_clone = attempt_times.clone();
        let start = time::Instant::now();

        let handle = spawn({
            let times_clone = times_clone.clone();
            async move {
                let _ = manager
                    .invocation(
                        "test_immediate",
                        move || {
                            let times = times_clone.clone();
                            async move {
                                times.lock().push(start.elapsed());
                                Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                            }
                        },
                        should_retry_test_error,
                        create_test_error,
                    )
                    .execute()
                    .await;
            }
        });

        // Allow initial attempt and immediate retry to run without advancing time
        yield_until(|| attempt_times.lock().len() >= 2).await;

        // Advance time for the next backoff interval
        advance_clock(Duration::from_millis(100)).await;
        yield_now().await;

        // Wait for the final retry to be recorded
        yield_until(|| attempt_times.lock().len() >= 3).await;

        handle.await.unwrap();

        let times = attempt_times.lock();
        assert_eq!(times.len(), 3); // Initial + 2 retries

        // First retry should be immediate (within 1ms tolerance)
        assert!(times[1] <= Duration::from_millis(1));
        // Second retry should have backoff delay (at least 100ms from start)
        assert!(times[2] >= Duration::from_millis(100));
        assert!(times[2] <= Duration::from_millis(110));
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_operation_without_timeout() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None, // No timeout
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let start = time::Instant::now();
        let result = manager
            .invocation(
                "test_no_timeout",
                || async {
                    time::sleep(Duration::from_millis(50)).await;
                    Ok::<i32, TestError>(42)
                },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        let elapsed = start.elapsed();
        assert_eq!(result.unwrap(), 42);
        // Should complete without timing out
        assert!(elapsed.as_millis() >= 30);
        assert!(elapsed.as_millis() < 200);
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_zero_retries() {
        let config = RetryConfig {
            max_retries: 0,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let result = manager
            .invocation(
                "test_no_retries",
                move || {
                    let counter = counter_clone.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        assert!(result.is_err());
        // Should only attempt once (no retries)
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 1);
    }

    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_jitter_applied() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 50,
            max_delay_ms: 100,
            backoff_factor: 2.0,
            jitter_ms: 50, // Significant jitter
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let delays = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let delays_clone = delays.clone();
        let last_time = Arc::new(parking_lot::Mutex::new(time::Instant::now()));
        let last_time_clone = last_time.clone();

        let handle = spawn({
            let delays_clone = delays_clone.clone();
            async move {
                let _ = manager
                    .invocation(
                        "test_jitter",
                        move || {
                            let delays = delays_clone.clone();
                            let last_time = last_time_clone.clone();
                            async move {
                                let now = time::Instant::now();
                                let delay = {
                                    let mut last = last_time.lock();
                                    let d = now.duration_since(*last);
                                    *last = now;
                                    d
                                };
                                delays.lock().push(delay);
                                Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                            }
                        },
                        should_retry_test_error,
                        create_test_error,
                    )
                    .execute()
                    .await;
            }
        });

        yield_until(|| !delays.lock().is_empty()).await;
        advance_until(|| delays.lock().len() >= 2).await;
        advance_until(|| delays.lock().len() >= 3).await;

        handle.await.unwrap();

        let delays = delays.lock();
        // Skip the first delay (initial attempt)
        for delay in delays.iter().skip(1) {
            // Each delay should be at least the base delay (50ms for first retry)
            assert!(delay.as_millis() >= 50);
            // But no more than base + jitter (allow small tolerance for step advance)
            assert!(delay.as_millis() <= 151);
        }
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_max_elapsed_stops_early() {
        let config = RetryConfig {
            max_retries: 100, // Very high retry count
            initial_delay_ms: 50,
            max_delay_ms: 100,
            backoff_factor: 1.5,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: Some(150), // Should stop after ~3 attempts
        };
        let manager = RetryManager::new(config);

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let start = time::Instant::now();
        let result = manager
            .invocation(
                "test_elapsed_limit",
                move || {
                    let counter = counter_clone.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        let elapsed = start.elapsed();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));

        // Should have stopped due to time limit, not retry count
        let attempts = attempt_counter.load(Ordering::SeqCst);
        assert!(attempts < 10); // Much less than max_retries
        assert!(elapsed.as_millis() >= 100);
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_mixed_errors_retry_behavior() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let result = manager
            .invocation(
                "test_mixed_errors",
                move || {
                    let counter = counter_clone.clone();
                    async move {
                        let attempts = counter.fetch_add(1, Ordering::SeqCst);
                        match attempts {
                            0 => Err(TestError::Retryable("retry 1".to_string())),
                            1 => Err(TestError::Retryable("retry 2".to_string())),
                            2 => Err(TestError::NonRetryable("stop here".to_string())),
                            _ => Ok(42),
                        }
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .execute()
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::NonRetryable(_)));
        // Should stop at the non-retryable error
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_cancellation_during_retry_delay() {
        use tokio_util::sync::CancellationToken;

        let config = RetryConfig {
            max_retries: 10,
            initial_delay_ms: 500, // Long delay to ensure cancellation happens during sleep
            max_delay_ms: 1000,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let token = CancellationToken::new();
        let token_clone = token.clone();

        // Cancel after a short delay
        spawn(async move {
            time::sleep(Duration::from_millis(100)).await;
            token_clone.cancel();
        });

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let start = time::Instant::now();
        let result = manager
            .invocation(
                "test_cancellation",
                move || {
                    let counter = counter_clone.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                    }
                },
                should_retry_test_error,
                create_test_error,
            )
            .cancellation_token(&token)
            .execute()
            .await;

        let elapsed = start.elapsed();

        // Should be canceled quickly
        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(error_msg.contains("canceled"));

        // Should not have taken the full delay time
        assert!(elapsed.as_millis() < 600);

        // Should have made at least one attempt
        let attempts = attempt_counter.load(Ordering::SeqCst);
        assert!(attempts >= 1);
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_cancellation_during_operation_execution() {
        use tokio_util::sync::CancellationToken;

        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 50,
            max_delay_ms: 100,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config);

        let token = CancellationToken::new();
        let token_clone = token.clone();

        // Cancel after a short delay
        spawn(async move {
            time::sleep(Duration::from_millis(50)).await;
            token_clone.cancel();
        });

        let start = time::Instant::now();
        let result = manager
            .invocation(
                "test_cancellation_during_op",
                || async {
                    // Long-running operation
                    time::sleep(Duration::from_millis(200)).await;
                    Ok::<i32, TestError>(42)
                },
                should_retry_test_error,
                create_test_error,
            )
            .cancellation_token(&token)
            .execute()
            .await;

        let elapsed = start.elapsed();

        // Should be canceled during the operation
        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(error_msg.contains("canceled"));

        // Should not have completed the long operation
        assert!(elapsed.as_millis() < 250);
    }

    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_cancellation_error_message() {
        use tokio_util::sync::CancellationToken;

        let config = RetryConfig::default();
        let manager = RetryManager::new(config);

        let token = CancellationToken::new();
        token.cancel(); // Pre-cancel for immediate cancellation

        let result = manager
            .invocation(
                "test_operation",
                || async { Ok::<i32, TestError>(42) },
                should_retry_test_error,
                create_test_error,
            )
            .cancellation_token(&token)
            .execute()
            .await;

        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(error_msg.contains("canceled"));
    }
}

#[cfg(test)]
mod proptest_tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    #[cfg(all(feature = "simulation", madsim))]
    use madsim::task::spawn;
    use proptest::prelude::*;
    // Import rstest attribute macro used within proptest! tests
    use rstest::rstest;
    #[cfg(not(all(feature = "simulation", madsim)))]
    use tokio::task::spawn;

    #[cfg(not(all(feature = "simulation", madsim)))]
    use super::tests::{advance_until, yield_until};
    use super::{test_utils::*, tests::advance_clock, *};
    use crate::dst::time;

    // Each proptest case constructs a runtime to drive the manager via
    // `block_on`. Under tokio, that runtime is paused so virtual sleeps
    // auto-advance; under madsim, the runtime is the deterministic sim
    // runtime, which also runs in virtual time. Both expose `block_on`.
    #[cfg(all(feature = "simulation", madsim))]
    fn build_paused_runtime() -> madsim::runtime::Runtime {
        madsim::runtime::Runtime::new()
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    fn build_paused_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
    }

    proptest! {
        #[rstest]
        fn test_retry_config_valid_ranges(
            max_retries in 0u32..100,
            initial_delay_ms in 1u64..10_000,
            max_delay_ms in 1u64..60_000,
            backoff_factor in 1.0f64..10.0,
            jitter_ms in 0u64..1_000,
            operation_timeout_ms in prop::option::of(1u64..120_000),
            immediate_first in any::<bool>(),
            max_elapsed_ms in prop::option::of(1u64..300_000)
        ) {
            // Ensure max_delay >= initial_delay for valid config
            let max_delay_ms = max_delay_ms.max(initial_delay_ms);

            let config = RetryConfig {
                max_retries,
                initial_delay_ms,
                max_delay_ms,
                backoff_factor,
                jitter_ms,
                operation_timeout_ms,
                immediate_first,
                max_elapsed_ms,
            };

            // Should always be able to create a RetryManager with valid config
            let _manager = RetryManager::<std::io::Error>::new(config);
        }

        #[rstest]
        fn test_retry_attempts_bounded(
            max_retries in 0u32..5,
            initial_delay_ms in 1u64..10,
            backoff_factor in 1.0f64..2.0,
        ) {
            let rt = build_paused_runtime();

            let config = RetryConfig {
                max_retries,
                initial_delay_ms,
                max_delay_ms: initial_delay_ms * 2,
                backoff_factor,
                jitter_ms: 0,
                operation_timeout_ms: None,
                immediate_first: false,
                max_elapsed_ms: None,
            };

            let manager = RetryManager::new(config);
            let attempt_counter = Arc::new(AtomicU32::new(0));
            let counter_clone = attempt_counter.clone();

            let _result = rt.block_on(manager.invocation(
                "prop_test",
                move || {
                    let counter = counter_clone.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                    }
                },
                |e: &TestError| matches!(e, TestError::Retryable(_)),
                TestError::Timeout,
            ).execute());

            let attempts = attempt_counter.load(Ordering::SeqCst);
            // Total attempts should be 1 (initial) + max_retries
            prop_assert_eq!(attempts, max_retries + 1);
        }

        #[rstest]
        fn test_error_retry_delay_obeys_selection_and_budget(
            backoff_ms in 1u64..500,
            minimum_ms in 0u64..1_000,
            operation_timeout_ms in 1u64..50,
        ) {
            let rt = build_paused_runtime();
            let selected_ms = backoff_ms.max(minimum_ms);
            let config = |max_elapsed_ms| RetryConfig {
                max_retries: 1,
                initial_delay_ms: backoff_ms,
                max_delay_ms: backoff_ms,
                backoff_factor: 1.0,
                jitter_ms: 0,
                operation_timeout_ms: Some(operation_timeout_ms),
                immediate_first: false,
                max_elapsed_ms: Some(max_elapsed_ms),
            };
            let minimum_delay = Duration::from_millis(minimum_ms);

            let manager = RetryManager::new(config(selected_ms + 1));
            let attempts = Arc::new(AtomicU32::new(0));
            let attempts_clone = attempts.clone();
            let (result, elapsed) = rt.block_on(async {
                let start = time::Instant::now();
                let result = manager
                    .invocation(
                        "prop_error_delay_selection",
                        move || {
                            let attempts = attempts_clone.clone();
                            async move {
                                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                                    Err(TestError::Retryable("rate limit".to_string()))
                                } else {
                                    Ok(42)
                                }
                            }
                        },
                        should_retry_test_error,
                        create_test_error,
                    )
                    .retry_delay(&|_| Some(minimum_delay))
                    .execute()
                    .await;
                (result, start.elapsed())
            });

            prop_assert_eq!(result.unwrap(), 42);
            prop_assert_eq!(attempts.load(Ordering::SeqCst), 2);
            let selected = Duration::from_millis(selected_ms);
            #[cfg(all(feature = "simulation", madsim))]
            {
                prop_assert!(elapsed >= selected);
                prop_assert!(elapsed < selected + Duration::from_millis(1));
            }
            #[cfg(not(all(feature = "simulation", madsim)))]
            prop_assert_eq!(elapsed, selected);

            let manager = RetryManager::new(config(selected_ms));
            let attempts = Arc::new(AtomicU32::new(0));
            let attempts_clone = attempts.clone();
            let (error, elapsed) = rt.block_on(async {
                let start = time::Instant::now();
                let error = manager
                    .invocation(
                        "prop_error_delay_budget",
                        move || {
                            let attempts = attempts_clone.clone();
                            async move {
                                attempts.fetch_add(1, Ordering::SeqCst);
                                Err::<i32, TestError>(TestError::Retryable(
                                    "rate limit".to_string(),
                                ))
                            }
                        },
                        should_retry_test_error,
                        create_test_error,
                    )
                    .retry_delay(&|_| Some(minimum_delay))
                    .execute()
                    .await
                    .unwrap_err();
                (error, start.elapsed())
            });

            match error {
                TestError::Retryable(message) => prop_assert_eq!(message, "rate limit"),
                error => prop_assert!(false, "expected original retryable error, was {error}"),
            }
            prop_assert_eq!(attempts.load(Ordering::SeqCst), 1);
            prop_assert_eq!(elapsed, Duration::ZERO);
        }

        #[rstest]
        fn test_timeout_always_respected(
            timeout_ms in 10u64..50,
            operation_delay_ms in 60u64..100,
        ) {
            let rt = build_paused_runtime();

            let config = RetryConfig {
                max_retries: 0, // No retries to isolate timeout behavior
                initial_delay_ms: 10,
                max_delay_ms: 100,
                backoff_factor: 2.0,
                jitter_ms: 0,
                operation_timeout_ms: Some(timeout_ms),
                immediate_first: false,
                max_elapsed_ms: None,
            };

            let manager = RetryManager::new(config);

            let result = rt.block_on(async {
                let operation_future = manager.invocation(
                    "timeout_test",
                    move || async move {
                        time::sleep(Duration::from_millis(operation_delay_ms)).await;
                        Ok::<i32, TestError>(42)
                    },
                    |_: &TestError| true,
                    TestError::Timeout,
                ).execute();

                // Advance time to trigger timeout
                advance_clock(Duration::from_millis(timeout_ms + 10)).await;
                operation_future.await
            });

            // Operation should timeout
            prop_assert!(result.is_err());
            prop_assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
        }

        #[rstest]
        fn test_max_elapsed_always_respected(
            max_elapsed_ms in 20u64..50,
            delay_per_retry in 15u64..30,
            max_retries in 10u32..20,
        ) {
            let rt = build_paused_runtime();

            // Set up config where we would exceed max_elapsed_ms before max_retries
            let config = RetryConfig {
                max_retries,
                initial_delay_ms: delay_per_retry,
                max_delay_ms: delay_per_retry * 2,
                backoff_factor: 1.0, // No backoff to make timing predictable
                jitter_ms: 0,
                operation_timeout_ms: None,
                immediate_first: false,
                max_elapsed_ms: Some(max_elapsed_ms),
            };

            let manager = RetryManager::new(config);
            let attempt_counter = Arc::new(AtomicU32::new(0));
            let counter_clone = attempt_counter.clone();

            let result = rt.block_on(async {
                let operation_future = manager.invocation(
                    "elapsed_test",
                    move || {
                        let counter = counter_clone.clone();
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                            Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                        }
                    },
                    |e: &TestError| matches!(e, TestError::Retryable(_)),
                    TestError::Timeout,
                ).execute();

                // Advance time past max_elapsed_ms
                advance_clock(Duration::from_millis(max_elapsed_ms + delay_per_retry)).await;
                operation_future.await
            });

            let attempts = attempt_counter.load(Ordering::SeqCst);

            // Should have failed with timeout error
            prop_assert!(result.is_err());
            prop_assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));

            // Should have stopped before exhausting all retries
            prop_assert!(attempts <= max_retries + 1);
        }

        #[rstest]
        fn test_jitter_bounds(
            jitter_ms in 0u64..20,
            base_delay_ms in 10u64..30,
        ) {
            let rt = build_paused_runtime();

            let config = RetryConfig {
                max_retries: 2,
                initial_delay_ms: base_delay_ms,
                max_delay_ms: base_delay_ms * 2,
                backoff_factor: 1.0, // No backoff to isolate jitter
                jitter_ms,
                operation_timeout_ms: None,
                immediate_first: false,
                max_elapsed_ms: None,
            };

            let manager = RetryManager::new(config);
            let attempt_times = Arc::new(parking_lot::Mutex::new(Vec::new()));
            let attempt_times_for_block = attempt_times.clone();

            rt.block_on(async move {
                #[cfg(not(all(feature = "simulation", madsim)))]
                let attempt_times_for_wait = attempt_times_for_block.clone();
                let handle = spawn({
                    let attempt_times_for_task = attempt_times_for_block.clone();
                    let manager = manager;
                    async move {
                        let start_time = time::Instant::now();
                        let _ = manager
                            .invocation(
                                "jitter_test",
                                move || {
                                    let attempt_times_inner = attempt_times_for_task.clone();
                                    async move {
                                        attempt_times_inner
                                            .lock()
                                            .push(start_time.elapsed());
                                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                                    }
                                },
                                |e: &TestError| matches!(e, TestError::Retryable(_)),
                                TestError::Timeout,
                            ).execute()
                            .await;
                    }
                });

                // Under tokio paused clock, drive virtual time forward in 1ms
                // ticks to release the manager's sleeps; under madsim the
                // runtime auto-advances when all tasks block on virtual time,
                // so awaiting the handle is enough and yields exact timings.
                #[cfg(not(all(feature = "simulation", madsim)))]
                {
                    yield_until(|| !attempt_times_for_wait.lock().is_empty()).await;
                    advance_until(|| attempt_times_for_wait.lock().len() >= 2).await;
                    advance_until(|| attempt_times_for_wait.lock().len() >= 3).await;
                }

                handle.await.unwrap();
            });

            let times = attempt_times.lock();

            // We expect at least 2 attempts total (initial + at least 1 retry)
            prop_assert!(times.len() >= 2);

            // First attempt should be immediate (no delay)
            prop_assert!(times[0].as_millis() < 5);

            // Check subsequent retries have appropriate delays
            for i in 1..times.len() {
                let delay_from_previous = if i == 1 {
                    times[i].checked_sub(times[0]).unwrap()
                } else {
                    times[i].checked_sub(times[i - 1]).unwrap()
                };

                // The delay floor is min(base, max - jitter): near the cap the
                // jittered base is lowered so the spread survives saturation
                let floor = base_delay_ms.min((base_delay_ms * 2).saturating_sub(jitter_ms));
                prop_assert!(
                    delay_from_previous.as_millis() >= u128::from(floor),
                    "Retry {} delay {}ms is less than floor {}ms",
                    i, delay_from_previous.as_millis(), floor
                );

                // Delay should be at most base_delay + jitter
                prop_assert!(
                    delay_from_previous.as_millis() <= u128::from(base_delay_ms + jitter_ms + 1),
                    "Retry {} delay {}ms exceeds base {} + jitter {}",
                    i, delay_from_previous.as_millis(), base_delay_ms, jitter_ms
                );
            }
        }

        #[rstest]
        fn test_immediate_first_property(
            immediate_first in any::<bool>(),
            initial_delay_ms in 10u64..30,
        ) {
            let rt = build_paused_runtime();

            let config = RetryConfig {
                max_retries: 2,
                initial_delay_ms,
                max_delay_ms: initial_delay_ms * 2,
                backoff_factor: 2.0,
                jitter_ms: 0,
                operation_timeout_ms: None,
                immediate_first,
                max_elapsed_ms: None,
            };

            let manager = RetryManager::new(config);
            let attempt_times = Arc::new(parking_lot::Mutex::new(Vec::new()));
            let attempt_times_for_block = attempt_times.clone();

            rt.block_on(async move {
                #[cfg(not(all(feature = "simulation", madsim)))]
                let attempt_times_for_wait = attempt_times_for_block.clone();
                let handle = spawn({
                    let attempt_times_for_task = attempt_times_for_block.clone();
                    let manager = manager;
                    async move {
                        let start = time::Instant::now();
                        let _ = manager
                            .invocation(
                                "immediate_test",
                                move || {
                                    let attempt_times_inner = attempt_times_for_task.clone();
                                    async move {
                                        let elapsed = start.elapsed();
                                        attempt_times_inner.lock().push(elapsed);
                                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                                    }
                                },
                                |e: &TestError| matches!(e, TestError::Retryable(_)),
                                TestError::Timeout,
                            ).execute()
                            .await;
                    }
                });

                // See test_jitter_bounds: madsim auto-advances virtual time
                // when all tasks block on it, so awaiting the handle suffices
                // and avoids the 1ms-tick driver's added scheduler overhead.
                #[cfg(not(all(feature = "simulation", madsim)))]
                {
                    yield_until(|| !attempt_times_for_wait.lock().is_empty()).await;
                    advance_until(|| attempt_times_for_wait.lock().len() >= 2).await;
                    advance_until(|| attempt_times_for_wait.lock().len() >= 3).await;
                }

                handle.await.unwrap();
            });

            let times = attempt_times.lock();
            prop_assert!(times.len() >= 2);

            if immediate_first {
                // First retry should be immediate
                prop_assert!(times[1].as_millis() < 20,
                    "With immediate_first=true, first retry took {}ms",
                    times[1].as_millis());
            } else {
                // First retry should have delay
                prop_assert!(times[1].as_millis() >= u128::from(initial_delay_ms - 1),
                    "With immediate_first=false, first retry was too fast: {}ms",
                    times[1].as_millis());
            }
        }

        #[rstest]
        fn test_non_retryable_stops_immediately(
            attempt_before_non_retryable in 0usize..3,
            max_retries in 3u32..5,
        ) {
            let rt = build_paused_runtime();

            let config = RetryConfig {
                max_retries,
                initial_delay_ms: 10,
                max_delay_ms: 100,
                backoff_factor: 2.0,
                jitter_ms: 0,
                operation_timeout_ms: None,
                immediate_first: false,
                max_elapsed_ms: None,
            };

            let manager = RetryManager::new(config);
            let attempt_counter = Arc::new(AtomicU32::new(0));
            let counter_clone = attempt_counter.clone();

            let result: Result<i32, TestError> = rt.block_on(manager.invocation(
                "non_retryable_test",
                move || {
                    let counter = counter_clone.clone();
                    async move {
                        let attempts = counter.fetch_add(1, Ordering::SeqCst) as usize;
                        if attempts == attempt_before_non_retryable {
                            Err(TestError::NonRetryable("stop".to_string()))
                        } else {
                            Err(TestError::Retryable("retry".to_string()))
                        }
                    }
                },
                |e: &TestError| matches!(e, TestError::Retryable(_)),
                TestError::Timeout,
            ).execute());

            let attempts = attempt_counter.load(Ordering::SeqCst) as usize;

            prop_assert!(result.is_err());
            prop_assert!(matches!(result.unwrap_err(), TestError::NonRetryable(_)));
            // Should stop exactly when non-retryable error occurs
            prop_assert_eq!(attempts, attempt_before_non_retryable + 1);
        }

        #[rstest]
        fn test_cancellation_stops_immediately(
            cancel_after_ms in 10u64..100,
            initial_delay_ms in 200u64..500,
        ) {
            use tokio_util::sync::CancellationToken;

            let rt = build_paused_runtime();

            let config = RetryConfig {
                max_retries: 10,
                initial_delay_ms,
                max_delay_ms: initial_delay_ms * 2,
                backoff_factor: 2.0,
                jitter_ms: 0,
                operation_timeout_ms: None,
                immediate_first: false,
                max_elapsed_ms: None,
            };

            let manager = RetryManager::new(config);
            let token = CancellationToken::new();
            let token_clone = token.clone();

            let result: Result<i32, TestError> = rt.block_on(async {
                // Spawn cancellation task
                spawn(async move {
                    time::sleep(Duration::from_millis(cancel_after_ms)).await;
                    token_clone.cancel();
                });

                let operation_future = manager.invocation(
                    "cancellation_test",
                    || async {
                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                    },
                    |e: &TestError| matches!(e, TestError::Retryable(_)),
                    create_test_error,
                )
                .cancellation_token(&token)
                .execute();

                // Advance time to trigger cancellation
                advance_clock(Duration::from_millis(cancel_after_ms + 10)).await;
                operation_future.await
            });

            // Should be canceled
            prop_assert!(result.is_err());
            let error_msg = format!("{}", result.unwrap_err());
            prop_assert!(error_msg.contains("canceled"));
        }

        #[rstest]
        fn test_budget_clamp_prevents_overshoot(
            max_elapsed_ms in 10u64..30,
            delay_per_retry in 30u64..50,
        ) {
            let rt = build_paused_runtime();

            // Configure so that first retry delay would exceed budget
            let config = RetryConfig {
                max_retries: 5,
                initial_delay_ms: delay_per_retry,
                max_delay_ms: delay_per_retry * 2,
                backoff_factor: 1.0,
                jitter_ms: 0,
                operation_timeout_ms: None,
                immediate_first: false,
                max_elapsed_ms: Some(max_elapsed_ms),
            };

            let manager = RetryManager::new(config);
            let attempts = Arc::new(AtomicU32::new(0));
            let attempts_for_operation = Arc::clone(&attempts);

            let (result, elapsed) = rt.block_on(async {
                let started_at = time::Instant::now();
                let result = manager.invocation(
                    "budget_clamp_test",
                    move || {
                        let attempts = Arc::clone(&attempts_for_operation);
                        async move {
                            attempts.fetch_add(1, Ordering::SeqCst);
                            Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                        }
                    },
                    |e: &TestError| matches!(e, TestError::Retryable(_)),
                    create_test_error,
                ).execute().await;
                (result, started_at.elapsed())
            });

            assert!(matches!(
                result,
                Err(TestError::Timeout(RetryError::ElapsedBudgetExceeded {
                    attempt: 2,
                    max_attempts: 6,
                    last_error: None,
                }))
            ));
            assert_eq!(attempts.load(Ordering::SeqCst), 1);
            #[cfg(not(all(feature = "simulation", madsim)))]
            assert_eq!(elapsed, Duration::from_millis(max_elapsed_ms));
            #[cfg(all(feature = "simulation", madsim))]
            assert!(
                elapsed >= Duration::from_millis(max_elapsed_ms)
                    && elapsed < Duration::from_millis(max_elapsed_ms + 1)
            );
        }

        #[rstest]
        fn test_success_on_kth_attempt(
            k in 1usize..5,
            initial_delay_ms in 5u64..20,
        ) {
            let rt = build_paused_runtime();

            let config = RetryConfig {
                max_retries: 10, // More than k
                initial_delay_ms,
                max_delay_ms: initial_delay_ms * 4,
                backoff_factor: 2.0,
                jitter_ms: 0,
                operation_timeout_ms: None,
                immediate_first: false,
                max_elapsed_ms: None,
            };

            let manager = RetryManager::new(config);
            let attempt_counter = Arc::new(AtomicU32::new(0));
            let counter_clone = attempt_counter.clone();
            let target_k = k;

            let (result, _elapsed) = rt.block_on(async {
                let start = time::Instant::now();

                let operation_future = manager.invocation(
                    "kth_attempt_test",
                    move || {
                        let counter = counter_clone.clone();
                        async move {
                            let attempt = counter.fetch_add(1, Ordering::SeqCst) as usize;
                            if attempt + 1 == target_k {
                                Ok(42)
                            } else {
                                Err(TestError::Retryable("retry".to_string()))
                            }
                        }
                    },
                    |e: &TestError| matches!(e, TestError::Retryable(_)),
                    create_test_error,
                ).execute();

                // Advance time to allow enough retries
                for _ in 0..k {
                    advance_clock(Duration::from_millis(initial_delay_ms * 4)).await;
                }

                let result = operation_future.await;
                let elapsed = start.elapsed();

                (result, elapsed)
            });

            let attempts = attempt_counter.load(Ordering::SeqCst) as usize;

            // Using paused Tokio time (start_paused + advance); assert behavior only (no wall-clock timing)
            prop_assert!(result.is_ok());
            prop_assert_eq!(result.unwrap(), 42);
            prop_assert_eq!(attempts, k);
        }
    }
}
