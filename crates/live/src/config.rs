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

//! Configuration types for live Nautilus system nodes.

use std::collections::HashMap;

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig,
};
use nautilus_core::UUID4;
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::identifiers::TraderId;
use nautilus_persistence::config::StreamingConfig;
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_system::config::NautilusKernelConfig;
use serde::{Deserialize, Serialize};

/// Configuration for live data engines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveDataEngineConfig {
    /// The queue size for the engine's internal queue buffers.
    pub qsize: u32,
}

impl Default for LiveDataEngineConfig {
    fn default() -> Self {
        Self { qsize: 100_000 }
    }
}

impl From<LiveDataEngineConfig> for DataEngineConfig {
    fn from(_config: LiveDataEngineConfig) -> Self {
        Self::default()
    }
}

/// Configuration for live risk engines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveRiskEngineConfig {
    /// The queue size for the engine's internal queue buffers.
    pub qsize: u32,
}

impl Default for LiveRiskEngineConfig {
    fn default() -> Self {
        Self { qsize: 100_000 }
    }
}

impl From<LiveRiskEngineConfig> for RiskEngineConfig {
    fn from(_config: LiveRiskEngineConfig) -> Self {
        Self::default()
    }
}

/// Configuration for live execution engines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveExecEngineConfig {
    /// If reconciliation is active at start-up.
    pub reconciliation: bool,
    /// The maximum lookback minutes to reconcile state for.
    pub reconciliation_lookback_mins: Option<u32>,
    /// If unclaimed order events with an EXTERNAL strategy ID should be filtered/dropped.
    pub filter_unclaimed_external_orders: bool,
    /// If position status reports are filtered from reconciliation.
    pub filter_position_reports: bool,
    /// If MARKET order events will be generated during reconciliation to align discrepancies.
    pub generate_missing_orders: bool,
    /// The interval (milliseconds) between checking whether in-flight orders have exceeded their threshold.
    pub inflight_check_interval_ms: u32,
    /// The threshold (milliseconds) beyond which an in-flight order's status is checked with the venue.
    pub inflight_check_threshold_ms: u32,
    /// The number of retry attempts for verifying in-flight order status.
    pub inflight_check_retries: u32,
    /// The interval (seconds) between auditing own books against public order books.
    pub own_books_audit_interval_secs: Option<f64>,
    /// The interval (seconds) between checks for open orders at the venue.
    pub open_check_interval_secs: Option<f64>,
    /// If the `check_open_orders` requests only currently open orders from the venue.
    pub open_check_open_only: bool,
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
    /// The queue size for the engine's internal queue buffers.
    pub qsize: u32,
}

impl Default for LiveExecEngineConfig {
    fn default() -> Self {
        Self {
            reconciliation: true,
            reconciliation_lookback_mins: None,
            filter_unclaimed_external_orders: false,
            filter_position_reports: false,
            generate_missing_orders: true,
            inflight_check_interval_ms: 2_000,
            inflight_check_threshold_ms: 5_000,
            inflight_check_retries: 5,
            own_books_audit_interval_secs: None,
            open_check_interval_secs: None,
            open_check_open_only: true,
            purge_closed_orders_interval_mins: None,
            purge_closed_orders_buffer_mins: None,
            purge_closed_positions_interval_mins: None,
            purge_closed_positions_buffer_mins: None,
            purge_account_events_interval_mins: None,
            purge_account_events_lookback_mins: None,
            qsize: 100_000,
        }
    }
}

impl From<LiveExecEngineConfig> for ExecutionEngineConfig {
    fn from(_config: LiveExecEngineConfig) -> Self {
        Self::default()
    }
}

/// Configuration for live client message routing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// If the client should be registered as the default routing client.
    pub default: bool,
    /// The venues to register for routing.
    pub venues: Option<Vec<String>>,
}

/// Configuration for instrument providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentProviderConfig {
    /// Whether to load all instruments on startup.
    pub load_all: bool,
    /// Whether to load instrument IDs only.
    pub load_ids: bool,
    /// Filters for loading specific instruments.
    pub filters: HashMap<String, String>,
}

impl Default for InstrumentProviderConfig {
    fn default() -> Self {
        Self {
            load_all: false,
            load_ids: true,
            filters: HashMap::new(),
        }
    }
}

/// Configuration for live data clients.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LiveDataClientConfig {
    /// If `DataClient` will emit bar updates when a new bar opens.
    pub handle_revised_bars: bool,
    /// The client's instrument provider configuration.
    pub instrument_provider: InstrumentProviderConfig,
    /// The client's message routing configuration.
    pub routing: RoutingConfig,
}

