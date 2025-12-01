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

//! Submit request broadcaster for redundant order submission.
//!
//! This module provides the [`SubmitBroadcaster`] which fans out submit requests
//! to multiple HTTP clients in parallel for redundancy. The broadcaster is triggered
//! when the `SubmitOrder` command contains `params["broadcast_submit_tries"]`.
//!
//! Key design patterns:
//!
//! - **Dependency injection via traits**: Uses `SubmitExecutor` trait to abstract
//!   the HTTP client, enabling testing without `#[cfg(test)]` conditional compilation.
//! - **Trait objects over generics**: Uses `Arc<dyn SubmitExecutor>` to avoid
//!   generic type parameters on the public API (simpler Python FFI).
//! - **Short-circuit on first success**: Aborts remaining requests once any client
//!   succeeds, minimizing latency.
//! - **Idempotent rejection handling**: Recognizes duplicate clOrdID as expected
//!   rejections for debug-level logging without noise.

// TODO: Replace boxed futures in `SubmitExecutor` once stable async trait object support
// lands so we can drop the per-call heap allocation

use std::{
    fmt::Debug,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::future;
use nautilus_model::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TriggerType},
    identifiers::{ClientOrderId, InstrumentId, OrderListId},
    instruments::InstrumentAny,
    reports::OrderStatusReport,
    types::{Price, Quantity},
};
use tokio::{sync::RwLock, task::JoinHandle, time::interval};

use crate::{common::consts::BITMEX_HTTP_TESTNET_URL, http::client::BitmexHttpClient};

/// Trait for order submission operations.
///
/// This trait abstracts the execution layer to enable dependency injection and testing
/// without conditional compilation. The broadcaster holds executors as `Arc<dyn SubmitExecutor>`
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
trait SubmitExecutor: Send + Sync {
    /// Adds an instrument for caching.
    fn add_instrument(&self, instrument: InstrumentAny);

    /// Performs a health check on the executor.
    fn health_check(&self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>;

    /// Submits a single order.
    #[allow(clippy::too_many_arguments)]
    fn submit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
        order_list_id: Option<OrderListId>,
        contingency_type: Option<ContingencyType>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<OrderStatusReport>> + Send + '_>>;
}

impl SubmitExecutor for BitmexHttpClient {
    fn add_instrument(&self, instrument: InstrumentAny) {
        Self::cache_instrument(self, instrument);
    }

    fn health_check(&self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async move {
            Self::get_server_time(self)
                .await
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("{e}"))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn submit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
        order_list_id: Option<OrderListId>,
        contingency_type: Option<ContingencyType>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<OrderStatusReport>> + Send + '_>> {
        Box::pin(async move {
            Self::submit_order(
                self,
                instrument_id,
                client_order_id,
                order_side,
                order_type,
                quantity,
                time_in_force,
                price,
                trigger_price,
                trigger_type,
                display_qty,
                post_only,
                reduce_only,
                order_list_id,
                contingency_type,
            )
            .await
        })
    }
}

/// Configuration for the submit broadcaster.
#[derive(Debug, Clone)]
pub struct SubmitBroadcasterConfig {
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
    /// Substrings to identify expected submit rejections for debug-level logging.
    pub expected_reject_patterns: Vec<String>,
    /// Optional list of proxy URLs for path diversity.
    ///
    /// Each transport instance uses the proxy at its index. If the list is shorter
    /// than pool_size, remaining transports will use no proxy. If longer, extra proxies
    /// are ignored.
    pub proxy_urls: Vec<Option<String>>,
}

impl Default for SubmitBroadcasterConfig {
    fn default() -> Self {
        Self {
            pool_size: 3,
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
            expected_reject_patterns: vec![r"Duplicate clOrdID".to_string()],
            proxy_urls: vec![],
        }
    }
}

/// Transport client wrapper with health monitoring.
#[derive(Clone)]
struct TransportClient {
    /// Executor wrapped in Arc to enable cloning without requiring Clone on SubmitExecutor.
    ///
    /// BitmexHttpClient doesn't implement Clone, so we use reference counting to share
    /// the executor across multiple TransportClient clones.
    executor: Arc<dyn SubmitExecutor>,
    client_id: String,
    healthy: Arc<AtomicBool>,
    submit_count: Arc<AtomicU64>,
    error_count: Arc<AtomicU64>,
}

impl Debug for TransportClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportClient")
            .field("client_id", &self.client_id)
            .field("healthy", &self.healthy)
            .field("submit_count", &self.submit_count)
            .field("error_count", &self.error_count)
            .finish()
    }
}

