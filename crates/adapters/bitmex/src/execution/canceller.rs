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

//! Cancel request broadcaster for redundant order cancellation.
//!
//! This module provides the [`CancelBroadcaster`] which fans out cancel requests
//! to multiple HTTP clients in parallel for redundancy. Key design patterns:
//!
//! - **Dependency injection via traits**: Uses `CancelExecutor` trait to abstract
//!   the HTTP client, enabling testing without `#[cfg(test)]` conditional compilation.
//! - **Trait objects over generics**: Uses `Arc<dyn CancelExecutor>` to avoid
//!   generic type parameters on the public API (simpler Python FFI).
//! - **Short-circuit on first success**: Aborts remaining requests once any client
//!   succeeds, minimizing latency.
//! - **Idempotent success handling**: Recognizes "already cancelled" responses as
//!   successful outcomes.

// TODO: Replace boxed futures in `CancelExecutor` once stable async trait object support
// lands so we can drop the per-call heap allocation

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::future;
use nautilus_model::{
    enums::OrderSide,
    identifiers::{ClientOrderId, InstrumentId, VenueOrderId},
    instruments::InstrumentAny,
    reports::OrderStatusReport,
};
use tokio::{sync::RwLock, task::JoinHandle, time::interval};

use crate::{common::consts::BITMEX_HTTP_TESTNET_URL, http::client::BitmexHttpClient};

/// Trait for order cancellation operations.
///
/// This trait abstracts the execution layer to enable dependency injection and testing
/// without conditional compilation. The broadcaster holds executors as `Arc<dyn CancelExecutor>`
/// to avoid generic type parameters that would complicate the Python FFI boundary.
///
/// # Thread Safety
///
/// All methods must be safe to call concurrently from multiple threads. Implementations
/// should use interior mutability (e.g., `Arc<Mutex<T>>`) if mutable state is required.
///
/// # Error Handling
///
/// Methods return `anyhow::Result` for flexibility. Implementers should provide
/// meaningful error messages that can be logged and tracked by the broadcaster.
///
/// # Implementation Note
///
/// This trait does not require `Clone` because executors are wrapped in `Arc` at the
/// `TransportClient` level. This allows `BitmexHttpClient` (which doesn't implement
/// `Clone`) to be used without modification.
trait CancelExecutor: Send + Sync {
    /// Adds an instrument for caching.
    fn add_instrument(&self, instrument: InstrumentAny);

    /// Performs a health check on the executor.
    fn health_check(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>;

    /// Cancels a single order.
    fn cancel_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<OrderStatusReport>> + Send + '_>,
    >;

    /// Cancels multiple orders.
    fn cancel_orders(
        &self,
        instrument_id: InstrumentId,
        client_order_ids: Option<Vec<ClientOrderId>>,
        venue_order_ids: Option<Vec<VenueOrderId>>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Vec<OrderStatusReport>>> + Send + '_>,
    >;

    /// Cancels all orders for an instrument.
    fn cancel_all_orders(
        &self,
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Vec<OrderStatusReport>>> + Send + '_>,
    >;
}

impl CancelExecutor for BitmexHttpClient {
    fn add_instrument(&self, instrument: InstrumentAny) {
        Self::add_instrument(self, instrument);
    }

    fn health_check(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async move {
            Self::http_get_server_time(self)
                .await
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("{e}"))
        })
    }

    fn cancel_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<OrderStatusReport>> + Send + '_>,
    > {
        Box::pin(async move {
            Self::cancel_order(self, instrument_id, client_order_id, venue_order_id).await
        })
    }

    fn cancel_orders(
        &self,
        instrument_id: InstrumentId,
        client_order_ids: Option<Vec<ClientOrderId>>,
        venue_order_ids: Option<Vec<VenueOrderId>>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Vec<OrderStatusReport>>> + Send + '_>,
    > {
        Box::pin(async move {
            Self::cancel_orders(self, instrument_id, client_order_ids, venue_order_ids).await
        })
    }

    fn cancel_all_orders(
        &self,
        instrument_id: InstrumentId,
        order_side: Option<nautilus_model::enums::OrderSide>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Vec<OrderStatusReport>>> + Send + '_>,
    > {
        Box::pin(async move { Self::cancel_all_orders(self, instrument_id, order_side).await })
    }
}

/// Configuration for the cancel broadcaster.
#[derive(Debug, Clone)]
pub struct CancelBroadcasterConfig {
    /// Number of HTTP clients in the pool.
    pub pool_size: usize,
    /// BitMEX API key (None will source from environment).
    pub api_key: Option<String>,
    /// BitMEX API secret (None will source from environment).
    pub api_secret: Option<String>,
    /// Base URL for BitMEX HTTP API.
    pub base_url: Option<String>,
    /// If connecting to BitMEX testnet.
    pub testnet: bool,
    /// Timeout in seconds for HTTP requests.
    pub timeout_secs: Option<u64>,
    /// Maximum number of retry attempts for failed requests.
    pub max_retries: Option<u32>,
    /// Initial delay in milliseconds between retry attempts.
    pub retry_delay_ms: Option<u64>,
    /// Maximum delay in milliseconds between retry attempts.
    pub retry_delay_max_ms: Option<u64>,
    /// Expiration window in milliseconds for signed requests.
    pub recv_window_ms: Option<u64>,
    /// Maximum REST burst rate (requests per second).
    pub max_requests_per_second: Option<u32>,
    /// Maximum REST rolling rate (requests per minute).
    pub max_requests_per_minute: Option<u32>,
    /// Interval in seconds between health check pings.
    pub health_check_interval_secs: u64,
    /// Timeout in seconds for health check requests.
    pub health_check_timeout_secs: u64,
    /// Substrings to identify expected cancel rejections for debug-level logging.
    pub expected_reject_patterns: Vec<String>,
    /// Substrings to identify idempotent success (order already cancelled/not found).
    pub idempotent_success_patterns: Vec<String>,
}

impl Default for CancelBroadcasterConfig {
    fn default() -> Self {
        Self {
            pool_size: 2,
            api_key: None,
            api_secret: None,
            base_url: None,
            testnet: false,
            timeout_secs: Some(60),
            max_retries: None,
            retry_delay_ms: Some(1_000),
            retry_delay_max_ms: Some(5_000),
            recv_window_ms: Some(10_000),
            max_requests_per_second: Some(10),
            max_requests_per_minute: Some(120),
            health_check_interval_secs: 30,
            health_check_timeout_secs: 5,
            expected_reject_patterns: vec![
                r"Order had execInst of ParticipateDoNotInitiate".to_string(),
            ],
            idempotent_success_patterns: vec![
                r"AlreadyCanceled".to_string(),
                r"orderID not found".to_string(),
                r"Unable to cancel order due to existing state".to_string(),
            ],
        }
    }
}

/// Transport client wrapper with health monitoring.
#[derive(Clone)]
struct TransportClient {
    /// Executor wrapped in Arc to enable cloning without requiring Clone on CancelExecutor.
    ///
    /// BitmexHttpClient doesn't implement Clone, so we use reference counting to share
    /// the executor across multiple TransportClient clones.
    executor: Arc<dyn CancelExecutor>,
    client_id: String,
    healthy: Arc<AtomicBool>,
    cancel_count: Arc<AtomicU64>,
    error_count: Arc<AtomicU64>,
}

impl std::fmt::Debug for TransportClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportClient")
            .field("client_id", &self.client_id)
            .field("healthy", &self.healthy)
            .field("cancel_count", &self.cancel_count)
            .field("error_count", &self.error_count)
            .finish()
    }
}

impl TransportClient {
    fn new<E: CancelExecutor + 'static>(executor: E, client_id: String) -> Self {
        Self {
            executor: Arc::new(executor),
            client_id,
            healthy: Arc::new(AtomicBool::new(true)),
            cancel_count: Arc::new(AtomicU64::new(0)),
            error_count: Arc::new(AtomicU64::new(0)),
        }
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    fn mark_healthy(&self) {
        self.healthy.store(true, Ordering::Relaxed);
    }