/// Configuration for live execution clients.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LiveExecClientConfig {
    /// The client's instrument provider configuration.
    pub instrument_provider: InstrumentProviderConfig,
    /// The client's message routing configuration.
    pub routing: RoutingConfig,
}

/// Configuration for live Nautilus system nodes.
#[derive(Debug, Clone)]
pub struct LiveNodeConfig {
    /// The trading environment.
    pub environment: Environment,
    /// The trader ID for the node.
    pub trader_id: TraderId,
    /// If trading strategy state should be loaded from the database on start.
    pub load_state: bool,
    /// If trading strategy state should be saved to the database on stop.
    pub save_state: bool,
    /// The logging configuration for the kernel.
    pub logging: LoggerConfig,
    /// The unique instance identifier for the kernel
    pub instance_id: Option<UUID4>,
    /// The timeout (seconds) for all clients to connect and initialize.
    pub timeout_connection: u32,
    /// The timeout (seconds) for execution state to reconcile.
    pub timeout_reconciliation: u32,
    /// The timeout (seconds) for portfolio to initialize margins and unrealized pnls.
    pub timeout_portfolio: u32,
    /// The timeout (seconds) for all engine clients to disconnect.
    pub timeout_disconnection: u32,
    /// The timeout (seconds) after stopping the node to await residual events before final shutdown.
    pub timeout_post_stop: u32,
    /// The timeout (seconds) to await pending tasks cancellation during shutdown.
    pub timeout_shutdown: u32,
    /// The cache configuration.
    pub cache: Option<CacheConfig>,
    /// The message bus configuration.
    pub msgbus: Option<MessageBusConfig>,
    /// The portfolio configuration.
    pub portfolio: Option<PortfolioConfig>,
    /// The configuration for streaming to feather files.
    pub streaming: Option<StreamingConfig>,
    /// The live data engine configuration.
    pub data_engine: LiveDataEngineConfig,
    /// The live risk engine configuration.
    pub risk_engine: LiveRiskEngineConfig,
    /// The live execution engine configuration.
    pub exec_engine: LiveExecEngineConfig,
    /// The data client configurations.
    pub data_clients: HashMap<String, LiveDataClientConfig>,
    /// The execution client configurations.
    pub exec_clients: HashMap<String, LiveExecClientConfig>,
}

impl Default for LiveNodeConfig {
    fn default() -> Self {
        Self {
            environment: Environment::Live,
            trader_id: TraderId::from("TRADER-001"),
            load_state: false,
            save_state: false,
            logging: LoggerConfig::default(),
            instance_id: None,
            timeout_connection: 60,
            timeout_reconciliation: 30,
            timeout_portfolio: 10,
            timeout_disconnection: 10,
            timeout_post_stop: 10,
            timeout_shutdown: 5,
            cache: None,
            msgbus: None,
            portfolio: None,
            streaming: None,
            data_engine: LiveDataEngineConfig::default(),
            risk_engine: LiveRiskEngineConfig::default(),
            exec_engine: LiveExecEngineConfig::default(),
            data_clients: HashMap::new(),
            exec_clients: HashMap::new(),
        }
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

    fn timeout_connection(&self) -> u32 {
        self.timeout_connection
    }

    fn timeout_reconciliation(&self) -> u32 {
        self.timeout_reconciliation
    }

    fn timeout_portfolio(&self) -> u32 {
        self.timeout_portfolio
    }

    fn timeout_disconnection(&self) -> u32 {
        self.timeout_disconnection
    }

    fn timeout_post_stop(&self) -> u32 {
        self.timeout_post_stop
    }

    fn timeout_shutdown(&self) -> u32 {
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
        self.portfolio.clone()
    }

    fn streaming(&self) -> Option<nautilus_persistence::config::StreamingConfig> {
        self.streaming.clone()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
        assert_eq!(config.reconciliation_lookback_mins, None);
        assert!(!config.filter_unclaimed_external_orders);
        assert!(!config.filter_position_reports);
        assert!(config.generate_missing_orders);
        assert_eq!(config.inflight_check_interval_ms, 2_000);
        assert_eq!(config.inflight_check_threshold_ms, 5_000);
        assert_eq!(config.inflight_check_retries, 5);
        assert!(config.open_check_open_only);
        assert_eq!(config.qsize, 100_000);
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
