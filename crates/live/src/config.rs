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

use std::{collections::HashMap, str::FromStr, time::Duration};

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig, throttler::RateLimit,
};
use nautilus_core::UUID4;
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::{
    enums::BarIntervalType,
    identifiers::{ClientId, ClientOrderId, InstrumentId, TraderId},
};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_system::config::{NautilusKernelConfig, StreamingConfig};
use rust_decimal::Decimal;
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
    /// If time bar aggregators will build and emit bars with no new market updates.
    #[builder(default = true)]
    pub time_bars_build_with_no_updates: bool,
    /// If time bar aggregators will timestamp `ts_event` on bar close.
    #[builder(default = true)]
    pub time_bars_timestamp_on_close: bool,
    /// If time bar aggregators will skip emitting a bar if aggregation starts mid-interval.
    #[builder(default)]
    pub time_bars_skip_first_non_full_bar: bool,
    /// Determines the interval semantics used for time aggregation.
    #[builder(default = BarIntervalType::LeftOpen)]
    pub time_bars_interval_type: BarIntervalType,
    /// The build delay in microseconds before time bars are emitted.
    #[builder(default)]
    pub time_bars_build_delay: u64,
    /// If data timestamp sequencing should be validated and handled.
    #[builder(default)]
    pub validate_data_sequence: bool,
    /// If order book deltas should be buffered until the final delta flag is seen.
    #[builder(default)]
    pub buffer_deltas: bool,
    /// Client IDs declared for external stream processing.
    pub external_clients: Option<Vec<ClientId>>,
    /// If debug mode is active (will provide extra debug logging).
    #[builder(default)]
    pub debug: bool,
    /// Reserved for future queue sizing support.
    ///
    /// Not currently implemented on the current v2 live node path.
    #[builder(default = 100_000)]
    pub qsize: u32,
}

impl Default for LiveDataEngineConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl From<LiveDataEngineConfig> for DataEngineConfig {
    fn from(config: LiveDataEngineConfig) -> Self {
        Self {
            time_bars_build_with_no_updates: config.time_bars_build_with_no_updates,
            time_bars_timestamp_on_close: config.time_bars_timestamp_on_close,
            time_bars_skip_first_non_full_bar: config.time_bars_skip_first_non_full_bar,
            time_bars_interval_type: config.time_bars_interval_type,
            time_bars_build_delay: config.time_bars_build_delay,
            validate_data_sequence: config.validate_data_sequence,
            buffer_deltas: config.buffer_deltas,
            external_clients: config.external_clients,
            debug: config.debug,
            ..Self::default()
        }
    }
}

impl LiveDataEngineConfig {
    fn validate_live_path(&self) -> anyhow::Result<()> {
        let default = Self::default();
        let mut unsupported = Vec::new();

        if self.qsize != default.qsize {
            unsupported.push("qsize");
        }

        if unsupported.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "Unsupported LiveDataEngineConfig field(s) on the current v2 live node path: {}",
                unsupported.join(", ")
            );
        }
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
    /// If all pre-trade risk checks should be bypassed.
    #[builder(default)]
    pub bypass: bool,
    /// The maximum submit order rate as `limit/HH:MM:SS`.
    #[builder(default = "100/00:00:01".to_string())]
    pub max_order_submit_rate: String,
    /// The maximum modify order rate as `limit/HH:MM:SS`.
    #[builder(default = "100/00:00:01".to_string())]
    pub max_order_modify_rate: String,
    /// The maximum notional per order keyed by instrument ID.
    #[builder(default)]
    pub max_notional_per_order: HashMap<String, String>,
    /// If debug mode is active (will provide extra debug logging).
    #[builder(default)]
    pub debug: bool,
    /// Reserved for future queue sizing support.
    ///
    /// Not currently implemented on the current v2 live node path.
    #[builder(default = 100_000)]
    pub qsize: u32,
}