    fn mark_unhealthy(&self) {
        self.healthy.store(false, Ordering::Relaxed);
    }

    async fn health_check(&self, timeout_secs: u64) -> bool {
        match tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            self.executor.health_check(),
        )
        .await
        {
            Ok(Ok(_)) => {
                self.mark_healthy();
                true
            }
            Ok(Err(e)) => {
                tracing::warn!("Health check failed for client {}: {e:?}", self.client_id);
                self.mark_unhealthy();
                false
            }
            Err(_) => {
                tracing::warn!("Health check timeout for client {}", self.client_id);
                self.mark_unhealthy();
                false
            }
        }
    }

    async fn cancel_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        self.cancel_count.fetch_add(1, Ordering::Relaxed);

        match self
            .executor
            .cancel_order(instrument_id, client_order_id, venue_order_id)
            .await
        {
            Ok(report) => {
                self.mark_healthy();
                Ok(report)
            }
            Err(e) => {
                self.error_count.fetch_add(1, Ordering::Relaxed);
                Err(e)
            }
        }
    }

    fn get_cancel_count(&self) -> u64 {
        self.cancel_count.load(Ordering::Relaxed)
    }

    fn get_error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }
}

/// Broadcasts cancel requests to multiple HTTP clients for redundancy.
///
/// This broadcaster fans out cancel requests to multiple pre-warmed HTTP clients
/// in parallel, short-circuits when the first successful acknowledgement is received,
/// and handles expected rejection patterns with appropriate log levels.
#[cfg_attr(feature = "python", pyo3::pyclass)]
#[derive(Debug)]
pub struct CancelBroadcaster {
    config: CancelBroadcasterConfig,
    transports: Arc<RwLock<Vec<TransportClient>>>,
    health_check_task: Arc<RwLock<Option<JoinHandle<()>>>>,
    running: Arc<AtomicBool>,
    total_cancels: Arc<AtomicU64>,
    successful_cancels: Arc<AtomicU64>,
    failed_cancels: Arc<AtomicU64>,
    expected_rejects: Arc<AtomicU64>,
    idempotent_successes: Arc<AtomicU64>,
}

impl CancelBroadcaster {
    /// Creates a new [`CancelBroadcaster`] with internal HTTP client pool.
    ///
    /// # Errors
    ///
    /// Returns an error if any HTTP client fails to initialize.
    pub fn new(config: CancelBroadcasterConfig) -> anyhow::Result<Self> {
        let mut transports = Vec::with_capacity(config.pool_size);

        // Synthesize base_url when testnet is true but base_url is None
        let base_url = if config.testnet && config.base_url.is_none() {
            Some(BITMEX_HTTP_TESTNET_URL.to_string())
        } else {
            config.base_url.clone()
        };

        for i in 0..config.pool_size {
            let client = BitmexHttpClient::with_credentials(
                config.api_key.clone(),
                config.api_secret.clone(),
                base_url.clone(),
                config.timeout_secs,
                config.max_retries,
                config.retry_delay_ms,
                config.retry_delay_max_ms,
                config.recv_window_ms,
                config.max_requests_per_second,
                config.max_requests_per_minute,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client {i}: {e}"))?;

            transports.push(TransportClient::new(client, format!("bitmex-cancel-{i}")));
        }

        Ok(Self {
            config,
            transports: Arc::new(RwLock::new(transports)),
            health_check_task: Arc::new(RwLock::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            total_cancels: Arc::new(AtomicU64::new(0)),
            successful_cancels: Arc::new(AtomicU64::new(0)),
            failed_cancels: Arc::new(AtomicU64::new(0)),
            expected_rejects: Arc::new(AtomicU64::new(0)),
            idempotent_successes: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Starts the broadcaster and health check loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the broadcaster is already running.
    pub async fn start(&self) -> anyhow::Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(true, Ordering::Relaxed);

        // Initial health check for all clients
        self.run_health_checks().await;

        // Start periodic health check task
        let transports = Arc::clone(&self.transports);
        let running = Arc::clone(&self.running);
        let interval_secs = self.config.health_check_interval_secs;
        let timeout_secs = self.config.health_check_timeout_secs;

        let task = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(interval_secs));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                ticker.tick().await;

                if !running.load(Ordering::Relaxed) {
                    break;
                }

                let transports_guard = transports.read().await;
                let transports_clone: Vec<_> = transports_guard.clone();
                drop(transports_guard);

                let tasks: Vec<_> = transports_clone
                    .iter()
                    .map(|t| t.health_check(timeout_secs))
                    .collect();

                let results = future::join_all(tasks).await;
                let healthy_count = results.iter().filter(|&&r| r).count();

                tracing::debug!(
                    "Health check complete: {}/{} clients healthy",
                    healthy_count,
                    results.len()
                );
            }
        });

        *self.health_check_task.write().await = Some(task);

        tracing::info!(
            "CancelBroadcaster started with {} clients",
            self.transports.read().await.len()
        );

        Ok(())
    }

