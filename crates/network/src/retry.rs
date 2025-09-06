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

use tokio::time::sleep;
use tracing::{debug, warn};

use crate::backoff::ExponentialBackoff;

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (total attempts = 1 initial + max_retries).
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
    /// If exceeded, retries stop even if max_retries hasn't been reached.
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
/// This is stateless and thread-safe - each operation gets its own backoff state.
#[derive(Debug)]
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
    /// This function will return an error if the configuration is invalid.
    pub fn new(config: RetryConfig) -> anyhow::Result<Self> {
        Ok(Self {
            config,
            _phantom: PhantomData,
        })
    }

    /// Executes an operation with retry logic.
    ///
    /// # Errors
    ///
    /// This function will return an error if the operation fails after exhausting all retries,
    /// if the operation times out, or if creating the backoff state fails.
    pub async fn execute_with_retry<F, Fut, T>(
        &self,
        operation_name: &str,
        mut operation: F,
        should_retry: impl Fn(&E) -> bool,
        create_timeout_error: impl Fn(String) -> E,
    ) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        // Create a fresh backoff state for this operation
        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(self.config.initial_delay_ms),
            Duration::from_millis(self.config.max_delay_ms),
            self.config.backoff_factor,
            self.config.jitter_ms,
            self.config.immediate_first,
        )
        .map_err(|e| create_timeout_error(format!("Failed to create backoff: {e}")))?;

        let mut attempt = 0;
        let start_time = tokio::time::Instant::now();

        loop {
            // Check if we've exceeded the total elapsed time budget
            if let Some(max_elapsed_ms) = self.config.max_elapsed_ms {
                let elapsed = start_time.elapsed();
                if elapsed.as_millis() > max_elapsed_ms as u128 {
                    let timeout_error = create_timeout_error(format!(
                        "Operation '{}' exceeded total time budget of {}ms",
                        operation_name, max_elapsed_ms
                    ));
                    warn!(
                        "Operation '{}' exceeded total time budget after {} attempts",
                        operation_name,
                        attempt + 1
                    );
                    return Err(timeout_error);
                }
            }
            // Execute the operation with optional timeout
            let result = if let Some(timeout_ms) = self.config.operation_timeout_ms {
                tokio::time::timeout(Duration::from_millis(timeout_ms), operation()).await
            } else {
                Ok(operation().await)
            };

            match result {
                Ok(Ok(success)) => {
                    if attempt > 0 {
                        debug!(
                            "Operation '{}' succeeded after {} attempts",
                            operation_name,
                            attempt + 1
                        );
                    }
                    return Ok(success);
                }
                Ok(Err(error)) => {
                    // Check if we should retry this error
                    if !should_retry(&error) {
                        debug!(
                            "Operation '{}' failed with non-retryable error: {}",
                            operation_name, error
                        );
                        return Err(error);
                    }

                    // Check if we've exhausted retries
                    if attempt >= self.config.max_retries {
                        warn!(
                            "Operation '{}' failed after {} attempts: {}",
                            operation_name,
                            attempt + 1,
                            error
                        );
                        return Err(error);
                    }

                    // Calculate delay and wait
                    let delay = backoff.next_duration();
                    debug!(
                        "Operation '{}' failed (attempt {}), retrying in {:?}: {}",
                        operation_name,
                        attempt + 1,
                        delay,
                        error
                    );

                    sleep(delay).await;
                    attempt += 1;
                }
                Err(_timeout) => {
                    let timeout_error = create_timeout_error(format!(
                        "Operation '{}' timed out after {}ms",
                        operation_name,
                        self.config.operation_timeout_ms.unwrap_or(0)
                    ));

                    // Check if we should retry this timeout error
                    if !should_retry(&timeout_error) {
                        debug!(
                            "Operation '{}' timed out with non-retryable timeout: {}",
                            operation_name, timeout_error
                        );
                        return Err(timeout_error);
                    }

                    // Check if we've exhausted retries
                    if attempt >= self.config.max_retries {
                        warn!(
                            "Operation '{}' timed out after {} attempts: {}",
                            operation_name,
                            attempt + 1,
                            timeout_error
                        );
                        return Err(timeout_error);
                    }

                    // Calculate delay and wait
                    let delay = backoff.next_duration();
                    debug!(
                        "Operation '{}' timed out (attempt {}), retrying in {:?}: {}",
                        operation_name,
                        attempt + 1,
                        delay,
                        timeout_error
                    );

                    sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }
}

/// Convenience function to create a retry manager with default configuration.
///
/// # Errors
///
/// This function will return an error if the default configuration is invalid.
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
/// This function will return an error if the HTTP configuration is invalid.
pub fn create_http_retry_manager<E>() -> anyhow::Result<RetryManager<E>>
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
/// This function will return an error if the WebSocket configuration is invalid.
pub fn create_websocket_retry_manager<E>() -> anyhow::Result<RetryManager<E>>
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
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    enum TestError {
        #[error("Retryable error: {0}")]
        Retryable(String),
        #[error("Non-retryable error: {0}")]
        NonRetryable(String),
        #[error("Timeout error: {0}")]
        Timeout(String),
    }

    fn should_retry_test_error(error: &TestError) -> bool {
        matches!(error, TestError::Retryable(_))
    }

    fn create_timeout_error(msg: String) -> TestError {
        TestError::Timeout(msg)
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay_ms, 1_000);
        assert_eq!(config.max_delay_ms, 10_000);
        assert_eq!(config.backoff_factor, 2.0);
        assert_eq!(config.jitter_ms, 100);
        assert_eq!(config.operation_timeout_ms, Some(30_000));
        assert!(!config.immediate_first); // Default to Python behavior
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
                create_timeout_error,
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
                create_timeout_error,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::NonRetryable(_)));
    }

    #[tokio::test]
    async fn test_retry_manager_retryable_error_exhausted() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 10, // Fast for testing
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 0, // No jitter for predictable testing
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
                create_timeout_error,
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
            operation_timeout_ms: Some(50), // Very short timeout
            immediate_first: false,
            max_elapsed_ms: None,
        };
        let manager = RetryManager::new(config).unwrap();

        let result = manager
            .execute_with_retry(
                "test_timeout",
                || async {
                    tokio::time::sleep(Duration::from_millis(100)).await; // Longer than timeout
                    Ok::<i32, TestError>(42)
                },
                should_retry_test_error,
                create_timeout_error,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
    }

    #[tokio::test]
    async fn test_max_elapsed_time_budget() {
        let config = RetryConfig {
            max_retries: 10, // High retry count
            initial_delay_ms: 50,
            max_delay_ms: 100,
            backoff_factor: 2.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: Some(200), // Short total budget
        };
        let manager = RetryManager::new(config).unwrap();

        let start = tokio::time::Instant::now();
        let result = manager
            .execute_with_retry(
                "test_budget",
                || async { Err::<i32, TestError>(TestError::Retryable("test".to_string())) },
                should_retry_test_error,
                create_timeout_error,
            )
            .await;

        let elapsed = start.elapsed();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
        // Should have stopped due to time budget, not retry count
        assert!(elapsed.as_millis() >= 200);
        assert!(elapsed.as_millis() < 500); // Shouldn't take too long
    }

    #[test]
    fn test_http_retry_manager_config() {
        let manager = create_http_retry_manager::<TestError>().unwrap();
        assert_eq!(manager.config.max_retries, 3);
        assert!(!manager.config.immediate_first); // HTTP should not retry immediately
        assert_eq!(manager.config.max_elapsed_ms, Some(180_000));
    }

    #[test]
    fn test_websocket_retry_manager_config() {
        let manager = create_websocket_retry_manager::<TestError>().unwrap();
        assert_eq!(manager.config.max_retries, 5);
        assert!(manager.config.immediate_first); // WebSocket should retry immediately
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
            operation_timeout_ms: Some(50), // Very short timeout
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
                    tokio::time::sleep(Duration::from_millis(100)).await; // Longer than timeout
                    Ok::<i32, TestError>(42)
                },
                should_not_retry_timeouts,
                create_timeout_error,
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
            operation_timeout_ms: Some(50), // Very short timeout
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
                    tokio::time::sleep(Duration::from_millis(100)).await; // Longer than timeout
                    Ok::<i32, TestError>(42)
                },
                should_retry_timeouts,
                create_timeout_error,
            )
            .await;

        let elapsed = start.elapsed();

        // Should fail after retries (not immediately)
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TestError::Timeout(_)));
        // Should have taken time for retries (at least 2 timeouts + delays)
        assert!(elapsed.as_millis() > 100); // More than just one timeout
    }
}