impl Default for LiveRiskEngineConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl From<LiveRiskEngineConfig> for RiskEngineConfig {
    fn from(config: LiveRiskEngineConfig) -> Self {
        let max_notional_per_order = config
            .max_notional_per_order
            .into_iter()
            .map(|(instrument_id, notional)| {
                let instrument_id = instrument_id
                    .parse()
                    .expect("LiveRiskEngineConfig instrument IDs must be validated before use");
                let notional = Decimal::from_str(&notional)
                    .expect("LiveRiskEngineConfig notionals must be validated before use");
                (instrument_id, notional)
            })
            .collect();

        Self {
            bypass: config.bypass,
            max_order_submit: parse_rate_limit(&config.max_order_submit_rate)
                .expect("LiveRiskEngineConfig submit rate must be validated before use"),
            max_order_modify: parse_rate_limit(&config.max_order_modify_rate)
                .expect("LiveRiskEngineConfig modify rate must be validated before use"),
            max_notional_per_order,
            debug: config.debug,
        }
    }
}

impl LiveRiskEngineConfig {
    fn validate_live_path(&self) -> anyhow::Result<()> {
        parse_rate_limit(&self.max_order_submit_rate)
            .map_err(|e| anyhow::anyhow!("invalid `max_order_submit_rate`: {e}"))?;
        parse_rate_limit(&self.max_order_modify_rate)
            .map_err(|e| anyhow::anyhow!("invalid `max_order_modify_rate`: {e}"))?;

        for (instrument_id, notional) in &self.max_notional_per_order {
            instrument_id.parse::<InstrumentId>().map_err(|e| {
                anyhow::anyhow!(
                    "invalid `max_notional_per_order` instrument ID {instrument_id:?}: {e}"
                )
            })?;

            Decimal::from_str(notional).map_err(|e| {
                anyhow::anyhow!("invalid `max_notional_per_order` notional {notional:?}: {e}")
            })?;
        }

        let default = Self::default();
        let mut unsupported = Vec::new();

        if self.qsize != default.qsize {
            unsupported.push("qsize");
        }

        if unsupported.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "Unsupported LiveRiskEngineConfig field(s) on the current v2 live node path: {}",
                unsupported.join(", ")
            );
        }
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
    /// If own order books should be maintained from commands and events.
    #[builder(default)]
    pub manage_own_order_books: bool,
    /// Reserved for future snapshot persistence support.
    ///
    /// Not currently implemented on the current v2 live node path because the live kernel
    /// does not yet wire a cache database adapter.
    #[builder(default)]
    pub snapshot_orders: bool,
    /// Reserved for future snapshot persistence support.
    ///
    /// Not currently implemented on the current v2 live node path because the live kernel
    /// does not yet wire a cache database adapter.
    #[builder(default)]
    pub snapshot_positions: bool,
    /// If order fills exceeding order quantity are allowed.
    #[builder(default)]
    pub allow_overfills: bool,
    /// Client IDs declared for external stream processing.
    pub external_clients: Option<Vec<ClientId>>,
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
    /// Reserved for future purge-to-database support.
    ///
    /// Not currently implemented on the current v2 live node path.
    #[builder(default)]
    pub purge_from_database: bool,
    /// If debug mode is active (will provide extra debug logging).
    #[builder(default)]
    pub debug: bool,
    /// The interval (seconds) between auditing own books against public order books.
    pub own_books_audit_interval_secs: Option<f64>,
    /// Reserved for future graceful shutdown handling.
    ///
    /// Not currently implemented on the current v2 live node path.
    #[builder(default)]
    pub graceful_shutdown_on_error: bool,
    /// Reserved for future queue sizing support.
    ///
    /// Not currently implemented on the current v2 live node path.
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
        let defaults = Self::default();

        Self {
            // These engine knobs are intentionally pinned to defaults until the
            // live kernel wires the remaining persistence/timer behavior.
            load_cache: defaults.load_cache,
            manage_own_order_books: config.manage_own_order_books,
            snapshot_orders: config.snapshot_orders,
            snapshot_positions: config.snapshot_positions,
            snapshot_positions_interval_secs: defaults.snapshot_positions_interval_secs,
            allow_overfills: config.allow_overfills,
            external_clients: config.external_clients,
            purge_closed_orders_interval_mins: config.purge_closed_orders_interval_mins,
            purge_closed_orders_buffer_mins: config.purge_closed_orders_buffer_mins,
            purge_closed_positions_interval_mins: config.purge_closed_positions_interval_mins,
            purge_closed_positions_buffer_mins: config.purge_closed_positions_buffer_mins,
            purge_account_events_interval_mins: config.purge_account_events_interval_mins,
            purge_account_events_lookback_mins: config.purge_account_events_lookback_mins,
            purge_from_database: config.purge_from_database,
            debug: config.debug,
        }
    }
}

