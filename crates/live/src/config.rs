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

//! Configuration types for live Nautilus system nodes.

use std::{collections::HashMap, time::Duration};

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig,
};
use nautilus_core::UUID4;
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::identifiers::TraderId;
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_system::config::{NautilusKernelConfig, StreamingConfig};
use serde::{Deserialize, Serialize};

/// Configuration for live data engines.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct LiveDataEngineConfig {
    /// The queue size for the engine's internal queue buffers.
    #[builder(default = 100_000)]
    pub qsize: u32,
}

impl Default for LiveDataEngineConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl From<LiveDataEngineConfig> for DataEngineConfig {
    fn from(_config: LiveDataEngineConfig) -> Self {
        Self::default()
    }
}

/// Configuration for live risk engines.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct LiveRiskEngineConfig {
    /// The queue size for the engine's internal queue buffers.
    #[builder(default = 100_000)]
    pub qsize: u32,
}

impl Default for LiveRiskEngineConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl From<LiveRiskEngineConfig> for RiskEngineConfig {
    fn from(_config: LiveRiskEngineConfig) -> Self {
        Self::default()
    }
}

/// Configuration for live execution engines.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, bon::Builder)]
pub struct LiveExecEngineConfig {
    /// If reconciliation is active at start-up.
    #[builder(default = true)]
    pub reconciliation: bool,
    /// The delay (seconds) before starting reconciliation at startup.
    #[builder(default = 10.0)]
    pub reconciliation_startup_delay_secs: f64,
    /// The maximum lookback minutes to reconcile state for.
    pub reconciliation_lookback_mins: Option<u32>,
    /// Specific instrument IDs to reconcile (if None, reconciles all).
    pub reconciliation_instrument_ids: Option<Vec<String>>,
    /// If unclaimed order events with an EXTERNAL strategy ID should be filtered/dropped.
    #[builder(default)]
    pub filter_unclaimed_external_orders: bool,
    /// If position status reports are filtered from reconciliation.
    #[builder(default)]
    pub filter_position_reports: bool,
    /// Client order IDs to filter from reconciliation.
    pub filtered_client_order_ids: Option<Vec<String>>,
    /// If MARKET order events will be generated during reconciliation to align discrepancies.
    #[builder(default = true)]
    pub generate_missing_orders: bool,
    /// The interval (milliseconds) between checking whether in-flight orders have exceeded their threshold.
    #[builder(default = 2_000)]
    pub inflight_check_interval_ms: u32,
    /// The threshold (milliseconds) beyond which an in-flight order's status is checked with the venue.
    #[builder(default = 5_000)]
    pub inflight_check_threshold_ms: u32,
    /// The number of retry attempts for verifying in-flight order status.
    #[builder(default = 5)]
    pub inflight_check_retries: u32,
    /// The interval (seconds) between checks for open orders at the venue.
    pub open_check_interval_secs: Option<f64>,
    /// The lookback minutes for open order checks.
    /// When `None`, the check is unbounded (no time filter).
    pub open_check_lookback_mins: Option<u32>,
    /// The minimum elapsed time (milliseconds) since an order update before acting on discrepancies.
    #[builder(default = 5_000)]
    pub open_check_threshold_ms: u32,
    /// The number of retries for missing open orders.
    #[builder(default = 5)]
    pub open_check_missing_retries: u32,
    /// If the `check_open_orders` requests only currently open orders from the venue.
    #[builder(default = true)]
    pub open_check_open_only: bool,
    /// The maximum number of single-order queries per consistency check cycle.
    #[builder(default = 5)]
    pub max_single_order_queries_per_cycle: u32,
    /// The delay (milliseconds) between consecutive single-order queries.
    #[builder(default = 100)]
    pub single_order_query_delay_ms: u32,
    /// The interval (seconds) between checks for open positions at the venue.
    pub position_check_interval_secs: Option<f64>,
    /// The lookback minutes for position consistency checks.
    #[builder(default = 60)]
    pub position_check_lookback_mins: u32,
    /// The minimum elapsed time (milliseconds) since a position update before acting on discrepancies.
    #[builder(default = 60_000)]
    pub position_check_threshold_ms: u32,
    /// The maximum number of reconciliation attempts for a position discrepancy.
    #[builder(default = 3)]
    pub position_check_retries: u32,
    /// The interval (minutes) between purging closed orders from the in-memory cache.
    pub purge_closed_orders_interval_mins: Option<u32>,
    /// The time buffer (minutes) before closed orders can be purged.
    pub purge_closed_orders_buffer_mins: Option<u32>,
    /// The interval (minutes) between purging closed positions from the in-memory cache.
    pub purge_closed_positions_interval_mins: Option<u32>,
    /// The time buffer (minutes) before closed positions can be purged.
    pub purge_closed_positions_buffer_mins: Option<u32>,
    /// The interval (minutes) between purging account events from the in-memory cache.
    pub purge_account_events_interval_mins: Option<u32>,
    /// The time buffer (minutes) before account events can be purged.
    pub purge_account_events_lookback_mins: Option<u32>,
    /// If purge operations should also delete from the backing database.
    #[builder(default)]
    pub purge_from_database: bool,
    /// The interval (seconds) between auditing own books against public order books.
    pub own_books_audit_interval_secs: Option<f64>,
    /// If the engine should gracefully shutdown when queue processing encounters unexpected errors.
    #[builder(default)]
    pub graceful_shutdown_on_error: bool,
    /// The queue size for the engine's internal queue buffers.
    #[builder(default = 100_000)]
    pub qsize: u32,
}