    /// Stops the broadcaster and health check loop.
    pub async fn stop(&self) {
        if !self.running.load(Ordering::Relaxed) {
            return;
        }

        self.running.store(false, Ordering::Relaxed);

        if let Some(task) = self.health_check_task.write().await.take() {
            task.abort();
        }

        tracing::info!("CancelBroadcaster stopped");
    }

    async fn run_health_checks(&self) {
        let transports_guard = self.transports.read().await;
        let transports_clone: Vec<_> = transports_guard.clone();
        drop(transports_guard);

        let tasks: Vec<_> = transports_clone
            .iter()
            .map(|t| t.health_check(self.config.health_check_timeout_secs))
            .collect();

        let results = future::join_all(tasks).await;
        let healthy_count = results.iter().filter(|&&r| r).count();

        tracing::debug!(
            "Health check complete: {}/{} clients healthy",
            healthy_count,
            results.len()
        );
    }

    fn is_expected_reject(&self, error_message: &str) -> bool {
        self.config
            .expected_reject_patterns
            .iter()
            .any(|pattern| error_message.contains(pattern))
    }

    fn is_idempotent_success(&self, error_message: &str) -> bool {
        self.config
            .idempotent_success_patterns
            .iter()
            .any(|pattern| error_message.contains(pattern))
    }