impl LiveExecEngineConfig {
    fn validate_live_path(&self) -> anyhow::Result<()> {
        if let Some(instrument_ids) = &self.reconciliation_instrument_ids {
            for instrument_id in instrument_ids {
                instrument_id.parse::<InstrumentId>().map_err(|e| {
                    anyhow::anyhow!(
                        "invalid `reconciliation_instrument_ids` instrument ID {instrument_id:?}: {e}"
                    )
                })?;
            }
        }

        if let Some(client_order_ids) = &self.filtered_client_order_ids {
            for client_order_id in client_order_ids {
                ClientOrderId::new_checked(client_order_id).map_err(|e| {
                    anyhow::anyhow!(
                        "invalid `filtered_client_order_ids` client order ID {client_order_id:?}: {e}"
                    )
                })?;
            }
        }

        let default = Self::default();
        let mut unsupported = Vec::new();

        if self.snapshot_orders != default.snapshot_orders {
            unsupported.push("snapshot_orders");
        }

        if self.snapshot_positions != default.snapshot_positions {
            unsupported.push("snapshot_positions");
        }

        if self.purge_from_database != default.purge_from_database {
            unsupported.push("purge_from_database");
        }

        if self.graceful_shutdown_on_error != default.graceful_shutdown_on_error {
            unsupported.push("graceful_shutdown_on_error");
        }

        if self.qsize != default.qsize {
            unsupported.push("qsize");
        }

        if unsupported.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "Unsupported LiveExecEngineConfig field(s) on the current v2 live node path: {}",
                unsupported.join(", ")
            );
        }
    }
}

fn parse_rate_limit(input: &str) -> anyhow::Result<RateLimit> {
    let (limit, interval) = input
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("invalid rate limit '{input}': missing '/' separator"))?;

    let limit = limit
        .parse::<usize>()
        .map_err(|e| anyhow::anyhow!("invalid rate limit '{input}': {e}"))?;

    let mut parts = interval.split(':');
    let hours = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid rate limit '{input}': missing hours"))?
        .parse::<u64>()
        .map_err(|e| anyhow::anyhow!("invalid rate limit '{input}': {e}"))?;
    let minutes = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid rate limit '{input}': missing minutes"))?
        .parse::<u64>()
        .map_err(|e| anyhow::anyhow!("invalid rate limit '{input}': {e}"))?;
    let seconds = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid rate limit '{input}': missing seconds"))?
        .parse::<u64>()
        .map_err(|e| anyhow::anyhow!("invalid rate limit '{input}': {e}"))?;

    if parts.next().is_some() {
        anyhow::bail!("invalid rate limit '{input}': expected HH:MM:SS interval");
    }

    let interval_ns = hours
        .saturating_mul(3_600)
        .saturating_add(minutes.saturating_mul(60))
        .saturating_add(seconds)
        .saturating_mul(1_000_000_000);

    Ok(RateLimit::new(limit, interval_ns))
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