impl Default for LiveExecEngineConfig {
    fn default() -> Self {
        Self {
            open_check_lookback_mins: Some(60),
            ..Self::builder().build()
        }
    }
}

impl From<LiveExecEngineConfig> for ExecutionEngineConfig {
    fn from(config: LiveExecEngineConfig) -> Self {
        Self {
            purge_closed_orders_interval_mins: config.purge_closed_orders_interval_mins,
            purge_closed_orders_buffer_mins: config.purge_closed_orders_buffer_mins,
            purge_closed_positions_interval_mins: config.purge_closed_positions_interval_mins,
            purge_closed_positions_buffer_mins: config.purge_closed_positions_buffer_mins,
            purge_account_events_interval_mins: config.purge_account_events_interval_mins,
            purge_account_events_lookback_mins: config.purge_account_events_lookback_mins,
            purge_from_database: config.purge_from_database,
            ..Self::default()
        }
    }
}

/// Configuration for live client message routing.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, bon::Builder)]
pub struct RoutingConfig {
    /// If the client should be registered as the default routing client.
    #[builder(default)]
    pub default: bool,
    /// The venues to register for routing.
    pub venues: Option<Vec<String>>,
}

/// Configuration for instrument providers.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct InstrumentProviderConfig {
    /// Whether to load all instruments on startup.
    #[builder(default)]
    pub load_all: bool,
    /// Whether to load instrument IDs only.
    #[builder(default = true)]
    pub load_ids: bool,
    /// Filters for loading specific instruments.
    #[builder(default)]
    pub filters: HashMap<String, String>,
}

impl Default for InstrumentProviderConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Configuration for live data clients.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, bon::Builder)]
pub struct LiveDataClientConfig {
    /// If `DataClient` will emit bar updates when a new bar opens.
    #[builder(default)]
    pub handle_revised_bars: bool,
    /// The client's instrument provider configuration.
    #[builder(default)]
    pub instrument_provider: InstrumentProviderConfig,
    /// The client's message routing configuration.
    #[builder(default)]
    pub routing: RoutingConfig,
}

/// Configuration for live execution clients.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, bon::Builder)]
pub struct LiveExecClientConfig {
    /// The client's instrument provider configuration.
    #[builder(default)]
    pub instrument_provider: InstrumentProviderConfig,
    /// The client's message routing configuration.
    #[builder(default)]
    pub routing: RoutingConfig,
}

/// Configuration for live Nautilus system nodes.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
#[derive(Debug, Clone, bon::Builder)]
pub struct LiveNodeConfig {
    /// The trading environment.
    #[builder(default = Environment::Live)]
    pub environment: Environment,
    /// The trader ID for the node.
    #[builder(default = TraderId::from("TRADER-001"))]
    pub trader_id: TraderId,
    /// If trading strategy state should be loaded from the database on start.
    #[builder(default)]
    pub load_state: bool,
    /// If trading strategy state should be saved to the database on stop.
    #[builder(default)]
    pub save_state: bool,
    /// The logging configuration for the kernel.
    #[builder(default)]
    pub logging: LoggerConfig,
    /// The unique instance identifier for the kernel
    pub instance_id: Option<UUID4>,
    /// The timeout for all clients to connect and initialize.
    #[builder(default = Duration::from_secs(60))]
    pub timeout_connection: Duration,
    /// The timeout for execution state to reconcile.
    #[builder(default = Duration::from_secs(30))]
    pub timeout_reconciliation: Duration,
    /// The timeout for portfolio to initialize margins and unrealized pnls.
    #[builder(default = Duration::from_secs(10))]
    pub timeout_portfolio: Duration,
    /// The timeout for all engine clients to disconnect.
    #[builder(default = Duration::from_secs(10))]
    pub timeout_disconnection: Duration,
    /// The delay after stopping the node to await residual events before final shutdown.
    #[builder(default = Duration::from_secs(10))]
    pub delay_post_stop: Duration,
    /// The timeout to await pending tasks cancellation during shutdown.
    #[builder(default = Duration::from_secs(5))]
    pub timeout_shutdown: Duration,
    /// The cache configuration.
    pub cache: Option<CacheConfig>,
    /// The message bus configuration.
    pub msgbus: Option<MessageBusConfig>,
    /// The portfolio configuration.
    pub portfolio: Option<PortfolioConfig>,
    /// The configuration for streaming to feather files.
    pub streaming: Option<StreamingConfig>,
    /// The live data engine configuration.
    #[builder(default)]
    pub data_engine: LiveDataEngineConfig,
    /// The live risk engine configuration.
    #[builder(default)]
    pub risk_engine: LiveRiskEngineConfig,
    /// The live execution engine configuration.
    #[builder(default)]
    pub exec_engine: LiveExecEngineConfig,
    /// The data client configurations.
    #[builder(default)]
    pub data_clients: HashMap<String, LiveDataClientConfig>,
    /// The execution client configurations.
    #[builder(default)]
    pub exec_clients: HashMap<String, LiveExecClientConfig>,
}

