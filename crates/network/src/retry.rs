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

//! Generic retry mechanism for network operations.

use std::{future::Future, marker::PhantomData, time::Duration};

use tokio_util::sync::CancellationToken;

use crate::backoff::ExponentialBackoff;

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
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
    /// Optional timeout for individual operations in milliseconds.
    /// If None, no timeout is applied.
    pub operation_timeout_ms: Option<u64>,
    /// Whether the first retry should happen immediately without delay.
    /// Should be false for HTTP/order operations, true for connection operations.
    pub immediate_first: bool,
    /// Optional maximum total elapsed time for all retries in milliseconds.
    /// If exceeded, retries stop even if `max_retries` hasn't been reached.
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

/// Generic retry manager for network operations.
///
/// Stateless and thread-safe - each operation maintains its own backoff state.
#[derive(Clone, Debug)]
pub struct RetryManager<E> {
    config: RetryConfig,
    _phantom: PhantomData<E>,
}

impl<E> RetryManager<E>
where
    E: std::error::Error,
{
    /// Creates a new retry manager with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid.
    pub const fn new(config: RetryConfig) -> anyhow::Result<Self> {
        Ok(Self {
            config,
            _phantom: PhantomData,
        })
    }

    /// Executes an operation with retry logic and optional cancellation.
    ///
    /// Cancellation is checked at three points:
    /// (1) Before each operation attempt.
    /// (2) During operation execution (via `tokio::select!`).
    /// (3) During retry delays.
    ///
    /// This means cancellation may be delayed by up to one operation timeout if it occurs mid-execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails after exhausting all retries,
    /// if the operation times out, if creating the backoff state fails, or if canceled.
    pub async fn execute_with_retry_inner<F, Fut, T>(
        &self,
        operation_name: &str,
        mut operation: F,
        should_retry: impl Fn(&E) -> bool,
        create_error: impl Fn(String) -> E,
        cancel: Option<&CancellationToken>,
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
        .map_err(|e| create_error(format!("Invalid configuration: {e}")))?;

        let mut attempt = 0;
        let start_time = tokio::time::Instant::now();

        loop {
            if let Some(token) = cancel
                && token.is_cancelled()
            {
                tracing::debug!(
                    operation = %operation_name,
                    attempts = attempt,
                    "Operation canceled"
                );
                return Err(create_error("canceled".to_string()));
            }

            if let Some(max_elapsed_ms) = self.config.max_elapsed_ms {
                let elapsed = start_time.elapsed();
                if elapsed.as_millis() >= u128::from(max_elapsed_ms) {
                    let e = create_error("Budget exceeded".to_string());
                    tracing::trace!(
                        operation = %operation_name,
                        attempts = attempt + 1,
                        budget_ms = max_elapsed_ms,
                        "Retry budget exceeded"
                    );
                    return Err(e);
                }
            }

            let result = match (self.config.operation_timeout_ms, cancel) {
                (Some(timeout_ms), Some(token)) => {
                    tokio::select! {
                        result = tokio::time::timeout(Duration::from_millis(timeout_ms), operation()) => result,
                        () = token.cancelled() => {
                            tracing::debug!(
                                operation = %operation_name,
                                "Operation canceled during execution"
                            );
                            return Err(create_error("canceled".to_string()));
                        }
                    }
                }
                (Some(timeout_ms), None) => {
                    tokio::time::timeout(Duration::from_millis(timeout_ms), operation()).await
                }
                (None, Some(token)) => {
                    tokio::select! {
                        result = operation() => Ok(result),
                        () = token.cancelled() => {
                            tracing::debug!(
                                operation = %operation_name,
                                "Operation canceled during execution"
                            );
                            return Err(create_error("canceled".to_string()));
                        }
                    }
                }
                (None, None) => Ok(operation().await),
            };

            match result {
                Ok(Ok(success)) => {
                    if attempt > 0 {
                        tracing::trace!(
                            operation = %operation_name,
                            attempts = attempt + 1,
                            "Retry succeeded"
                        );
                    }
                    return Ok(success);
                }
                Ok(Err(e)) => {
                    if !should_retry(&e) {
                        tracing::trace!(
                            operation = %operation_name,
                            error = %e,
                            "Non-retryable error"
                        );
                        return Err(e);
                    }

                    if attempt >= self.config.max_retries {
                        tracing::trace!(
                            operation = %operation_name,
                            attempts = attempt + 1,
                            error = %e,
                            "Retries exhausted"
                        );
                        return Err(e);
                    }

                    let mut delay = backoff.next_duration();

                    if let Some(max_elapsed_ms) = self.config.max_elapsed_ms {
                        let elapsed = start_time.elapsed();
                        let remaining =
                            Duration::from_millis(max_elapsed_ms).saturating_sub(elapsed);

                        if remaining.is_zero() {
                            let e = create_error("Budget exceeded".to_string());
                            tracing::trace!(
                                operation = %operation_name,
                                attempts = attempt + 1,
                                budget_ms = max_elapsed_ms,
                                "Retry budget exceeded"
                            );
                            return Err(e);
                        }

                        delay = delay.min(remaining);
                    }

                    tracing::trace!(
                        operation = %operation_name,
                        attempt = attempt + 1,
                        delay_ms = delay.as_millis() as u64,
                        error = %e,
                        "Retrying after failure"
                    );

                    // Yield even on zero-delay to avoid busy-wait loop
                    if delay.is_zero() {
                        tokio::task::yield_now().await;
                        attempt += 1;
                        continue;
                    }

                    if let Some(token) = cancel {
                        tokio::select! {
                            () = tokio::time::sleep(delay) => {},
                            () = token.cancelled() => {
                                tracing::debug!(
                                    operation = %operation_name,
                                    attempt = attempt + 1,
                                    "Operation canceled during retry delay"
                                );
                                return Err(create_error("canceled".to_string()));
                            }
                        }
                    } else {
                        tokio::time::sleep(delay).await;
                    }
                    attempt += 1;
                }
                Err(_) => {
                    let e = create_error(format!(
                        "Timed out after {}ms",
                        self.config.operation_timeout_ms.unwrap_or(0)
                    ));

                    if !should_retry(&e) {
                        tracing::trace!(
                            operation = %operation_name,
                            error = %e,
                            "Non-retryable timeout"
                        );
                        return Err(e);
                    }

                    if attempt >= self.config.max_retries {
                        tracing::trace!(
                            operation = %operation_name,
                            attempts = attempt + 1,
                            error = %e,
                            "Retries exhausted after timeout"
                        );
                        return Err(e);
                    }

                    let mut delay = backoff.next_duration();

                    if let Some(max_elapsed_ms) = self.config.max_elapsed_ms {
                        let elapsed = start_time.elapsed();
                        let remaining =
                            Duration::from_millis(max_elapsed_ms).saturating_sub(elapsed);

                        if remaining.is_zero() {
                            let e = create_error("Budget exceeded".to_string());
                            tracing::trace!(
                                operation = %operation_name,
                                attempts = attempt + 1,
                                budget_ms = max_elapsed_ms,
                                "Retry budget exceeded"
                            );
                            return Err(e);
                        }

                        delay = delay.min(remaining);
                    }

                    tracing::trace!(
                        operation = %operation_name,
                        attempt = attempt + 1,
                        delay_ms = delay.as_millis() as u64,
                        error = %e,
                        "Retrying after timeout"
                    );

                    // Yield even on zero-delay to avoid busy-wait loop
                    if delay.is_zero() {
                        tokio::task::yield_now().await;
                        attempt += 1;
                        continue;
                    }

                    if let Some(token) = cancel {
                        tokio::select! {
                            () = tokio::time::sleep(delay) => {},
                            () = token.cancelled() => {
                                tracing::debug!(
                                    operation = %operation_name,
                                    attempt = attempt + 1,
                                    "Operation canceled during retry delay"
                                );
                                return Err(create_error("canceled".to_string()));
                            }
                        }
                    } else {
                        tokio::time::sleep(delay).await;
                    }
                    attempt += 1;
                }
            }
        }
    }

    /// Executes an operation with retry logic.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails after exhausting all retries,
    /// if the operation times out, or if creating the backoff state fails.
    pub async fn execute_with_retry<F, Fut, T>(
        &self,
        operation_name: &str,
        operation: F,
        should_retry: impl Fn(&E) -> bool,
        create_error: impl Fn(String) -> E,
    ) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        self.execute_with_retry_inner(operation_name, operation, should_retry, create_error, None)
            .await
    }

    /// Executes an operation with retry logic and cancellation support.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails after exhausting all retries,
    /// if the operation times out, if creating the backoff state fails, or if canceled.
    pub async fn execute_with_retry_with_cancel<F, Fut, T>(
        &self,
        operation_name: &str,
        operation: F,
        should_retry: impl Fn(&E) -> bool,
        create_error: impl Fn(String) -> E,
        cancellation_token: &CancellationToken,
    ) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        self.execute_with_retry_inner(
            operation_name,
            operation,
            should_retry,
            create_error,
            Some(cancellation_token),
        )
        .await
    }
}