impl TransportClient {
    fn new<E: SubmitExecutor + 'static>(executor: E, client_id: String) -> Self {
        Self {
            executor: Arc::new(executor),
            client_id,
            healthy: Arc::new(AtomicBool::new(true)),
            submit_count: Arc::new(AtomicU64::new(0)),
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

    fn get_submit_count(&self) -> u64 {
        self.submit_count.load(Ordering::Relaxed)
    }

    fn get_error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
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

    #[allow(clippy::too_many_arguments)]
    async fn submit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
        order_list_id: Option<OrderListId>,
        contingency_type: Option<ContingencyType>,
    ) -> anyhow::Result<OrderStatusReport> {
        self.submit_count.fetch_add(1, Ordering::Relaxed);

        match self
            .executor
            .submit_order(
                instrument_id,
                client_order_id,
                order_side,
                order_type,
                quantity,
                time_in_force,
                price,
                trigger_price,
                trigger_type,
                display_qty,
                post_only,
                reduce_only,
                order_list_id,
                contingency_type,
            )
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
}

/// Broadcasts submit requests to multiple HTTP clients for redundancy.
///
/// This broadcaster fans out submit requests to multiple pre-warmed HTTP clients
/// in parallel, short-circuits when the first successful acknowledgement is received,
/// and handles expected rejection patterns (duplicate clOrdID) with appropriate log levels.
#[cfg_attr(feature = "python", pyo3::pyclass)]
#[derive(Debug)]
pub struct SubmitBroadcaster {
    config: SubmitBroadcasterConfig,
    transports: Arc<Vec<TransportClient>>,
    health_check_task: Arc<RwLock<Option<JoinHandle<()>>>>,
    running: Arc<AtomicBool>,
    total_submits: Arc<AtomicU64>,
    successful_submits: Arc<AtomicU64>,
    failed_submits: Arc<AtomicU64>,
    expected_rejects: Arc<AtomicU64>,
}

impl SubmitBroadcaster {
    /// Creates a new [`SubmitBroadcaster`] with internal HTTP client pool.
    ///
    /// # Errors
    ///
    /// Returns an error if any HTTP client fails to initialize.
    pub fn new(config: SubmitBroadcasterConfig) -> anyhow::Result<Self> {
        let mut transports = Vec::with_capacity(config.pool_size);

        // Synthesize base_url when testnet is true but base_url is None
        let base_url = if config.testnet && config.base_url.is_none() {
            Some(BITMEX_HTTP_TESTNET_URL.to_string())
        } else {
            config.base_url.clone()
        };

        for i in 0..config.pool_size {
            // Assign proxy from config list, or None if index exceeds list length
            let proxy_url = config.proxy_urls.get(i).and_then(|p| p.clone());

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
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client {i}: {e}"))?;

            transports.push(TransportClient::new(client, format!("bitmex-submit-{i}")));
        }

        Ok(Self {
            config,
            transports: Arc::new(transports),
            health_check_task: Arc::new(RwLock::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            total_submits: Arc::new(AtomicU64::new(0)),
            successful_submits: Arc::new(AtomicU64::new(0)),
            failed_submits: Arc::new(AtomicU64::new(0)),
            expected_rejects: Arc::new(AtomicU64::new(0)),
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

                let tasks: Vec<_> = transports
                    .iter()
                    .map(|t| t.health_check(timeout_secs))
                    .collect();

                let results = future::join_all(tasks).await;
                let healthy_count = results.iter().filter(|&&r| r).count();

                tracing::debug!(
                    "Health check complete: {healthy_count}/{} clients healthy",
                    results.len()
                );
            }
        });

        *self.health_check_task.write().await = Some(task);

        tracing::info!(
            "SubmitBroadcaster started with {} clients",
            self.transports.len()
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

        tracing::info!("SubmitBroadcaster stopped");
    }

    async fn run_health_checks(&self) {
        let tasks: Vec<_> = self
            .transports
            .iter()
            .map(|t| t.health_check(self.config.health_check_timeout_secs))
            .collect();

        let results = future::join_all(tasks).await;
        let healthy_count = results.iter().filter(|&&r| r).count();

        tracing::debug!(
            "Health check complete: {healthy_count}/{} clients healthy",
            results.len()
        );
    }

    fn is_expected_reject(&self, error_message: &str) -> bool {
        self.config
            .expected_reject_patterns
            .iter()
            .any(|pattern| error_message.contains(pattern))
    }

    /// Processes submit request results, handling success and failures.
    ///
    /// This helper consolidates the common error handling loop used for submit broadcasts.
    async fn process_submit_results<T>(
        &self,
        mut handles: Vec<JoinHandle<(String, anyhow::Result<T>)>>,
        operation: &str,
        params: String,
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
                    self.successful_submits.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!("{} broadcast succeeded [{client_id}] {params}", operation,);
                    return Ok(result);
                }
                Ok((client_id, Err(e))) => {
                    let error_msg = e.to_string();

                    if self.is_expected_reject(&error_msg) {
                        self.expected_rejects.fetch_add(1, Ordering::Relaxed);
                        tracing::debug!(
                            "Expected {} rejection [{client_id}]: {error_msg} {params}",
                            operation.to_lowercase(),
                        );
                        errors.push(error_msg);
                    } else {
                        tracing::warn!(
                            "{} request failed [{client_id}]: {error_msg} {params}",
                            operation,
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
        self.failed_submits.fetch_add(1, Ordering::Relaxed);
        tracing::error!(
            "All {} requests failed: {errors:?} {params}",
            operation.to_lowercase(),
        );
        Err(anyhow::anyhow!(
            "All {} requests failed: {:?}",
            operation.to_lowercase(),
            errors
        ))
    }

    /// Broadcasts a submit request to all healthy clients in parallel.
    ///
    /// # Returns
    ///
    /// - `Ok(report)` if successfully submitted with a report.
    /// - `Err` if all requests failed.
    ///
    /// # Errors
    ///
    /// Returns an error if all submit requests fail or no healthy clients are available.
    #[allow(clippy::too_many_arguments)]
    pub async fn broadcast_submit(
        &self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
        order_list_id: Option<OrderListId>,
        contingency_type: Option<ContingencyType>,
        submit_tries: Option<usize>,
    ) -> anyhow::Result<OrderStatusReport> {
        self.total_submits.fetch_add(1, Ordering::Relaxed);

        let pool_size = self.config.pool_size;
        let actual_tries = if let Some(t) = submit_tries {
            if t > pool_size {
                // Use log macro for Python visibility for now
                log::warn!("submit_tries={t} exceeds pool_size={pool_size}, capping at pool_size");
            }
            std::cmp::min(t, pool_size)
        } else {
            pool_size
        };

        tracing::debug!(
            "Submit broadcast requested for client_order_id={client_order_id} (tries={actual_tries}/{pool_size})",
        );

        let healthy_transports: Vec<TransportClient> = self
            .transports
            .iter()
            .filter(|t| t.is_healthy())
            .take(actual_tries)
            .cloned()
            .collect();

        if healthy_transports.is_empty() {
            self.failed_submits.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!("No healthy transport clients available");
        }

        tracing::debug!(
            "Broadcasting submit to {} clients: client_order_id={client_order_id}, instrument_id={instrument_id}",
            healthy_transports.len(),
        );

        let mut handles = Vec::new();
        for (idx, transport) in healthy_transports.into_iter().enumerate() {
            // First client uses original ID, subsequent clients get suffix to avoid duplicates
            let modified_client_order_id = if idx == 0 {
                client_order_id
            } else {
                ClientOrderId::new(format!("{}-{}", client_order_id.as_str(), idx))
            };

            let handle = tokio::spawn(async move {
                let client_id = transport.client_id.clone();
                let result = transport
                    .submit_order(
                        instrument_id,
                        modified_client_order_id,
                        order_side,
                        order_type,
                        quantity,
                        time_in_force,
                        price,
                        trigger_price,
                        trigger_type,
                        display_qty,
                        post_only,
                        reduce_only,
                        order_list_id,
                        contingency_type,
                    )
                    .await;
                (client_id, result)
            });
            handles.push(handle);
        }

        self.process_submit_results(
            handles,
            "Submit",
            format!("(client_order_id={client_order_id:?})"),
        )
        .await
    }

    /// Gets broadcaster metrics.
    pub fn get_metrics(&self) -> BroadcasterMetrics {
        let healthy_clients = self.transports.iter().filter(|t| t.is_healthy()).count();
        let total_clients = self.transports.len();

        BroadcasterMetrics {
            total_submits: self.total_submits.load(Ordering::Relaxed),
            successful_submits: self.successful_submits.load(Ordering::Relaxed),
            failed_submits: self.failed_submits.load(Ordering::Relaxed),
            expected_rejects: self.expected_rejects.load(Ordering::Relaxed),
            healthy_clients,
            total_clients,
        }
    }

    /// Gets broadcaster metrics (async version for use within async context).
    pub async fn get_metrics_async(&self) -> BroadcasterMetrics {
        self.get_metrics()
    }

    /// Gets per-client statistics.
    pub fn get_client_stats(&self) -> Vec<ClientStats> {
        self.transports
            .iter()
            .map(|t| ClientStats {
                client_id: t.client_id.clone(),
                healthy: t.is_healthy(),
                submit_count: t.get_submit_count(),
                error_count: t.get_error_count(),
            })
            .collect()
    }

    /// Gets per-client statistics (async version for use within async context).
    pub async fn get_client_stats_async(&self) -> Vec<ClientStats> {
        self.get_client_stats()
    }

    /// Caches an instrument in all HTTP clients in the pool.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        for transport in self.transports.iter() {
            transport.executor.add_instrument(instrument.clone());
        }
    }

    #[must_use]
    pub fn clone_for_async(&self) -> Self {
        Self {
            config: self.config.clone(),
            transports: Arc::clone(&self.transports),
            health_check_task: Arc::clone(&self.health_check_task),
            running: Arc::clone(&self.running),
            total_submits: Arc::clone(&self.total_submits),
            successful_submits: Arc::clone(&self.successful_submits),
            failed_submits: Arc::clone(&self.failed_submits),
            expected_rejects: Arc::clone(&self.expected_rejects),
        }
    }

    #[cfg(test)]
    fn new_with_transports(
        config: SubmitBroadcasterConfig,
        transports: Vec<TransportClient>,
    ) -> Self {
        Self {
            config,
            transports: Arc::new(transports),
            health_check_task: Arc::new(RwLock::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            total_submits: Arc::new(AtomicU64::new(0)),
            successful_submits: Arc::new(AtomicU64::new(0)),
            failed_submits: Arc::new(AtomicU64::new(0)),
            expected_rejects: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Broadcaster metrics snapshot.
#[derive(Debug, Clone)]
pub struct BroadcasterMetrics {
    pub total_submits: u64,
    pub successful_submits: u64,
    pub failed_submits: u64,
    pub expected_rejects: u64,
    pub healthy_clients: usize,
    pub total_clients: usize,
}

/// Per-client statistics.
#[derive(Debug, Clone)]
pub struct ClientStats {
    pub client_id: String,
    pub healthy: bool,
    pub submit_count: u64,
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
            dyn Fn() -> Pin<Box<dyn Future<Output = anyhow::Result<OrderStatusReport>> + Send>>
                + Send
                + Sync,
        >,
    }

    impl MockExecutor {
        fn new<F, Fut>(handler: F) -> Self
        where
            F: Fn() -> Fut + Send + Sync + 'static,
            Fut: Future<Output = anyhow::Result<OrderStatusReport>> + Send + 'static,
        {
            Self {
                handler: Arc::new(move || Box::pin(handler())),
            }
        }
    }

    impl SubmitExecutor for MockExecutor {
        fn health_check(&self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        #[allow(clippy::too_many_arguments)]
        fn submit_order(
            &self,
            _instrument_id: InstrumentId,
            _client_order_id: ClientOrderId,
            _order_side: OrderSide,
            _order_type: OrderType,
            _quantity: Quantity,
            _time_in_force: TimeInForce,
            _price: Option<Price>,
            _trigger_price: Option<Price>,
            _trigger_type: Option<TriggerType>,
            _display_qty: Option<Quantity>,
            _post_only: bool,
            _reduce_only: bool,
            _order_list_id: Option<OrderListId>,
            _contingency_type: Option<ContingencyType>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<OrderStatusReport>> + Send + '_>> {
            (self.handler)()
        }

        fn add_instrument(&self, _instrument: InstrumentAny) {
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
            order_status: OrderStatus::Accepted,
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
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<OrderStatusReport>> + Send + 'static,
    {
        let executor = MockExecutor::new(handler);
        TransportClient::new(executor, client_id.to_string())
    }

    #[tokio::test]
    async fn test_broadcast_submit_immediate_success() {
        let report = create_test_report("ORDER-1");
        let report_clone = report.clone();

        let transports = vec![
            create_stub_transport("client-0", move || {
                let report = report_clone.clone();
                async move { Ok(report) }
            }),
            create_stub_transport("client-1", || async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                anyhow::bail!("Should be aborted")
            }),
        ];

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-123"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::new(100.0, 0),
                TimeInForce::Gtc,
                Some(Price::new(50000.0, 2)),
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        assert!(result.is_ok());
        let returned_report = result.unwrap();
        assert_eq!(returned_report.venue_order_id, report.venue_order_id);

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.successful_submits, 1);
        assert_eq!(metrics.failed_submits, 0);
        assert_eq!(metrics.total_submits, 1);
    }

    #[tokio::test]
    async fn test_broadcast_submit_duplicate_clordid_expected() {
        let transports = vec![
            create_stub_transport("client-0", || async { anyhow::bail!("Duplicate clOrdID") }),
            create_stub_transport("client-1", || async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                anyhow::bail!("Should be aborted")
            }),
        ];

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-123"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::new(100.0, 0),
                TimeInForce::Gtc,
                Some(Price::new(50000.0, 2)),
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        assert!(result.is_err());

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.expected_rejects, 1);
        assert_eq!(metrics.successful_submits, 0);
        assert_eq!(metrics.failed_submits, 1);
    }

    #[tokio::test]
    async fn test_broadcast_submit_all_failures() {
        let transports = vec![
            create_stub_transport("client-0", || async { anyhow::bail!("502 Bad Gateway") }),
            create_stub_transport("client-1", || async { anyhow::bail!("Connection refused") }),
        ];

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-456"),
                OrderSide::Sell,
                OrderType::Market,
                Quantity::new(50.0, 0),
                TimeInForce::Ioc,
                None,
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("All submit requests failed")
        );

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.failed_submits, 1);
        assert_eq!(metrics.successful_submits, 0);
    }

    #[tokio::test]
    async fn test_broadcast_submit_no_healthy_clients() {
        let transport =
            create_stub_transport("client-0", || async { Ok(create_test_report("ORDER-1")) });
        transport.healthy.store(false, Ordering::Relaxed);

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, vec![transport]);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-789"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::new(100.0, 0),
                TimeInForce::Gtc,
                Some(Price::new(50000.0, 2)),
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No healthy transport clients available")
        );

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.failed_submits, 1);
    }

    #[tokio::test]
    async fn test_default_config() {
        let config = SubmitBroadcasterConfig {
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            ..Default::default()
        };

        let broadcaster = SubmitBroadcaster::new(config);
        assert!(broadcaster.is_ok());

        let broadcaster = broadcaster.unwrap();
        let metrics = broadcaster.get_metrics_async().await;

        // Default pool_size is 3
        assert_eq!(metrics.total_clients, 3);
    }

    #[tokio::test]
    async fn test_broadcaster_lifecycle() {
        let config = SubmitBroadcasterConfig {
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
            health_check_timeout_secs: 1,
            expected_reject_patterns: vec![],
            proxy_urls: vec![],
        };

        let broadcaster = SubmitBroadcaster::new(config).unwrap();

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
    async fn test_broadcast_submit_metrics_increment() {
        let report = create_test_report("ORDER-1");
        let report_clone = report.clone();

        let transports = vec![create_stub_transport("client-0", move || {
            let report = report_clone.clone();
            async move { Ok(report) }
        })];

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let _ = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-123"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::new(100.0, 0),
                TimeInForce::Gtc,
                Some(Price::new(50000.0, 2)),
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.total_submits, 1);
        assert_eq!(metrics.successful_submits, 1);
        assert_eq!(metrics.failed_submits, 0);
    }

    #[tokio::test]
    async fn test_broadcaster_creation_with_pool() {
        let config = SubmitBroadcasterConfig {
            pool_size: 4,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            ..Default::default()
        };

        let broadcaster = SubmitBroadcaster::new(config);
        assert!(broadcaster.is_ok());

        let broadcaster = broadcaster.unwrap();
        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.total_clients, 4);
    }

    #[tokio::test]
    async fn test_client_stats_collection() {
        let report = create_test_report("ORDER-1");
        let report_clone = report.clone();

        let transports = vec![
            create_stub_transport("client-0", move || {
                let report = report_clone.clone();
                async move { Ok(report) }
            }),
            create_stub_transport("client-1", || async { anyhow::bail!("Connection error") }),
        ];

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let _ = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-123"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::new(100.0, 0),
                TimeInForce::Gtc,
                Some(Price::new(50000.0, 2)),
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        let stats = broadcaster.get_client_stats_async().await;
        assert_eq!(stats.len(), 2);

        let client0 = stats.iter().find(|s| s.client_id == "client-0").unwrap();
        assert_eq!(client0.submit_count, 1);
        assert_eq!(client0.error_count, 0);

        let client1 = stats.iter().find(|s| s.client_id == "client-1").unwrap();
        assert_eq!(client1.submit_count, 1);
        assert_eq!(client1.error_count, 1);
    }

    #[tokio::test]
    async fn test_testnet_config_sets_base_url() {
        let config = SubmitBroadcasterConfig {
            pool_size: 1,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            testnet: true,
            base_url: None,
            ..Default::default()
        };

        let broadcaster = SubmitBroadcaster::new(config);
        assert!(broadcaster.is_ok());
    }

    #[tokio::test]
    async fn test_clone_for_async() {
        let config = SubmitBroadcasterConfig {
            pool_size: 1,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            ..Default::default()
        };

        let broadcaster = SubmitBroadcaster::new(config).unwrap();
        let cloned = broadcaster.clone_for_async();

        // Verify they share the same atomics
        broadcaster.total_submits.fetch_add(1, Ordering::Relaxed);
        assert_eq!(cloned.total_submits.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_pattern_matching() {
        let config = SubmitBroadcasterConfig {
            expected_reject_patterns: vec![
                "Duplicate clOrdID".to_string(),
                "Order already exists".to_string(),
            ],
            ..Default::default()
        };

        let broadcaster = SubmitBroadcaster::new_with_transports(config, vec![]);

        assert!(broadcaster.is_expected_reject("Error: Duplicate clOrdID for order"));
        assert!(broadcaster.is_expected_reject("Order already exists in system"));
        assert!(!broadcaster.is_expected_reject("Rate limit exceeded"));
        assert!(!broadcaster.is_expected_reject("Internal server error"));
    }

    #[tokio::test]
    async fn test_submit_metrics_with_mixed_responses() {
        let report = create_test_report("ORDER-1");
        let report_clone = report.clone();

        let transports = vec![
            create_stub_transport("client-0", move || {
                let report = report_clone.clone();
                async move { Ok(report) }
            }),
            create_stub_transport("client-1", || async { anyhow::bail!("Timeout") }),
        ];

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-123"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::new(100.0, 0),
                TimeInForce::Gtc,
                Some(Price::new(50000.0, 2)),
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        assert!(result.is_ok());

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.total_submits, 1);
        assert_eq!(metrics.successful_submits, 1);
        assert_eq!(metrics.failed_submits, 0);
    }

    #[tokio::test]
    async fn test_metrics_initialization_and_health() {
        let config = SubmitBroadcasterConfig {
            pool_size: 2,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            ..Default::default()
        };

        let broadcaster = SubmitBroadcaster::new(config).unwrap();
        let metrics = broadcaster.get_metrics_async().await;

        assert_eq!(metrics.total_submits, 0);
        assert_eq!(metrics.successful_submits, 0);
        assert_eq!(metrics.failed_submits, 0);
        assert_eq!(metrics.expected_rejects, 0);
        assert_eq!(metrics.total_clients, 2);
        assert_eq!(metrics.healthy_clients, 2);
    }

    #[tokio::test]
    async fn test_health_check_task_lifecycle() {
        let config = SubmitBroadcasterConfig {
            pool_size: 2,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url: Some("https://test.example.com".to_string()),
            health_check_interval_secs: 1,
            ..Default::default()
        };

        let broadcaster = SubmitBroadcaster::new(config).unwrap();

        // Start should spawn health check task
        broadcaster.start().await.unwrap();
        assert!(broadcaster.running.load(Ordering::Relaxed));
        assert!(
            broadcaster
                .health_check_task
                .read()
                .await
                .as_ref()
                .is_some()
        );

        // Wait a bit to let health checks run
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Stop should clean up task
        broadcaster.stop().await;
        assert!(!broadcaster.running.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_expected_reject_pattern_comprehensive() {
        let transports = vec![
            create_stub_transport("client-0", || async {
                anyhow::bail!("Duplicate clOrdID: O-123 already exists")
            }),
            create_stub_transport("client-1", || async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                anyhow::bail!("Should be aborted")
            }),
        ];

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-123"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::new(100.0, 0),
                TimeInForce::Gtc,
                Some(Price::new(50000.0, 2)),
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        // All failed with expected reject
        assert!(result.is_err());

        let metrics = broadcaster.get_metrics_async().await;
        assert_eq!(metrics.expected_rejects, 1);
        assert_eq!(metrics.failed_submits, 1);
        assert_eq!(metrics.successful_submits, 0);
    }

    #[tokio::test]
    async fn test_client_order_id_suffix_for_multiple_clients() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct CaptureExecutor {
            captured_ids: Arc<Mutex<Vec<String>>>,
            report: OrderStatusReport,
        }

        impl SubmitExecutor for CaptureExecutor {
            fn health_check(
                &self,
            ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
                Box::pin(async { Ok(()) })
            }

            #[allow(clippy::too_many_arguments)]
            fn submit_order(
                &self,
                _instrument_id: InstrumentId,
                client_order_id: ClientOrderId,
                _order_side: OrderSide,
                _order_type: OrderType,
                _quantity: Quantity,
                _time_in_force: TimeInForce,
                _price: Option<Price>,
                _trigger_price: Option<Price>,
                _trigger_type: Option<TriggerType>,
                _display_qty: Option<Quantity>,
                _post_only: bool,
                _reduce_only: bool,
                _order_list_id: Option<OrderListId>,
                _contingency_type: Option<ContingencyType>,
            ) -> Pin<Box<dyn Future<Output = anyhow::Result<OrderStatusReport>> + Send + '_>>
            {
                // Capture the client_order_id
                self.captured_ids
                    .lock()
                    .unwrap()
                    .push(client_order_id.as_str().to_string());
                let report = self.report.clone();
                Box::pin(async move { Ok(report) })
            }

            fn add_instrument(&self, _instrument: InstrumentAny) {}
        }

        let captured_ids = Arc::new(Mutex::new(Vec::new()));
        let report = create_test_report("ORDER-1");

        let transports = vec![
            TransportClient::new(
                CaptureExecutor {
                    captured_ids: Arc::clone(&captured_ids),
                    report: report.clone(),
                },
                "client-0".to_string(),
            ),
            TransportClient::new(
                CaptureExecutor {
                    captured_ids: Arc::clone(&captured_ids),
                    report: report.clone(),
                },
                "client-1".to_string(),
            ),
            TransportClient::new(
                CaptureExecutor {
                    captured_ids: Arc::clone(&captured_ids),
                    report: report.clone(),
                },
                "client-2".to_string(),
            ),
        ];

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-123"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::new(100.0, 0),
                TimeInForce::Gtc,
                Some(Price::new(50000.0, 2)),
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        assert!(result.is_ok());

        // Check captured client_order_ids
        let ids = captured_ids.lock().unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0], "O-123"); // First client gets original ID
        assert_eq!(ids[1], "O-123-1"); // Second client gets suffix -1
        assert_eq!(ids[2], "O-123-2"); // Third client gets suffix -2
    }

    #[tokio::test]
    async fn test_client_order_id_suffix_with_partial_failure() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct CaptureAndFailExecutor {
            captured_ids: Arc<Mutex<Vec<String>>>,
            should_succeed: bool,
        }

        impl SubmitExecutor for CaptureAndFailExecutor {
            fn health_check(
                &self,
            ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
                Box::pin(async { Ok(()) })
            }

            #[allow(clippy::too_many_arguments)]
            fn submit_order(
                &self,
                _instrument_id: InstrumentId,
                client_order_id: ClientOrderId,
                _order_side: OrderSide,
                _order_type: OrderType,
                _quantity: Quantity,
                _time_in_force: TimeInForce,
                _price: Option<Price>,
                _trigger_price: Option<Price>,
                _trigger_type: Option<TriggerType>,
                _display_qty: Option<Quantity>,
                _post_only: bool,
                _reduce_only: bool,
                _order_list_id: Option<OrderListId>,
                _contingency_type: Option<ContingencyType>,
            ) -> Pin<Box<dyn Future<Output = anyhow::Result<OrderStatusReport>> + Send + '_>>
            {
                // Capture the client_order_id
                self.captured_ids
                    .lock()
                    .unwrap()
                    .push(client_order_id.as_str().to_string());
                let should_succeed = self.should_succeed;
                Box::pin(async move {
                    if should_succeed {
                        Ok(create_test_report("ORDER-1"))
                    } else {
                        anyhow::bail!("Network error")
                    }
                })
            }

            fn add_instrument(&self, _instrument: InstrumentAny) {}
        }

        let captured_ids = Arc::new(Mutex::new(Vec::new()));

        let transports = vec![
            TransportClient::new(
                CaptureAndFailExecutor {
                    captured_ids: Arc::clone(&captured_ids),
                    should_succeed: false,
                },
                "client-0".to_string(),
            ),
            TransportClient::new(
                CaptureAndFailExecutor {
                    captured_ids: Arc::clone(&captured_ids),
                    should_succeed: true,
                },
                "client-1".to_string(),
            ),
        ];

        let config = SubmitBroadcasterConfig::default();
        let broadcaster = SubmitBroadcaster::new_with_transports(config, transports);

        let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
        let result = broadcaster
            .broadcast_submit(
                instrument_id,
                ClientOrderId::from("O-456"),
                OrderSide::Sell,
                OrderType::Market,
                Quantity::new(50.0, 0),
                TimeInForce::Ioc,
                None,
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .await;

        assert!(result.is_ok());

        // Check that both clients received unique client_order_ids
        let ids = captured_ids.lock().unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], "O-456"); // First client gets original ID
        assert_eq!(ids[1], "O-456-1"); // Second client gets suffix -1
    }

    #[tokio::test]
    async fn test_proxy_urls_populated_from_config() {
        let config = SubmitBroadcasterConfig {
            pool_size: 3,
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            proxy_urls: vec![
                Some("http://proxy1:8080".to_string()),
                Some("http://proxy2:8080".to_string()),
                Some("http://proxy3:8080".to_string()),
            ],
            ..Default::default()
        };

        assert_eq!(config.proxy_urls.len(), 3);
        assert_eq!(config.proxy_urls[0], Some("http://proxy1:8080".to_string()));
        assert_eq!(config.proxy_urls[1], Some("http://proxy2:8080".to_string()));
        assert_eq!(config.proxy_urls[2], Some("http://proxy3:8080".to_string()));
    }
}