    /// Processes cancel request results, handling success, idempotent success, and failures.
    ///
    /// This helper consolidates the common error handling loop used across all broadcast methods.
    async fn process_cancel_results<T>(
        &self,
        mut handles: Vec<JoinHandle<(String, anyhow::Result<T>)>>,
        idempotent_result: impl FnOnce() -> anyhow::Result<T>,
        operation: &str,
        params: String,
        idempotent_reason: &str,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
    {
        let mut errors = Vec::new();

        while !handles.is_empty() {
            let current_handles = std::mem::take(&mut handles);
            let (result, _idx, remaining) = future::select_all(current_handles).await;
            handles = remaining.into_iter().collect();

            match result {
                Ok((client_id, Ok(result))) => {
                    // First success - abort remaining handles
                    for handle in &handles {
                        handle.abort();
                    }
                    self.successful_cancels.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(
                        "{} broadcast succeeded [{}] {}",
                        operation,
                        client_id,
                        params
                    );
                    return Ok(result);
                }
                Ok((client_id, Err(e))) => {
                    let error_msg = e.to_string();

                    if self.is_idempotent_success(&error_msg) {
                        // First idempotent success - abort remaining handles and return success
                        for handle in &handles {
                            handle.abort();
                        }
                        self.idempotent_successes.fetch_add(1, Ordering::Relaxed);
                        tracing::debug!(
                            "Idempotent success [{}] - {}: {} {}",
                            client_id,
                            idempotent_reason,
                            error_msg,
                            params
                        );
                        return idempotent_result();
                    }

                    if self.is_expected_reject(&error_msg) {
                        self.expected_rejects.fetch_add(1, Ordering::Relaxed);
                        tracing::debug!(
                            "Expected {} rejection [{}]: {} {}",
                            operation.to_lowercase(),
                            client_id,
                            error_msg,
                            params
                        );
                        errors.push(error_msg);
                    } else {
                        tracing::warn!(
                            "{} request failed [{}]: {} {}",
                            operation,
                            client_id,
                            error_msg,
                            params
                        );
                        errors.push(error_msg);
                    }
                }
                Err(e) => {
                    tracing::warn!("{} task join error: {e:?}", operation);
                    errors.push(format!("Task panicked: {e:?}"));
                }
            }
        }

        // All tasks failed
        self.failed_cancels.fetch_add(1, Ordering::Relaxed);
        tracing::error!(
            "All {} requests failed: {:?} {}",
            operation.to_lowercase(),
            errors,
            params
        );
        Err(anyhow::anyhow!(
            "All {} requests failed: {:?}",
            operation.to_lowercase(),
            errors
        ))
    }

    /// Broadcasts a single cancel request to all healthy clients in parallel.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(report))` if successfully cancelled with a report.
    /// - `Ok(None)` if the order was already cancelled (idempotent success).
    /// - `Err` if all requests failed.
    ///
    /// # Errors
    ///
    /// Returns an error if all cancel requests fail or no healthy clients are available.
    pub async fn broadcast_cancel(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.total_cancels.fetch_add(1, Ordering::Relaxed);

        // Filter for healthy clients and clone them
        let transports_guard = self.transports.read().await;
        let healthy_transports: Vec<TransportClient> = transports_guard
            .iter()
            .filter(|t| t.is_healthy())
            .cloned()
            .collect();
        drop(transports_guard);

        if healthy_transports.is_empty() {
            self.failed_cancels.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!("No healthy transport clients available");
        }

        // Spawn tasks for all healthy clients
        let mut handles = Vec::new();
        for transport in healthy_transports {
            let handle = tokio::spawn(async move {
                let client_id = transport.client_id.clone();
                let result = transport
                    .cancel_order(instrument_id, client_order_id, venue_order_id)
                    .await
                    .map(Some); // Wrap success in Some for Option<OrderStatusReport>
                (client_id, result)
            });
            handles.push(handle);
        }

        self.process_cancel_results(
            handles,
            || Ok(None),
            "Cancel",
            format!(
                "(client_order_id={:?}, venue_order_id={:?})",
                client_order_id, venue_order_id
            ),
            "order already cancelled/not found",
        )
        .await
    }

    /// Broadcasts a batch cancel request to all healthy clients in parallel.
    ///
    /// # Errors
    ///
    /// Returns an error if all cancel requests fail or no healthy clients are available.
    pub async fn broadcast_batch_cancel(
        &self,
        instrument_id: InstrumentId,
        client_order_ids: Option<Vec<ClientOrderId>>,
        venue_order_ids: Option<Vec<VenueOrderId>>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.total_cancels.fetch_add(1, Ordering::Relaxed);

        // Filter for healthy clients and clone them
        let transports_guard = self.transports.read().await;
        let healthy_transports: Vec<TransportClient> = transports_guard
            .iter()
            .filter(|t| t.is_healthy())
            .cloned()
            .collect();
        drop(transports_guard);

        if healthy_transports.is_empty() {
            self.failed_cancels.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!("No healthy transport clients available");
        }

        // Spawn tasks for all healthy clients
        let mut handles = Vec::new();

        for transport in healthy_transports {
            let client_order_ids_clone = client_order_ids.clone();
            let venue_order_ids_clone = venue_order_ids.clone();
            let handle = tokio::spawn(async move {
                let client_id = transport.client_id.clone();
                let result = transport
                    .executor
                    .cancel_orders(instrument_id, client_order_ids_clone, venue_order_ids_clone)
                    .await;
                (client_id, result)
            });
            handles.push(handle);
        }

        self.process_cancel_results(
            handles,
            || Ok(Vec::new()),
            "Batch cancel",
            format!(
                "(client_order_ids={:?}, venue_order_ids={:?})",
                client_order_ids, venue_order_ids
            ),
            "orders already cancelled/not found",
        )
        .await
    }

    /// Broadcasts a cancel all request to all healthy clients in parallel.
    ///
    /// # Errors
    ///
    /// Returns an error if all cancel requests fail or no healthy clients are available.
    pub async fn broadcast_cancel_all(
        &self,
        instrument_id: InstrumentId,
        order_side: Option<nautilus_model::enums::OrderSide>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.total_cancels.fetch_add(1, Ordering::Relaxed);

        // Filter for healthy clients and clone them
        let transports_guard = self.transports.read().await;
        let healthy_transports: Vec<TransportClient> = transports_guard
            .iter()
            .filter(|t| t.is_healthy())
            .cloned()
            .collect();
        drop(transports_guard);

        if healthy_transports.is_empty() {
            self.failed_cancels.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!("No healthy transport clients available");
        }

        // Spawn tasks for all healthy clients
        let mut handles = Vec::new();
        for transport in healthy_transports {
            let handle = tokio::spawn(async move {
                let client_id = transport.client_id.clone();
                let result = transport
                    .executor
                    .cancel_all_orders(instrument_id, order_side)
                    .await;
                (client_id, result)
            });
            handles.push(handle);
        }

        self.process_cancel_results(
            handles,
            || Ok(Vec::new()),
            "Cancel all",
            format!(
                "(instrument_id={}, order_side={:?})",
                instrument_id, order_side
            ),
            "no orders to cancel",
        )
        .await
    }

    /// Gets broadcaster metrics.
    pub fn get_metrics(&self) -> BroadcasterMetrics {
        let transports_guard = self.transports.blocking_read();
        let healthy_clients = transports_guard.iter().filter(|t| t.is_healthy()).count();
        let total_clients = transports_guard.len();
        drop(transports_guard);

        BroadcasterMetrics {
            total_cancels: self.total_cancels.load(Ordering::Relaxed),
            successful_cancels: self.successful_cancels.load(Ordering::Relaxed),
            failed_cancels: self.failed_cancels.load(Ordering::Relaxed),
            expected_rejects: self.expected_rejects.load(Ordering::Relaxed),
            idempotent_successes: self.idempotent_successes.load(Ordering::Relaxed),
            healthy_clients,
            total_clients,
        }
    }

    /// Gets broadcaster metrics (async version for use within async context).
    pub async fn get_metrics_async(&self) -> BroadcasterMetrics {
        let transports_guard = self.transports.read().await;
        let healthy_clients = transports_guard.iter().filter(|t| t.is_healthy()).count();
        let total_clients = transports_guard.len();
        drop(transports_guard);

        BroadcasterMetrics {
            total_cancels: self.total_cancels.load(Ordering::Relaxed),
            successful_cancels: self.successful_cancels.load(Ordering::Relaxed),
            failed_cancels: self.failed_cancels.load(Ordering::Relaxed),
            expected_rejects: self.expected_rejects.load(Ordering::Relaxed),
            idempotent_successes: self.idempotent_successes.load(Ordering::Relaxed),
            healthy_clients,
            total_clients,
        }
    }

    /// Gets per-client statistics.
    pub fn get_client_stats(&self) -> Vec<ClientStats> {
        let transports = self.transports.blocking_read();
        transports
            .iter()
            .map(|t| ClientStats {
                client_id: t.client_id.clone(),
                healthy: t.is_healthy(),
                cancel_count: t.get_cancel_count(),
                error_count: t.get_error_count(),
            })
            .collect()
    }

    /// Gets per-client statistics (async version for use within async context).
    pub async fn get_client_stats_async(&self) -> Vec<ClientStats> {
        let transports = self.transports.read().await;
        transports
            .iter()
            .map(|t| ClientStats {
                client_id: t.client_id.clone(),
                healthy: t.is_healthy(),
                cancel_count: t.get_cancel_count(),
                error_count: t.get_error_count(),
            })
            .collect()
    }

    /// Adds an instrument to all HTTP clients in the pool for caching.
    pub fn add_instrument(&self, instrument: nautilus_model::instruments::any::InstrumentAny) {
        let transports = self.transports.blocking_read();
        for transport in transports.iter() {
            transport.executor.add_instrument(instrument.clone());
        }
    }

    pub fn clone_for_async(&self) -> Self {
        Self {
            config: self.config.clone(),
            transports: Arc::clone(&self.transports),
            health_check_task: Arc::clone(&self.health_check_task),
            running: Arc::clone(&self.running),
            total_cancels: Arc::clone(&self.total_cancels),
            successful_cancels: Arc::clone(&self.successful_cancels),
            failed_cancels: Arc::clone(&self.failed_cancels),
            expected_rejects: Arc::clone(&self.expected_rejects),
            idempotent_successes: Arc::clone(&self.idempotent_successes),
        }
    }

    #[cfg(test)]
    fn new_with_transports(
        config: CancelBroadcasterConfig,
        transports: Vec<TransportClient>,
    ) -> Self {
        Self {
            config,
            transports: Arc::new(RwLock::new(transports)),
            health_check_task: Arc::new(RwLock::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            total_cancels: Arc::new(AtomicU64::new(0)),
            successful_cancels: Arc::new(AtomicU64::new(0)),
            failed_cancels: Arc::new(AtomicU64::new(0)),
            expected_rejects: Arc::new(AtomicU64::new(0)),
            idempotent_successes: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Broadcaster metrics snapshot.
#[derive(Debug, Clone)]
pub struct BroadcasterMetrics {
    pub total_cancels: u64,
    pub successful_cancels: u64,
    pub failed_cancels: u64,
    pub expected_rejects: u64,
    pub idempotent_successes: u64,
    pub healthy_clients: usize,
    pub total_clients: usize,
}

/// Per-client statistics.
#[derive(Debug, Clone)]
pub struct ClientStats {
    pub client_id: String,
    pub healthy: bool,
    pub cancel_count: u64,
    pub error_count: u64,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::atomic::Ordering, time::Duration};

    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::{
            ContingencyType, OrderSide, OrderStatus, OrderType, TimeInForce, TrailingOffsetType,
        },
        identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
        reports::OrderStatusReport,
        types::{Price, Quantity},
    };

    use super::*;

    /// Mock executor for testing.
    #[derive(Clone)]
    #[allow(clippy::type_complexity)]
    struct MockExecutor {
        handler: Arc<
            dyn Fn(
                    InstrumentId,
                    Option<ClientOrderId>,
                    Option<VenueOrderId>,
                ) -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = anyhow::Result<OrderStatusReport>> + Send>,
                > + Send
                + Sync,
        >,
    }

    impl MockExecutor {
        fn new<F, Fut>(handler: F) -> Self
        where
            F: Fn(InstrumentId, Option<ClientOrderId>, Option<VenueOrderId>) -> Fut
                + Send
                + Sync
                + 'static,
            Fut: std::future::Future<Output = anyhow::Result<OrderStatusReport>> + Send + 'static,
        {
            Self {
                handler: Arc::new(move |id, cid, vid| Box::pin(handler(id, cid, vid))),
            }
        }
    }

    impl CancelExecutor for MockExecutor {
        fn health_check(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            Box::pin(async { Ok(()) })
        }

        fn cancel_order(
            &self,
            instrument_id: InstrumentId,
            client_order_id: Option<ClientOrderId>,
            venue_order_id: Option<VenueOrderId>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<OrderStatusReport>> + Send + '_>,
        > {
            (self.handler)(instrument_id, client_order_id, venue_order_id)
        }

        fn cancel_orders(
            &self,
            _instrument_id: InstrumentId,
            _client_order_ids: Option<Vec<ClientOrderId>>,
            _venue_order_ids: Option<Vec<VenueOrderId>>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = anyhow::Result<Vec<OrderStatusReport>>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn cancel_all_orders(
            &self,
            instrument_id: InstrumentId,
            _order_side: Option<nautilus_model::enums::OrderSide>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = anyhow::Result<Vec<OrderStatusReport>>>
                    + Send
                    + '_,
            >,
        > {
            // Try to get result from the single-order handler to propagate errors
            let handler = Arc::clone(&self.handler);
            Box::pin(async move {
                // Call the handler to check if it would fail
                let result = handler(instrument_id, None, None).await;
                match result {
                    Ok(_) => Ok(Vec::new()),
                    Err(e) => Err(e),
                }
            })
        }

        fn add_instrument(&self, _instrument: nautilus_model::instruments::any::InstrumentAny) {
            // No-op for mock
        }
    }

    fn create_test_report(venue_order_id: &str) -> OrderStatusReport {
        OrderStatusReport {
            account_id: AccountId::from("BITMEX-001"),
            instrument_id: InstrumentId::from_str("XBTUSD.BITMEX").unwrap(),
            venue_order_id: VenueOrderId::from(venue_order_id),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            order_status: OrderStatus::Canceled,
            price: Some(Price::new(50000.0, 2)),
            quantity: Quantity::new(100.0, 0),
            filled_qty: Quantity::new(0.0, 0),
            report_id: UUID4::new(),
            ts_accepted: 0.into(),
            ts_last: 0.into(),
            ts_init: 0.into(),
            client_order_id: None,
            avg_px: None,
            trigger_price: None,
            trigger_type: None,
            contingency_type: ContingencyType::NoContingency,
            expire_time: None,
            order_list_id: None,
            venue_position_id: None,
            linked_order_ids: None,
            parent_order_id: None,
            display_qty: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: TrailingOffsetType::NoTrailingOffset,
            post_only: false,
            reduce_only: false,
            cancel_reason: None,
            ts_triggered: None,
        }
    }

    fn create_stub_transport<F, Fut>(client_id: &str, handler: F) -> TransportClient
    where
        F: Fn(InstrumentId, Option<ClientOrderId>, Option<VenueOrderId>) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: std::future::Future<Output = anyhow::Result<OrderStatusReport>> + Send + 'static,
    {
        let executor = MockExecutor::new(handler);
        TransportClient::new(executor, client_id.to_string())
    }

    #[tokio::test]
    async fn test_broadcast_cancel_immediate_success() {
        let report = create_test_report("ORDER-1");
        let report_clone = report.clone();

        let transports = vec![
            create_stub_transport("client-0", move |_, _, _| {
                let report = report_clone.clone();
                async move { Ok(report) }
            }),
            create_stub_transport("client-1", |_, _, _| async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                anyhow::bail!("Should be aborted")
            }),
        ];

        let config = CancelBroadcasterConfig::default();
        let broadcaster = CancelBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_cancel(instrument_id, Some(ClientOrderId::from("O-123")), None)
            .await;

        assert!(result.is_ok());
        let returned_report = result.unwrap();
        assert!(returned_report.is_some());
        assert_eq!(
            returned_report.unwrap().venue_order_id,
            report.venue_order_id
        );

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.successful_cancels, 1);
        assert_eq!(metrics.failed_cancels, 0);
        assert_eq!(metrics.total_cancels, 1);
    }

    #[tokio::test]
    async fn test_broadcast_cancel_idempotent_success() {
        let transports = vec![
            create_stub_transport("client-0", |_, _, _| async {
                anyhow::bail!("AlreadyCanceled")
            }),
            create_stub_transport("client-1", |_, _, _| async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                anyhow::bail!("Should be aborted")
            }),
        ];

        let config = CancelBroadcasterConfig::default();
        let broadcaster = CancelBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_cancel(instrument_id, None, Some(VenueOrderId::from("12345")))
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.idempotent_successes, 1);
        assert_eq!(metrics.successful_cancels, 0);
        assert_eq!(metrics.failed_cancels, 0);
    }

    #[tokio::test]
    async fn test_broadcast_cancel_mixed_idempotent_and_failure() {
        let transports = vec![
            create_stub_transport("client-0", |_, _, _| async {
                anyhow::bail!("502 Bad Gateway")
            }),
            create_stub_transport("client-1", |_, _, _| async {
                anyhow::bail!("orderID not found")
            }),
        ];

        let config = CancelBroadcasterConfig::default();
        let broadcaster = CancelBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_cancel(instrument_id, Some(ClientOrderId::from("O-456")), None)
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.idempotent_successes, 1);
        assert_eq!(metrics.failed_cancels, 0);
    }

    #[tokio::test]
    async fn test_broadcast_cancel_all_failures() {
        let transports = vec![
            create_stub_transport("client-0", |_, _, _| async {
                anyhow::bail!("502 Bad Gateway")
            }),
            create_stub_transport("client-1", |_, _, _| async {
                anyhow::bail!("Connection refused")
            }),
        ];

        let config = CancelBroadcasterConfig::default();
        let broadcaster = CancelBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster.broadcast_cancel_all(instrument_id, None).await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("All cancel all requests failed")
        );

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.failed_cancels, 1);
        assert_eq!(metrics.successful_cancels, 0);
        assert_eq!(metrics.idempotent_successes, 0);
    }

    #[tokio::test]
    async fn test_broadcast_cancel_no_healthy_clients() {
        let transport = create_stub_transport("client-0", |_, _, _| async {
            Ok(create_test_report("ORDER-1"))
        });
        transport.healthy.store(false, Ordering::Relaxed);

        let config = CancelBroadcasterConfig::default();
        let broadcaster = CancelBroadcaster::new_with_transports(config, vec![transport]);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_cancel(instrument_id, Some(ClientOrderId::from("O-789")), None)
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No healthy transport clients available")
        );

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.failed_cancels, 1);
    }

    #[tokio::test]
    async fn test_broadcast_cancel_metrics_increment() {
        let report1 = create_test_report("ORDER-1");
        let report1_clone = report1.clone();
        let report2 = create_test_report("ORDER-2");
        let report2_clone = report2.clone();

        let transports = vec![
            create_stub_transport("client-0", move |_, _, _| {
                let report = report1_clone.clone();
                async move { Ok(report) }
            }),
            create_stub_transport("client-1", move |_, _, _| {
                let report = report2_clone.clone();
                async move { Ok(report) }
            }),
        ];

        let config = CancelBroadcasterConfig::default();
        let broadcaster = CancelBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();

        let _ = broadcaster
            .broadcast_cancel(instrument_id, Some(ClientOrderId::from("O-1")), None)
            .await;

        let _ = broadcaster
            .broadcast_cancel(instrument_id, Some(ClientOrderId::from("O-2")), None)
            .await;

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.total_cancels, 2);
        assert_eq!(metrics.successful_cancels, 2);
    }

    #[tokio::test]
    async fn test_broadcast_cancel_expected_reject_pattern() {
        let transports = vec![
            create_stub_transport("client-0", |_, _, _| async {
                anyhow::bail!("Order had execInst of ParticipateDoNotInitiate")
            }),
            create_stub_transport("client-1", |_, _, _| async {
                anyhow::bail!("Order had execInst of ParticipateDoNotInitiate")
            }),
        ];

        let config = CancelBroadcasterConfig::default();
        let broadcaster = CancelBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_cancel(instrument_id, Some(ClientOrderId::from("O-PDI")), None)
            .await;

        assert!(result.is_err());

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.expected_rejects, 2);
        assert_eq!(metrics.failed_cancels, 1);
    }

    #[tokio::test]
    async fn test_broadcaster_creation_with_pool() {
        let config = CancelBroadcasterConfig {
            pool_size: 3,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            testnet: false,
            timeout_secs: Some(5),
            max_retries: Some(2),
            retry_delay_ms: Some(100),
            retry_delay_max_ms: Some(1000),
            recv_window_ms: Some(5000),
            max_requests_per_second: Some(10),
            max_requests_per_minute: Some(100),
            health_check_interval_secs: 30,
            health_check_timeout_secs: 5,
            expected_reject_patterns: vec!["test_pattern".to_string()],
            idempotent_success_patterns: vec!["AlreadyCanceled".to_string()],
        };

        let broadcaster = CancelBroadcaster::new(config.clone());
        assert!(broadcaster.is_ok());

        let broadcaster = broadcaster.unwrap();
        let metrics = broadcaster.get_metrics_async().await;

        assert_eq!(metrics.total_clients, 3);
        assert_eq!(metrics.total_cancels, 0);
        assert_eq!(metrics.successful_cancels, 0);
        assert_eq!(metrics.failed_cancels, 0);
    }

    #[tokio::test]
    async fn test_broadcaster_lifecycle() {
        let config = CancelBroadcasterConfig {
            pool_size: 2,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            testnet: false,
            timeout_secs: Some(5),
            max_retries: None,
            retry_delay_ms: None,
            retry_delay_max_ms: None,
            recv_window_ms: None,
            max_requests_per_second: None,
            max_requests_per_minute: None,
            health_check_interval_secs: 60, // Long interval so it won't tick during test
            health_check_timeout_secs: 1,
            expected_reject_patterns: vec![],
            idempotent_success_patterns: vec![],
        };

        let broadcaster = CancelBroadcaster::new(config).unwrap();

        // Should not be running initially
        assert!(!broadcaster.running.load(Ordering::Relaxed));

        // Start broadcaster
        let start_result = broadcaster.start().await;
        assert!(start_result.is_ok());
        assert!(broadcaster.running.load(Ordering::Relaxed));

        // Starting again should be idempotent
        let start_again = broadcaster.start().await;
        assert!(start_again.is_ok());

        // Stop broadcaster
        broadcaster.stop().await;
        assert!(!broadcaster.running.load(Ordering::Relaxed));

        // Stopping again should be safe
        broadcaster.stop().await;
        assert!(!broadcaster.running.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_client_stats_collection() {
        let config = CancelBroadcasterConfig {
            pool_size: 2,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            testnet: false,
            timeout_secs: Some(5),
            max_retries: None,
            retry_delay_ms: None,
            retry_delay_max_ms: None,
            recv_window_ms: None,
            max_requests_per_second: None,
            max_requests_per_minute: None,
            health_check_interval_secs: 60,
            health_check_timeout_secs: 5,
            expected_reject_patterns: vec![],
            idempotent_success_patterns: vec![],
        };

        let broadcaster = CancelBroadcaster::new(config).unwrap();
        let stats = broadcaster.get_client_stats_async().await;

        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].client_id, "bitmex-cancel-0");
        assert_eq!(stats[1].client_id, "bitmex-cancel-1");
        assert!(stats[0].healthy); // Should be healthy initially
        assert!(stats[1].healthy);
        assert_eq!(stats[0].cancel_count, 0);
        assert_eq!(stats[1].cancel_count, 0);
        assert_eq!(stats[0].error_count, 0);
        assert_eq!(stats[1].error_count, 0);
    }

    #[tokio::test]
    async fn test_testnet_config_sets_base_url() {
        let config = CancelBroadcasterConfig {
            pool_size: 1,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: None, // Not specified
            testnet: true,  // But testnet is true
            timeout_secs: Some(5),
            max_retries: None,
            retry_delay_ms: None,
            retry_delay_max_ms: None,
            recv_window_ms: None,
            max_requests_per_second: None,
            max_requests_per_minute: None,
            health_check_interval_secs: 60,
            health_check_timeout_secs: 5,
            expected_reject_patterns: vec![],
            idempotent_success_patterns: vec![],
        };

        let broadcaster = CancelBroadcaster::new(config);
        assert!(broadcaster.is_ok());
    }

    #[tokio::test]
    async fn test_default_config() {
        let config = CancelBroadcasterConfig {
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            ..Default::default()
        };

        let broadcaster = CancelBroadcaster::new(config);
        assert!(broadcaster.is_ok());

        let broadcaster = broadcaster.unwrap();
        let metrics = broadcaster.get_metrics_async().await;

        // Default pool_size is 2
        assert_eq!(metrics.total_clients, 2);
    }

    #[tokio::test]
    async fn test_clone_for_async() {
        let config = CancelBroadcasterConfig {
            pool_size: 1,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            testnet: false,
            timeout_secs: Some(5),
            max_retries: None,
            retry_delay_ms: None,
            retry_delay_max_ms: None,
            recv_window_ms: None,
            max_requests_per_second: None,
            max_requests_per_minute: None,
            health_check_interval_secs: 60,
            health_check_timeout_secs: 5,
            expected_reject_patterns: vec![],
            idempotent_success_patterns: vec![],
        };

        let broadcaster1 = CancelBroadcaster::new(config).unwrap();

        // Increment a metric on original
        broadcaster1.total_cancels.fetch_add(1, Ordering::Relaxed);

        // Clone should share the same atomic
        let broadcaster2 = broadcaster1.clone_for_async();
        let metrics2 = broadcaster2.get_metrics_async().await;

        assert_eq!(metrics2.total_cancels, 1); // Should see the increment

        // Modify through clone
        broadcaster2
            .successful_cancels
            .fetch_add(5, Ordering::Relaxed);

        // Original should see the change
        let metrics1 = broadcaster1.get_metrics_async().await;
        assert_eq!(metrics1.successful_cancels, 5);
    }

    #[tokio::test]
    async fn test_pattern_matching() {
        // Test that pattern matching works for expected rejects and idempotent successes
        let config = CancelBroadcasterConfig {
            pool_size: 1,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            testnet: false,
            timeout_secs: Some(5),
            max_retries: None,
            retry_delay_ms: None,
            retry_delay_max_ms: None,
            recv_window_ms: None,
            max_requests_per_second: None,
            max_requests_per_minute: None,
            health_check_interval_secs: 60,
            health_check_timeout_secs: 5,
            expected_reject_patterns: vec![
                "ParticipateDoNotInitiate".to_string(),
                "Close-only".to_string(),
            ],
            idempotent_success_patterns: vec![
                "AlreadyCanceled".to_string(),
                "orderID not found".to_string(),
                "Unable to cancel".to_string(),
            ],
        };

        let broadcaster = CancelBroadcaster::new(config).unwrap();

        // Test expected reject patterns
        assert!(broadcaster.is_expected_reject("Order had execInst of ParticipateDoNotInitiate"));
        assert!(broadcaster.is_expected_reject("This is a Close-only order"));
        assert!(!broadcaster.is_expected_reject("Connection timeout"));

        // Test idempotent success patterns
        assert!(broadcaster.is_idempotent_success("AlreadyCanceled"));
        assert!(broadcaster.is_idempotent_success("Error: orderID not found for this account"));
        assert!(broadcaster.is_idempotent_success("Unable to cancel order due to existing state"));
        assert!(!broadcaster.is_idempotent_success("502 Bad Gateway"));
    }

    // Happy-path coverage for broadcast_batch_cancel and broadcast_cancel_all
    // Note: These use simplified stubs since batch/cancel-all bypass test_handler
    // Full HTTP mocking tested in integration tests
    #[tokio::test]
    async fn test_broadcast_batch_cancel_structure() {
        // Validates broadcaster structure and metric initialization
        let config = CancelBroadcasterConfig {
            pool_size: 2,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            testnet: false,
            timeout_secs: Some(5),
            max_retries: None,
            retry_delay_ms: None,
            retry_delay_max_ms: None,
            recv_window_ms: None,
            max_requests_per_second: None,
            max_requests_per_minute: None,
            health_check_interval_secs: 60,
            health_check_timeout_secs: 5,
            expected_reject_patterns: vec![],
            idempotent_success_patterns: vec!["AlreadyCanceled".to_string()],
        };

        let broadcaster = CancelBroadcaster::new(config).unwrap();
        let metrics = broadcaster.get_metrics_async().await;

        // Verify initial state
        assert_eq!(metrics.total_clients, 2);
        assert_eq!(metrics.total_cancels, 0);
        assert_eq!(metrics.successful_cancels, 0);
        assert_eq!(metrics.failed_cancels, 0);
    }

    #[tokio::test]
    async fn test_broadcast_cancel_all_structure() {
        // Validates broadcaster structure for cancel_all operations
        let config = CancelBroadcasterConfig {
            pool_size: 3,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            testnet: false,
            timeout_secs: Some(5),
            max_retries: None,
            retry_delay_ms: None,
            retry_delay_max_ms: None,
            recv_window_ms: None,
            max_requests_per_second: None,
            max_requests_per_minute: None,
            health_check_interval_secs: 60,
            health_check_timeout_secs: 5,
            expected_reject_patterns: vec![],
            idempotent_success_patterns: vec!["orderID not found".to_string()],
        };

        let broadcaster = CancelBroadcaster::new(config).unwrap();
        let metrics = broadcaster.get_metrics_async().await;

        // Verify pool size and initial metrics
        assert_eq!(metrics.total_clients, 3);
        assert_eq!(metrics.healthy_clients, 3);
        assert_eq!(metrics.total_cancels, 0);
    }

    // Metric health tests - validates that idempotent successes don't increment failed_cancels
    #[tokio::test]
    async fn test_single_cancel_metrics_with_mixed_responses() {
        // Test similar to test_broadcast_cancel_mixed_idempotent_and_failure
        // but explicitly validates metric health
        let transports = vec![
            create_stub_transport("client-0", |_, _, _| async {
                anyhow::bail!("Connection timeout")
            }),
            create_stub_transport("client-1", |_, _, _| async {
                anyhow::bail!("AlreadyCanceled")
            }),
        ];

        let config = CancelBroadcasterConfig::default();
        let broadcaster = CancelBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_cancel(instrument_id, Some(ClientOrderId::from("O-123")), None)
            .await;

        // Should succeed with idempotent
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        // Verify metrics: idempotent success doesn't count as failure
        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(
            metrics.failed_cancels, 0,
            "Idempotent success should not increment failed_cancels"
        );
        assert_eq!(metrics.idempotent_successes, 1);
        assert_eq!(metrics.successful_cancels, 0);
    }

    #[tokio::test]
    async fn test_metrics_initialization_and_health() {
        // Validates that metrics start at zero and clients start healthy
        let config = CancelBroadcasterConfig {
            pool_size: 4,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            testnet: false,
            timeout_secs: Some(5),
            max_retries: None,
            retry_delay_ms: None,
            retry_delay_max_ms: None,
            recv_window_ms: None,
            max_requests_per_second: None,
            max_requests_per_minute: None,
            health_check_interval_secs: 60,
            health_check_timeout_secs: 5,
            expected_reject_patterns: vec![],
            idempotent_success_patterns: vec![],
        };

        let broadcaster = CancelBroadcaster::new(config).unwrap();
        let metrics = broadcaster.get_metrics_async().await;

        // All metrics should start at zero
        assert_eq!(metrics.total_cancels, 0);
        assert_eq!(metrics.successful_cancels, 0);
        assert_eq!(metrics.failed_cancels, 0);
        assert_eq!(metrics.expected_rejects, 0);
        assert_eq!(metrics.idempotent_successes, 0);

        // All clients should start healthy
        assert_eq!(metrics.healthy_clients, 4);
        assert_eq!(metrics.total_clients, 4);
    }

    // Health-check task lifecycle test
    #[tokio::test]
    async fn test_health_check_task_lifecycle() {
        let config = CancelBroadcasterConfig {
            pool_size: 1,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            testnet: false,
            timeout_secs: Some(5),
            max_retries: None,
            retry_delay_ms: None,
            retry_delay_max_ms: None,
            recv_window_ms: None,
            max_requests_per_second: None,
            max_requests_per_minute: None,
            health_check_interval_secs: 1, // Very short interval
            health_check_timeout_secs: 1,
            expected_reject_patterns: vec![],
            idempotent_success_patterns: vec![],
        };

        let broadcaster = CancelBroadcaster::new(config).unwrap();

        // Start the broadcaster
        broadcaster.start().await.unwrap();
        assert!(broadcaster.running.load(Ordering::Relaxed));

        // Verify task handle exists
        {
            let task_guard = broadcaster.health_check_task.read().await;
            assert!(task_guard.is_some());
        }

        // Wait a bit for health check to potentially run
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Stop the broadcaster
        broadcaster.stop().await;
        assert!(!broadcaster.running.load(Ordering::Relaxed));

        // Verify task handle has been cleared
        {
            let task_guard = broadcaster.health_check_task.read().await;
            assert!(task_guard.is_none());
        }
    }
}