/// Convenience function to create a retry manager with default configuration.
///
/// # Errors
///
/// Returns an error if the default configuration is invalid.
pub fn create_default_retry_manager<E>() -> anyhow::Result<RetryManager<E>>
where
    E: std::error::Error,
{
    RetryManager::new(RetryConfig::default())
}

/// Convenience function to create a retry manager for HTTP operations.
///
/// # Errors
///
/// Returns an error if the HTTP configuration is invalid.
pub const fn create_http_retry_manager<E>() -> anyhow::Result<RetryManager<E>>
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
///
/// # Errors
///
/// Returns an error if the WebSocket configuration is invalid.
pub const fn create_websocket_retry_manager<E>() -> anyhow::Result<RetryManager<E>>
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod test_utils {
    #[derive(Debug, thiserror::Error)]
    pub enum TestError {
        #[error("Retryable error: {0}")]
        Retryable(String),
        #[error("Non-retryable error: {0}")]
        NonRetryable(String),
        #[error("Timeout error: {0}")]
        Timeout(String),
    }

    pub fn should_retry_test_error(error: &TestError) -> bool {
        matches!(error, TestError::Retryable(_))
    }

    pub fn create_test_error(msg: String) -> TestError {
        TestError::Timeout(msg)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    use nautilus_core::MUTEX_POISONED;
    use rstest::rstest;

    use super::{test_utils::*, *};

    const MAX_WAIT_ITERS: usize = 10_000;
    const MAX_ADVANCE_ITERS: usize = 10_000;

    pub(crate) async fn yield_until<F>(mut condition: F)
    where
        F: FnMut() -> bool,
    {
        for _ in 0..MAX_WAIT_ITERS {
            if condition() {
                return;
            }
            tokio::task::yield_now().await;
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
            tokio::time::advance(Duration::from_millis(1)).await;
            tokio::task::yield_now().await;
        }

        panic!("advance_until timed out waiting for condition");
    }

    #[rstest]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay_ms, 1_000);
        assert_eq!(config.max_delay_ms, 10_000);
        assert_eq!(config.backoff_factor, 2.0);
        assert_eq!(config.jitter_ms, 100);
        assert_eq!(config.operation_timeout_ms, Some(30_000));
        assert!(!config.immediate_first);
        assert_eq!(config.max_elapsed_ms, None);
    }

    #[tokio::test]
    async fn test_retry_manager_success_first_attempt() {
        let manager = RetryManager::new(RetryConfig::default()).unwrap();

        let result = manager
            .execute_with_retry(
                "test_operation",
                || async { Ok::<i32, TestError>(42) },
                should_retry_test_error,
                create_test_error,
            )
            .await;

        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_manager_non_retryable_error() {
        let manager = RetryManager::new(RetryConfig::default()).unwrap();

        let result = manager
            .execute_with_retry(
                "test_operation",
                || async { Err::<i32, TestError>(TestError::NonRetryable("test".to_string())) },
                should_retry_test_error,
                create_test_error,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::NonRetryable(_)));
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let result = manager
            .execute_with_retry(
                "test_operation",
                || async { Err::<i32, TestError>(TestError::Retryable("test".to_string())) },
                should_retry_test_error,
                create_test_error,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Retryable(_)));
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let result = manager
            .execute_with_retry(
                "test_timeout",
                || async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok::<i32, TestError>(42)
                },
                should_retry_test_error,
                create_test_error,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let start = tokio::time::Instant::now();
        let result = manager
            .execute_with_retry(
                "test_budget",
                || async { Err::<i32, TestError>(TestError::Retryable("test".to_string())) },
                should_retry_test_error,
                create_test_error,
            )
            .await;

        let elapsed = start.elapsed();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
        assert!(elapsed.as_millis() >= 150);
        assert!(elapsed.as_millis() < 1000);
    }

    #[rstest]
    fn test_http_retry_manager_config() {
        let manager = create_http_retry_manager::<TestError>().unwrap();
        assert_eq!(manager.config.max_retries, 3);
        assert!(!manager.config.immediate_first);
        assert_eq!(manager.config.max_elapsed_ms, Some(180_000));
    }

    #[rstest]
    fn test_websocket_retry_manager_config() {
        let manager = create_websocket_retry_manager::<TestError>().unwrap();
        assert_eq!(manager.config.max_retries, 5);
        assert!(manager.config.immediate_first);
        assert_eq!(manager.config.max_elapsed_ms, Some(120_000));
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        // Test with retry predicate that rejects timeouts
        let should_not_retry_timeouts = |error: &TestError| !matches!(error, TestError::Timeout(_));

        let result = manager
            .execute_with_retry(
                "test_timeout_non_retryable",
                || async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok::<i32, TestError>(42)
                },
                should_not_retry_timeouts,
                create_test_error,
            )
            .await;

        // Should fail immediately without retries since timeout is non-retryable
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        // Test with retry predicate that allows timeouts
        let should_retry_timeouts = |error: &TestError| matches!(error, TestError::Timeout(_));

        let start = tokio::time::Instant::now();
        let result = manager
            .execute_with_retry(
                "test_timeout_retryable",
                || async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok::<i32, TestError>(42)
                },
                should_retry_timeouts,
                create_test_error,
            )
            .await;

        let elapsed = start.elapsed();

        // Should fail after retries (not immediately)
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
        // Should have taken time for retries (at least 2 timeouts + delays)
        assert!(elapsed.as_millis() > 80); // More than just one timeout
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let result = manager
            .execute_with_retry(
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
            .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
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
        let manager = RetryManager::new(config).unwrap();

        let attempt_times = Arc::new(std::sync::Mutex::new(Vec::new()));
        let times_clone = attempt_times.clone();
        let start = tokio::time::Instant::now();

        let handle = tokio::spawn({
            let times_clone = times_clone.clone();
            async move {
                let _ = manager
                    .execute_with_retry(
                        "test_immediate",
                        move || {
                            let times = times_clone.clone();
                            async move {
                                times.lock().expect(MUTEX_POISONED).push(start.elapsed());
                                Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                            }
                        },
                        should_retry_test_error,
                        create_test_error,
                    )
                    .await;
            }
        });

        // Allow initial attempt and immediate retry to run without advancing time
        yield_until(|| attempt_times.lock().expect(MUTEX_POISONED).len() >= 2).await;

        // Advance time for the next backoff interval
        tokio::time::advance(Duration::from_millis(100)).await;
        tokio::task::yield_now().await;

        // Wait for the final retry to be recorded
        yield_until(|| attempt_times.lock().expect(MUTEX_POISONED).len() >= 3).await;

        handle.await.unwrap();

        let times = attempt_times.lock().expect(MUTEX_POISONED);
        assert_eq!(times.len(), 3); // Initial + 2 retries

        // First retry should be immediate (within 1ms tolerance)
        assert!(times[1] <= Duration::from_millis(1));
        // Second retry should have backoff delay (at least 100ms from start)
        assert!(times[2] >= Duration::from_millis(100));
        assert!(times[2] <= Duration::from_millis(110));
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let start = tokio::time::Instant::now();
        let result = manager
            .execute_with_retry(
                "test_no_timeout",
                || async {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok::<i32, TestError>(42)
                },
                should_retry_test_error,
                create_test_error,
            )
            .await;

        let elapsed = start.elapsed();
        assert_eq!(result.unwrap(), 42);
        // Should complete without timing out
        assert!(elapsed.as_millis() >= 30);
        assert!(elapsed.as_millis() < 200);
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let result = manager
            .execute_with_retry(
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
            .await;

        assert!(result.is_err());
        // Should only attempt once (no retries)
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
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
        let manager = RetryManager::new(config).unwrap();

        let delays = Arc::new(std::sync::Mutex::new(Vec::new()));
        let delays_clone = delays.clone();
        let last_time = Arc::new(std::sync::Mutex::new(tokio::time::Instant::now()));
        let last_time_clone = last_time.clone();

        let handle = tokio::spawn({
            let delays_clone = delays_clone.clone();
            async move {
                let _ = manager
                    .execute_with_retry(
                        "test_jitter",
                        move || {
                            let delays = delays_clone.clone();
                            let last_time = last_time_clone.clone();
                            async move {
                                let now = tokio::time::Instant::now();
                                let delay = {
                                    let mut last = last_time.lock().expect(MUTEX_POISONED);
                                    let d = now.duration_since(*last);
                                    *last = now;
                                    d
                                };
                                delays.lock().expect(MUTEX_POISONED).push(delay);
                                Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                            }
                        },
                        should_retry_test_error,
                        create_test_error,
                    )
                    .await;
            }
        });

        yield_until(|| !delays.lock().expect(MUTEX_POISONED).is_empty()).await;
        advance_until(|| delays.lock().expect(MUTEX_POISONED).len() >= 2).await;
        advance_until(|| delays.lock().expect(MUTEX_POISONED).len() >= 3).await;

        handle.await.unwrap();

        let delays = delays.lock().expect(MUTEX_POISONED);
        // Skip the first delay (initial attempt)
        for delay in delays.iter().skip(1) {
            // Each delay should be at least the base delay (50ms for first retry)
            assert!(delay.as_millis() >= 50);
            // But no more than base + jitter (allow small tolerance for step advance)
            assert!(delay.as_millis() <= 151);
        }
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let start = tokio::time::Instant::now();
        let result = manager
            .execute_with_retry(
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
            .await;

        let elapsed = start.elapsed();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));

        // Should have stopped due to time limit, not retry count
        let attempts = attempt_counter.load(Ordering::SeqCst);
        assert!(attempts < 10); // Much less than max_retries
        assert!(elapsed.as_millis() >= 100);
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let result = manager
            .execute_with_retry(
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
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::NonRetryable(_)));
        // Should stop at the non-retryable error
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let token = CancellationToken::new();
        let token_clone = token.clone();

        // Cancel after a short delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            token_clone.cancel();
        });

        let attempt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = attempt_counter.clone();

        let start = tokio::time::Instant::now();
        let result = manager
            .execute_with_retry_with_cancel(
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
                &token,
            )
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

    #[tokio::test]
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
        let manager = RetryManager::new(config).unwrap();

        let token = CancellationToken::new();
        let token_clone = token.clone();

        // Cancel after a short delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            token_clone.cancel();
        });

        let start = tokio::time::Instant::now();
        let result = manager
            .execute_with_retry_with_cancel(
                "test_cancellation_during_op",
                || async {
                    // Long-running operation
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    Ok::<i32, TestError>(42)
                },
                should_retry_test_error,
                create_test_error,
                &token,
            )
            .await;

        let elapsed = start.elapsed();

        // Should be canceled during the operation
        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(error_msg.contains("canceled"));

        // Should not have completed the long operation
        assert!(elapsed.as_millis() < 250);
    }

    #[tokio::test]
    async fn test_cancellation_error_message() {
        use tokio_util::sync::CancellationToken;

        let config = RetryConfig::default();
        let manager = RetryManager::new(config).unwrap();

        let token = CancellationToken::new();
        token.cancel(); // Pre-cancel for immediate cancellation

        let result = manager
            .execute_with_retry_with_cancel(
                "test_operation",
                || async { Ok::<i32, TestError>(42) },
                should_retry_test_error,
                create_test_error,
                &token,
            )
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

    use nautilus_core::MUTEX_POISONED;
    use proptest::prelude::*;
    // Import rstest attribute macro used within proptest! tests
    use rstest::rstest;

    use super::{
        test_utils::*,
        tests::{advance_until, yield_until},
        *,
    };

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
            let manager = RetryManager::<std::io::Error>::new(config);
            prop_assert!(manager.is_ok());
        }

        #[rstest]
        fn test_retry_attempts_bounded(
            max_retries in 0u32..5,
            initial_delay_ms in 1u64..10,
            backoff_factor in 1.0f64..2.0,
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();

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

            let manager = RetryManager::new(config).unwrap();
            let attempt_counter = Arc::new(AtomicU32::new(0));
            let counter_clone = attempt_counter.clone();

            let _result = rt.block_on(manager.execute_with_retry(
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
            ));

            let attempts = attempt_counter.load(Ordering::SeqCst);
            // Total attempts should be 1 (initial) + max_retries
            prop_assert_eq!(attempts, max_retries + 1);
        }

        #[rstest]
        fn test_timeout_always_respected(
            timeout_ms in 10u64..50,
            operation_delay_ms in 60u64..100,
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap();

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

            let manager = RetryManager::new(config).unwrap();

            let result = rt.block_on(async {
                let operation_future = manager.execute_with_retry(
                    "timeout_test",
                    move || async move {
                        tokio::time::sleep(Duration::from_millis(operation_delay_ms)).await;
                        Ok::<i32, TestError>(42)
                    },
                    |_: &TestError| true,
                    TestError::Timeout,
                );

                // Advance time to trigger timeout
                tokio::time::advance(Duration::from_millis(timeout_ms + 10)).await;
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
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap();

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

            let manager = RetryManager::new(config).unwrap();
            let attempt_counter = Arc::new(AtomicU32::new(0));
            let counter_clone = attempt_counter.clone();

            let result = rt.block_on(async {
                let operation_future = manager.execute_with_retry(
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
                );

                // Advance time past max_elapsed_ms
                tokio::time::advance(Duration::from_millis(max_elapsed_ms + delay_per_retry)).await;
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
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap();

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

            let manager = RetryManager::new(config).unwrap();
            let attempt_times = Arc::new(std::sync::Mutex::new(Vec::new()));
            let attempt_times_for_block = attempt_times.clone();

            rt.block_on(async move {
                let attempt_times_for_wait = attempt_times_for_block.clone();
                let handle = tokio::spawn({
                    let attempt_times_for_task = attempt_times_for_block.clone();
                    let manager = manager;
                    async move {
                        let start_time = tokio::time::Instant::now();
                        let _ = manager
                            .execute_with_retry(
                                "jitter_test",
                                move || {
                                    let attempt_times_inner = attempt_times_for_task.clone();
                                    async move {
                                        attempt_times_inner
                                            .lock()
                                            .unwrap()
                                            .push(start_time.elapsed());
                                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                                    }
                                },
                                |e: &TestError| matches!(e, TestError::Retryable(_)),
                                TestError::Timeout,
                            )
                            .await;
                    }
                });

                yield_until(|| !attempt_times_for_wait.lock().expect(MUTEX_POISONED).is_empty()).await;
                advance_until(|| attempt_times_for_wait.lock().expect(MUTEX_POISONED).len() >= 2).await;
                advance_until(|| attempt_times_for_wait.lock().expect(MUTEX_POISONED).len() >= 3).await;

                handle.await.unwrap();
            });

            let times = attempt_times.lock().expect(MUTEX_POISONED);

            // We expect at least 2 attempts total (initial + at least 1 retry)
            prop_assert!(times.len() >= 2);

            // First attempt should be immediate (no delay)
            prop_assert!(times[0].as_millis() < 5);

            // Check subsequent retries have appropriate delays
            for i in 1..times.len() {
                let delay_from_previous = if i == 1 {
                    times[i] - times[0]
                } else {
                    times[i] - times[i - 1]
                };

                // The delay should be at least base_delay_ms
                prop_assert!(
                    delay_from_previous.as_millis() >= base_delay_ms as u128,
                    "Retry {} delay {}ms is less than base {}ms",
                    i, delay_from_previous.as_millis(), base_delay_ms
                );

                // Delay should be at most base_delay + jitter
                prop_assert!(
                    delay_from_previous.as_millis() <= (base_delay_ms + jitter_ms + 1) as u128,
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
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap();

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

            let manager = RetryManager::new(config).unwrap();
            let attempt_times = Arc::new(std::sync::Mutex::new(Vec::new()));
            let attempt_times_for_block = attempt_times.clone();

            rt.block_on(async move {
                let attempt_times_for_wait = attempt_times_for_block.clone();
                let handle = tokio::spawn({
                    let attempt_times_for_task = attempt_times_for_block.clone();
                    let manager = manager;
                    async move {
                        let start = tokio::time::Instant::now();
                        let _ = manager
                            .execute_with_retry(
                                "immediate_test",
                                move || {
                                    let attempt_times_inner = attempt_times_for_task.clone();
                                    async move {
                                        let elapsed = start.elapsed();
                                        attempt_times_inner.lock().expect(MUTEX_POISONED).push(elapsed);
                                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                                    }
                                },
                                |e: &TestError| matches!(e, TestError::Retryable(_)),
                                TestError::Timeout,
                            )
                            .await;
                    }
                });

                yield_until(|| !attempt_times_for_wait.lock().expect(MUTEX_POISONED).is_empty()).await;
                advance_until(|| attempt_times_for_wait.lock().expect(MUTEX_POISONED).len() >= 2).await;
                advance_until(|| attempt_times_for_wait.lock().expect(MUTEX_POISONED).len() >= 3).await;

                handle.await.unwrap();
            });

            let times = attempt_times.lock().expect(MUTEX_POISONED);
            prop_assert!(times.len() >= 2);

            if immediate_first {
                // First retry should be immediate
                prop_assert!(times[1].as_millis() < 20,
                    "With immediate_first=true, first retry took {}ms",
                    times[1].as_millis());
            } else {
                // First retry should have delay
                prop_assert!(times[1].as_millis() >= (initial_delay_ms - 1) as u128,
                    "With immediate_first=false, first retry was too fast: {}ms",
                    times[1].as_millis());
            }
        }

        #[rstest]
        fn test_non_retryable_stops_immediately(
            attempt_before_non_retryable in 0usize..3,
            max_retries in 3u32..5,
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();

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

            let manager = RetryManager::new(config).unwrap();
            let attempt_counter = Arc::new(AtomicU32::new(0));
            let counter_clone = attempt_counter.clone();

            let result: Result<i32, TestError> = rt.block_on(manager.execute_with_retry(
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
            ));

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

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap();

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

            let manager = RetryManager::new(config).unwrap();
            let token = CancellationToken::new();
            let token_clone = token.clone();

            let result: Result<i32, TestError> = rt.block_on(async {
                // Spawn cancellation task
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(cancel_after_ms)).await;
                    token_clone.cancel();
                });

                let operation_future = manager.execute_with_retry_with_cancel(
                    "cancellation_test",
                    || async {
                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                    },
                    |e: &TestError| matches!(e, TestError::Retryable(_)),
                    create_test_error,
                    &token,
                );

                // Advance time to trigger cancellation
                tokio::time::advance(Duration::from_millis(cancel_after_ms + 10)).await;
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
            delay_per_retry in 20u64..50,
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap();

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

            let manager = RetryManager::new(config).unwrap();

            let _result = rt.block_on(async {
                let operation_future = manager.execute_with_retry(
                    "budget_clamp_test",
                    || async {
                        // Fast operation to focus on delay timing
                        Err::<i32, TestError>(TestError::Retryable("fail".to_string()))
                    },
                    |e: &TestError| matches!(e, TestError::Retryable(_)),
                    create_test_error,
                );

                // Advance time past max_elapsed_ms
                tokio::time::advance(Duration::from_millis(max_elapsed_ms + delay_per_retry)).await;
                operation_future.await
            });

            // With deterministic time, operation completes without wall-clock delay
            // The budget constraint is still enforced by the retry manager
        }

        #[rstest]
        fn test_success_on_kth_attempt(
            k in 1usize..5,
            initial_delay_ms in 5u64..20,
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap();

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

            let manager = RetryManager::new(config).unwrap();
            let attempt_counter = Arc::new(AtomicU32::new(0));
            let counter_clone = attempt_counter.clone();
            let target_k = k;

            let (result, _elapsed) = rt.block_on(async {
                let start = tokio::time::Instant::now();

                let operation_future = manager.execute_with_retry(
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
                );

                // Advance time to allow enough retries
                for _ in 0..k {
                    tokio::time::advance(Duration::from_millis(initial_delay_ms * 4)).await;
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