impl Default for LiveNodeConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl NautilusKernelConfig for LiveNodeConfig {
    fn environment(&self) -> Environment {
        self.environment
    }

    fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    fn load_state(&self) -> bool {
        self.load_state
    }

    fn save_state(&self) -> bool {
        self.save_state
    }

    fn logging(&self) -> LoggerConfig {
        self.logging.clone()
    }

    fn instance_id(&self) -> Option<UUID4> {
        self.instance_id
    }

    fn timeout_connection(&self) -> Duration {
        self.timeout_connection
    }

    fn timeout_reconciliation(&self) -> Duration {
        self.timeout_reconciliation
    }

    fn timeout_portfolio(&self) -> Duration {
        self.timeout_portfolio
    }

    fn timeout_disconnection(&self) -> Duration {
        self.timeout_disconnection
    }

    fn delay_post_stop(&self) -> Duration {
        self.delay_post_stop
    }

    fn timeout_shutdown(&self) -> Duration {
        self.timeout_shutdown
    }

    fn cache(&self) -> Option<CacheConfig> {
        self.cache.clone()
    }

    fn msgbus(&self) -> Option<MessageBusConfig> {
        self.msgbus.clone()
    }

    fn data_engine(&self) -> Option<DataEngineConfig> {
        Some(self.data_engine.clone().into())
    }

    fn risk_engine(&self) -> Option<RiskEngineConfig> {
        Some(self.risk_engine.clone().into())
    }

    fn exec_engine(&self) -> Option<ExecutionEngineConfig> {
        Some(self.exec_engine.clone().into())
    }

    fn portfolio(&self) -> Option<PortfolioConfig> {
        self.portfolio
    }

    fn streaming(&self) -> Option<StreamingConfig> {
        self.streaming.clone()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_trading_node_config_default() {
        let config = LiveNodeConfig::default();

        assert_eq!(config.environment, Environment::Live);
        assert_eq!(config.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(config.data_engine.qsize, 100_000);
        assert_eq!(config.risk_engine.qsize, 100_000);
        assert_eq!(config.exec_engine.qsize, 100_000);
        assert!(config.exec_engine.reconciliation);
        assert!(!config.exec_engine.filter_unclaimed_external_orders);
        assert!(config.data_clients.is_empty());
        assert!(config.exec_clients.is_empty());
    }

    #[rstest]
    fn test_trading_node_config_as_kernel_config() {
        let config = LiveNodeConfig::default();

        assert_eq!(config.environment(), Environment::Live);
        assert_eq!(config.trader_id(), TraderId::from("TRADER-001"));
        assert!(config.data_engine().is_some());
        assert!(config.risk_engine().is_some());
        assert!(config.exec_engine().is_some());
        assert!(!config.load_state());
        assert!(!config.save_state());
    }

    #[rstest]
    fn test_live_exec_engine_config_defaults() {
        let config = LiveExecEngineConfig::default();

        assert!(config.reconciliation);
        assert_eq!(config.reconciliation_startup_delay_secs, 10.0);
        assert_eq!(config.reconciliation_lookback_mins, None);
        assert_eq!(config.reconciliation_instrument_ids, None);
        assert_eq!(config.filtered_client_order_ids, None);
        assert!(!config.filter_unclaimed_external_orders);
        assert!(!config.filter_position_reports);
        assert!(config.generate_missing_orders);
        assert_eq!(config.inflight_check_interval_ms, 2_000);
        assert_eq!(config.inflight_check_threshold_ms, 5_000);
        assert_eq!(config.inflight_check_retries, 5);
        assert_eq!(config.open_check_threshold_ms, 5_000);
        assert_eq!(config.open_check_lookback_mins, Some(60));
        assert_eq!(config.open_check_missing_retries, 5);
        assert!(config.open_check_open_only);
        assert_eq!(config.position_check_retries, 3);
        assert!(!config.purge_from_database);
        assert!(!config.graceful_shutdown_on_error);
        assert_eq!(config.qsize, 100_000);
        assert_eq!(config.reconciliation_startup_delay_secs, 10.0);
    }

    #[rstest]
    fn test_routing_config_default() {
        let config = RoutingConfig::default();

        assert!(!config.default);
        assert_eq!(config.venues, None);
    }

    #[rstest]
    fn test_live_data_client_config_default() {
        let config = LiveDataClientConfig::default();

        assert!(!config.handle_revised_bars);
        assert!(!config.instrument_provider.load_all);
        assert!(config.instrument_provider.load_ids);
        assert!(!config.routing.default);
    }
}