impl LiveNodeConfig {
    pub(crate) fn validate_live_path(&self) -> anyhow::Result<()> {
        self.data_engine.validate_live_path()?;
        self.risk_engine.validate_live_path()?;
        self.exec_engine.validate_live_path()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_live_data_engine_config_defaults() {
        let config = LiveDataEngineConfig::default();

        assert!(config.time_bars_build_with_no_updates);
        assert!(config.time_bars_timestamp_on_close);
        assert!(!config.time_bars_skip_first_non_full_bar);
        assert_eq!(config.time_bars_interval_type, BarIntervalType::LeftOpen);
        assert_eq!(config.time_bars_build_delay, 0);
        assert!(!config.validate_data_sequence);
        assert!(!config.buffer_deltas);
        assert_eq!(config.external_clients, None);
        assert!(!config.debug);
        assert_eq!(config.qsize, 100_000);
    }

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
    fn test_live_data_engine_config_converts_to_data_engine_config() {
        let config = LiveDataEngineConfig {
            time_bars_build_with_no_updates: false,
            time_bars_timestamp_on_close: false,
            time_bars_skip_first_non_full_bar: true,
            time_bars_interval_type: BarIntervalType::RightOpen,
            time_bars_build_delay: 1_500,
            validate_data_sequence: true,
            buffer_deltas: true,
            external_clients: Some(vec![ClientId::from("EXTERNAL")]),
            debug: true,
            qsize: 7,
        };

        let converted: DataEngineConfig = config.into();

        assert!(!converted.time_bars_build_with_no_updates);
        assert!(!converted.time_bars_timestamp_on_close);
        assert!(converted.time_bars_skip_first_non_full_bar);
        assert_eq!(
            converted.time_bars_interval_type,
            BarIntervalType::RightOpen
        );
        assert_eq!(converted.time_bars_build_delay, 1_500);
        assert!(converted.validate_data_sequence);
        assert!(converted.buffer_deltas);
        assert_eq!(
            converted.external_clients,
            Some(vec![ClientId::from("EXTERNAL")])
        );
        assert!(converted.debug);
    }

    #[rstest]
    fn test_live_risk_engine_config_converts_to_risk_engine_config() {
        let config = LiveRiskEngineConfig {
            bypass: true,
            max_order_submit_rate: "12/00:00:03".to_string(),
            max_order_modify_rate: "7/00:00:05".to_string(),
            max_notional_per_order: HashMap::from([(
                "ETHUSDT.BINANCE".to_string(),
                "1000.5".to_string(),
            )]),
            debug: true,
            qsize: 99,
        };

        let converted: RiskEngineConfig = config.into();

        assert!(converted.bypass);
        assert_eq!(
            converted.max_order_submit,
            RateLimit::new(12, 3_000_000_000)
        );
        assert_eq!(converted.max_order_modify, RateLimit::new(7, 5_000_000_000));
        assert_eq!(
            converted.max_notional_per_order[&"ETHUSDT.BINANCE".parse().unwrap()],
            Decimal::from_str("1000.5").unwrap(),
        );
        assert!(converted.debug);
    }

    #[rstest]
    fn test_live_exec_engine_config_defaults() {
        let config = LiveExecEngineConfig::default();

        assert!(!config.manage_own_order_books);
        assert!(!config.snapshot_orders);
        assert!(!config.snapshot_positions);
        assert_eq!(config.external_clients, None);
        assert!(!config.allow_overfills);
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
        assert!(!config.debug);
        assert!(!config.graceful_shutdown_on_error);
        assert_eq!(config.qsize, 100_000);
        assert_eq!(config.reconciliation_startup_delay_secs, 10.0);
    }

    #[rstest]
    fn test_live_exec_engine_config_converts_to_execution_engine_config() {
        let config = LiveExecEngineConfig {
            manage_own_order_books: true,
            snapshot_orders: true,
            snapshot_positions: true,
            allow_overfills: true,
            external_clients: Some(vec![ClientId::from("EXT-EXEC")]),
            reconciliation: false,
            reconciliation_startup_delay_secs: 5.0,
            reconciliation_lookback_mins: Some(30),
            reconciliation_instrument_ids: None,
            filter_unclaimed_external_orders: true,
            filter_position_reports: true,
            filtered_client_order_ids: Some(vec!["O-123".to_string()]),
            generate_missing_orders: false,
            inflight_check_interval_ms: 2_500,
            inflight_check_threshold_ms: 7_500,
            inflight_check_retries: 6,
            open_check_interval_secs: Some(10.0),
            open_check_lookback_mins: Some(30),
            open_check_threshold_ms: 8_000,
            open_check_missing_retries: 8,
            open_check_open_only: false,
            max_single_order_queries_per_cycle: 9,
            single_order_query_delay_ms: 150,
            position_check_interval_secs: Some(20.0),
            position_check_lookback_mins: 45,
            position_check_threshold_ms: 9_000,
            position_check_retries: 4,
            purge_closed_orders_interval_mins: Some(1),
            purge_closed_orders_buffer_mins: Some(2),
            purge_closed_positions_interval_mins: Some(3),
            purge_closed_positions_buffer_mins: Some(4),
            purge_account_events_interval_mins: Some(5),
            purge_account_events_lookback_mins: Some(6),
            purge_from_database: true,
            debug: true,
            own_books_audit_interval_secs: Some(30.0),
            graceful_shutdown_on_error: true,
            qsize: 11,
        };

        let converted: ExecutionEngineConfig = config.into();

        assert!(converted.manage_own_order_books);
        assert!(converted.snapshot_orders);
        assert!(converted.snapshot_positions);
        assert!(converted.allow_overfills);
        assert_eq!(
            converted.external_clients,
            Some(vec![ClientId::from("EXT-EXEC")]),
        );
        assert_eq!(converted.purge_closed_orders_interval_mins, Some(1));
        assert_eq!(converted.purge_closed_orders_buffer_mins, Some(2));
        assert_eq!(converted.purge_closed_positions_interval_mins, Some(3));
        assert_eq!(converted.purge_closed_positions_buffer_mins, Some(4));
        assert_eq!(converted.purge_account_events_interval_mins, Some(5));
        assert_eq!(converted.purge_account_events_lookback_mins, Some(6));
        assert!(converted.purge_from_database);
        assert!(converted.debug);
    }

    #[rstest]
    fn test_live_data_engine_config_rejects_unsupported_live_path_fields() {
        let config = LiveDataEngineConfig {
            qsize: 1,
            ..Default::default()
        };

        let err = config.validate_live_path().unwrap_err().to_string();
        assert!(err.contains("qsize"));
    }

    #[rstest]
    fn test_live_risk_engine_config_rejects_unsupported_live_path_fields() {
        let config = LiveRiskEngineConfig {
            qsize: 1,
            ..Default::default()
        };

        let err = config.validate_live_path().unwrap_err().to_string();
        assert!(err.contains("qsize"));
    }

    #[rstest]
    fn test_live_risk_engine_config_rejects_invalid_supported_live_path_fields() {
        let config = LiveRiskEngineConfig {
            max_order_submit_rate: "bad-rate".to_string(),
            ..Default::default()
        };

        let err = config.validate_live_path().unwrap_err().to_string();
        assert!(err.contains("max_order_submit_rate"));
    }

    #[rstest]
    fn test_live_exec_engine_config_rejects_unsupported_live_path_fields() {
        let config = LiveExecEngineConfig {
            snapshot_orders: true,
            snapshot_positions: true,
            purge_from_database: true,
            graceful_shutdown_on_error: true,
            qsize: 1,
            ..Default::default()
        };

        let err = config.validate_live_path().unwrap_err().to_string();
        assert!(err.contains("snapshot_orders"));
        assert!(err.contains("snapshot_positions"));
        assert!(err.contains("purge_from_database"));
        assert!(err.contains("graceful_shutdown_on_error"));
        assert!(err.contains("qsize"));
    }

    #[rstest]
    fn test_live_exec_engine_config_rejects_invalid_supported_live_path_fields() {
        let config = LiveExecEngineConfig {
            reconciliation_instrument_ids: Some(vec!["INVALID".to_string()]),
            ..Default::default()
        };

        let err = config.validate_live_path().unwrap_err().to_string();
        assert!(err.contains("reconciliation_instrument_ids"));
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
