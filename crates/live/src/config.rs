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

use ahash::AHashMap;
use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig, throttler::RateLimit,
};
use nautilus_core::{UUID4, datetime::NANOSECONDS_IN_SECOND};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::{
    engine::config::ExecutionEngineConfig, order_emulator::config::OrderEmulatorConfig,
};
use nautilus_model::{
    enums::{BarAggregation, BarIntervalType},
    identifiers::{ClientId, ClientOrderId, InstrumentId, TraderId},
};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_system::config::{NautilusKernelConfig, StreamingConfig};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// The default rate limit string used for order submission and modification.
const DEFAULT_ORDER_RATE_LIMIT: &str = "100/00:00:01";

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
#[serde(deny_unknown_fields)]
pub struct LiveDataEngineConfig {
    /// If time bar aggregators will build and emit bars with no new market updates.
    #[builder(default = true)]
    pub time_bars_build_with_no_updates: bool,
    /// If time bar aggregators will timestamp `ts_event` on bar close.
    /// If false, the aggregator will timestamp on bar open.
    #[builder(default = true)]
    pub time_bars_timestamp_on_close: bool,
    /// If time bar aggregators will skip emitting a bar when aggregation starts mid-interval.
    #[builder(default)]
    pub time_bars_skip_first_non_full_bar: bool,
    /// The interval semantics used for time aggregation.
    #[builder(default = BarIntervalType::LeftOpen)]
    pub time_bars_interval_type: BarIntervalType,
    /// The build delay (microseconds) before a time bar is emitted.
    #[builder(default)]
    pub time_bars_build_delay: u64,
    /// A mapping of time bar aggregation types to their origin time offsets (nanoseconds).
    ///
    /// Keys are `BarAggregation` variant names, values are offset durations in nanoseconds.
    #[builder(default)]
    pub time_bars_origins: HashMap<String, u64>,
    /// If data timestamp sequencing should be validated and handled.
    #[builder(default)]
    pub validate_data_sequence: bool,
    /// If order book deltas should be buffered until the `F_LAST` flag is set for a delta.
    #[builder(default)]
    pub buffer_deltas: bool,
    /// If quotes should be emitted on order book updates.
    #[builder(default)]
    pub emit_quotes_from_book: bool,
    /// If quotes should be emitted on order book depth updates.
    #[builder(default)]
    pub emit_quotes_from_book_depths: bool,
    /// Client IDs declared for external stream processing.
    ///
    /// The data engine will not attempt to send data commands to these client IDs.
    pub external_clients: Option<Vec<ClientId>>,
    /// If debug mode is active (will provide extra debug logging).
    #[builder(default)]
    pub debug: bool,
    /// If the engine should gracefully shut down when queue processing encounters unexpected errors.
    #[builder(default)]
    pub graceful_shutdown_on_error: bool,
    /// The queue size for the engine's internal queue buffers.
    ///
    /// Not implemented on the current live runtime; `validate_runtime_support` rejects
    /// any value other than the default.
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
        let time_bars_origins = config
            .time_bars_origins
            .into_iter()
            .map(|(agg, nanos)| {
                let agg = BarAggregation::from_str(&agg)
                    .expect("validate_runtime_support must run before DataEngineConfig conversion");
                (agg, Duration::from_nanos(nanos))
            })
            .collect();

        Self {
            time_bars_build_with_no_updates: config.time_bars_build_with_no_updates,
            time_bars_timestamp_on_close: config.time_bars_timestamp_on_close,
            time_bars_skip_first_non_full_bar: config.time_bars_skip_first_non_full_bar,
            time_bars_interval_type: config.time_bars_interval_type,
            time_bars_build_delay: config.time_bars_build_delay,
            time_bars_origins,
            validate_data_sequence: config.validate_data_sequence,
            buffer_deltas: config.buffer_deltas,
            emit_quotes_from_book: config.emit_quotes_from_book,
            emit_quotes_from_book_depths: config.emit_quotes_from_book_depths,
            external_clients: config.external_clients,
            debug: config.debug,
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
#[serde(deny_unknown_fields)]
pub struct LiveRiskEngineConfig {
    /// If all pre-trade risk checks should be bypassed.
    #[builder(default)]
    pub bypass: bool,
    /// The maximum submit order rate as `limit/HH:MM:SS`.
    #[builder(default = DEFAULT_ORDER_RATE_LIMIT.to_string())]
    pub max_order_submit_rate: String,
    /// The maximum modify order rate as `limit/HH:MM:SS`.
    #[builder(default = DEFAULT_ORDER_RATE_LIMIT.to_string())]
    pub max_order_modify_rate: String,
    /// The maximum notional per order keyed by instrument ID.
    ///
    /// Entries map instrument ID strings to decimal notional strings.
    #[builder(default)]
    pub max_notional_per_order: HashMap<String, String>,
    /// If debug mode is active (will provide extra debug logging).
    #[builder(default)]
    pub debug: bool,
    /// If the engine should gracefully shut down when queue processing encounters unexpected errors.
    #[builder(default)]
    pub graceful_shutdown_on_error: bool,
    /// The queue size for the engine's internal queue buffers.
    ///
    /// Not implemented on the current live runtime; `validate_runtime_support` rejects
    /// any value other than the default.
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
                let instrument_id = InstrumentId::from_str(&instrument_id)
                    .expect("validate_runtime_support must run before RiskEngineConfig conversion");
                let notional = Decimal::from_str(&notional)
                    .expect("validate_runtime_support must run before RiskEngineConfig conversion");
                (instrument_id, notional)
            })
            .collect::<AHashMap<_, _>>();

        Self {
            bypass: config.bypass,
            max_order_submit: parse_rate_limit(&config.max_order_submit_rate)
                .expect("validate_runtime_support must run before RiskEngineConfig conversion"),
            max_order_modify: parse_rate_limit(&config.max_order_modify_rate)
                .expect("validate_runtime_support must run before RiskEngineConfig conversion"),
            max_notional_per_order,
            debug: config.debug,
        }
    }
}

fn parse_rate_limit(input: &str) -> anyhow::Result<RateLimit> {
    let (limit, interval) = input.split_once('/').ok_or_else(|| {
        anyhow::anyhow!("invalid rate limit '{input}': expected 'limit/HH:MM:SS'")
    })?;

    let limit = limit
        .parse::<usize>()
        .map_err(|e| anyhow::anyhow!("invalid rate limit '{input}': {e}"))?;

    if limit == 0 {
        anyhow::bail!("invalid rate limit '{input}': limit must be greater than zero");
    }

    let mut parts = interval.split(':');
    let mut next = |label: &str| -> anyhow::Result<u64> {
        parts
            .next()
            .ok_or_else(|| {
                anyhow::anyhow!("invalid rate limit '{input}': missing {label} component")
            })?
            .parse::<u64>()
            .map_err(|e| anyhow::anyhow!("invalid rate limit '{input}': {label}: {e}"))
    };

    let hours = next("hours")?;
    let minutes = next("minutes")?;
    let seconds = next("seconds")?;

    if parts.next().is_some() {
        anyhow::bail!("invalid rate limit '{input}': expected 'limit/HH:MM:SS'");
    }

    let interval_ns = hours
        .saturating_mul(3_600)
        .saturating_add(minutes.saturating_mul(60))
        .saturating_add(seconds)
        .saturating_mul(NANOSECONDS_IN_SECOND);

    if interval_ns == 0 {
        anyhow::bail!("invalid rate limit '{input}': interval must be greater than zero");
    }

    Ok(RateLimit::new(limit, interval_ns))
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
#[serde(deny_unknown_fields)]
pub struct LiveExecEngineConfig {
    /// If the cache should be loaded on initialization.
    #[builder(default = true)]
    pub load_cache: bool,
    /// If order state snapshot lists should be persisted to a backing database.
    ///
    /// Not implemented on the current live runtime; `validate_runtime_support` rejects
    /// any value other than the default because the live kernel does not yet wire a
    /// cache database adapter.
    #[builder(default)]
    pub snapshot_orders: bool,
    /// If position state snapshot lists should be persisted to a backing database.
    ///
    /// Not implemented on the current live runtime; `validate_runtime_support` rejects
    /// any value other than the default because the live kernel does not yet wire a
    /// cache database adapter.
    #[builder(default)]
    pub snapshot_positions: bool,
    /// The interval (seconds) at which additional position state snapshots are persisted.
    /// If `None` then no additional snapshots will be taken.
    pub snapshot_positions_interval_secs: Option<f64>,
    /// Client IDs declared for external stream processing.
    ///
    /// The execution engine will not attempt to send trading commands to these client
    /// IDs, assuming an external process consumes them from the bus.
    pub external_clients: Option<Vec<ClientId>>,
    /// If debug mode is active (will provide extra debug logging).
    #[builder(default)]
    pub debug: bool,
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
    #[builder(default = 10)]
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
    #[builder(default = 5_000)]
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
    /// If order fills exceeding order quantity are allowed (logs warning instead of raising).
    /// Useful when position reconciliation races with exchange fill events.
    #[builder(default)]
    pub allow_overfills: bool,
    /// If the execution engine should maintain own/user order books based on commands and events.
    #[builder(default)]
    pub manage_own_order_books: bool,
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
            load_cache: config.load_cache,
            manage_own_order_books: config.manage_own_order_books,
            snapshot_orders: config.snapshot_orders,
            snapshot_positions: config.snapshot_positions,
            snapshot_positions_interval_secs: config.snapshot_positions_interval_secs,
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
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
pub struct InstrumentProviderConfig {
    /// Whether to load all instruments on startup.
    #[builder(default)]
    pub load_all: bool,
    /// Specific instrument IDs to load on startup (if `load_all` is false).
    pub load_ids: Option<Vec<String>>,
    /// Venue-specific instrument loading filters.
    #[builder(default)]
    pub filters: HashMap<String, serde_json::Value>,
    /// A fully qualified path to a callable for custom instrument filtering.
    pub filter_callable: Option<String>,
    /// If parser warnings should be logged.
    #[builder(default = true)]
    pub log_warnings: bool,
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
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
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
    #[builder(default = Duration::from_secs(120))]
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
    /// The order emulator configuration.
    pub emulator: Option<OrderEmulatorConfig>,
    /// The configuration for streaming to feather files.
    pub streaming: Option<StreamingConfig>,
    /// If the asyncio event loop should run in debug mode.
    #[builder(default)]
    pub loop_debug: bool,
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

impl LiveNodeConfig {
    /// Validates config fields that the Rust live runtime does not support yet, and checks
    /// that supported fields hold values the downstream engine conversions can parse.
    ///
    /// # Errors
    ///
    /// Returns an error when a config field would otherwise be ignored at runtime, or when a
    /// supported field holds a value that cannot be converted to its engine-side representation.
    pub(crate) fn validate_runtime_support(&self) -> anyhow::Result<()> {
        if self.msgbus.is_some() {
            anyhow::bail!("LiveNodeConfig.msgbus is not supported by the Rust live runtime yet");
        }

        if self.streaming.is_some() {
            anyhow::bail!("LiveNodeConfig.streaming is not supported by the Rust live runtime yet");
        }

        if self.emulator.is_some() {
            anyhow::bail!("LiveNodeConfig.emulator is not supported by the Rust live runtime yet");
        }

        if self.loop_debug {
            anyhow::bail!(
                "LiveNodeConfig.loop_debug is not supported by the Rust live runtime yet"
            );
        }

        if self.logging.file_config.is_some() {
            anyhow::bail!(
                "LoggerConfig.file_config is not supported by the Rust live runtime yet (use py_init_logging)"
            );
        }

        if self.logging.clear_log_file {
            anyhow::bail!(
                "LoggerConfig.clear_log_file is not supported by the Rust live runtime yet"
            );
        }

        self.data_engine.validate_runtime_support()?;
        self.risk_engine.validate_runtime_support()?;
        self.exec_engine.validate_runtime_support()?;

        Ok(())
    }
}

impl LiveDataEngineConfig {
    fn validate_runtime_support(&self) -> anyhow::Result<()> {
        for agg_str in self.time_bars_origins.keys() {
            BarAggregation::from_str(agg_str).map_err(|e| {
                anyhow::anyhow!(
                    "invalid LiveDataEngineConfig.time_bars_origins key {agg_str:?}: {e}"
                )
            })?;
        }

        let default = Self::default();

        if self.graceful_shutdown_on_error != default.graceful_shutdown_on_error {
            anyhow::bail!(
                "LiveDataEngineConfig.graceful_shutdown_on_error is not supported by the Rust live runtime yet"
            );
        }

        if self.qsize != default.qsize {
            anyhow::bail!(
                "LiveDataEngineConfig.qsize is not supported by the Rust live runtime yet"
            );
        }

        Ok(())
    }
}

impl LiveRiskEngineConfig {
    fn validate_runtime_support(&self) -> anyhow::Result<()> {
        parse_rate_limit(&self.max_order_submit_rate).map_err(|e| {
            anyhow::anyhow!("invalid LiveRiskEngineConfig.max_order_submit_rate: {e}")
        })?;
        parse_rate_limit(&self.max_order_modify_rate).map_err(|e| {
            anyhow::anyhow!("invalid LiveRiskEngineConfig.max_order_modify_rate: {e}")
        })?;

        for (instrument_id, notional) in &self.max_notional_per_order {
            InstrumentId::from_str(instrument_id).map_err(|e| {
                anyhow::anyhow!(
                    "invalid LiveRiskEngineConfig.max_notional_per_order instrument ID {instrument_id:?}: {e}"
                )
            })?;
            Decimal::from_str(notional).map_err(|e| {
                anyhow::anyhow!(
                    "invalid LiveRiskEngineConfig.max_notional_per_order notional {notional:?}: {e}"
                )
            })?;
        }

        let default = Self::default();

        if self.graceful_shutdown_on_error != default.graceful_shutdown_on_error {
            anyhow::bail!(
                "LiveRiskEngineConfig.graceful_shutdown_on_error is not supported by the Rust live runtime yet"
            );
        }

        if self.qsize != default.qsize {
            anyhow::bail!(
                "LiveRiskEngineConfig.qsize is not supported by the Rust live runtime yet"
            );
        }

        Ok(())
    }
}

impl LiveExecEngineConfig {
    fn validate_runtime_support(&self) -> anyhow::Result<()> {
        // `Duration::from_secs_f64` panics on negative, NaN, or infinite input, and the
        // `run()` path feeds this value straight in when reconciliation is enabled. Match
        // the legacy Python `PositiveFloat` semantics and reject hostile values at build.
        if !self.reconciliation_startup_delay_secs.is_finite()
            || self.reconciliation_startup_delay_secs < 0.0
        {
            anyhow::bail!(
                "invalid LiveExecEngineConfig.reconciliation_startup_delay_secs: {} (must be a non-negative finite number)",
                self.reconciliation_startup_delay_secs
            );
        }

        if let Some(instrument_ids) = &self.reconciliation_instrument_ids {
            for instrument_id in instrument_ids {
                InstrumentId::from_str(instrument_id).map_err(|e| {
                    anyhow::anyhow!(
                        "invalid LiveExecEngineConfig.reconciliation_instrument_ids entry {instrument_id:?}: {e}"
                    )
                })?;
            }
        }

        if let Some(client_order_ids) = &self.filtered_client_order_ids {
            for client_order_id in client_order_ids {
                ClientOrderId::new_checked(client_order_id).map_err(|e| {
                    anyhow::anyhow!(
                        "invalid LiveExecEngineConfig.filtered_client_order_ids entry {client_order_id:?}: {e}"
                    )
                })?;
            }
        }

        let default = Self::default();

        if self.snapshot_orders != default.snapshot_orders {
            anyhow::bail!(
                "LiveExecEngineConfig.snapshot_orders is not supported by the Rust live runtime yet"
            );
        }

        if self.snapshot_positions != default.snapshot_positions {
            anyhow::bail!(
                "LiveExecEngineConfig.snapshot_positions is not supported by the Rust live runtime yet"
            );
        }

        if self.purge_from_database != default.purge_from_database {
            anyhow::bail!(
                "LiveExecEngineConfig.purge_from_database is not supported by the Rust live runtime yet"
            );
        }

        if self.graceful_shutdown_on_error != default.graceful_shutdown_on_error {
            anyhow::bail!(
                "LiveExecEngineConfig.graceful_shutdown_on_error is not supported by the Rust live runtime yet"
            );
        }

        if self.qsize != default.qsize {
            anyhow::bail!(
                "LiveExecEngineConfig.qsize is not supported by the Rust live runtime yet"
            );
        }

        Ok(())
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
    use nautilus_system::config::RotationConfig;
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
    fn test_validate_runtime_support_with_defaults() {
        let config = LiveNodeConfig::default();

        assert!(config.validate_runtime_support().is_ok());
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_msgbus_config() {
        let config = LiveNodeConfig {
            msgbus: Some(MessageBusConfig::default()),
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err();
        assert_eq!(
            error.to_string(),
            "LiveNodeConfig.msgbus is not supported by the Rust live runtime yet"
        );
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_streaming_config() {
        let config = LiveNodeConfig {
            streaming: Some(StreamingConfig::new(
                "catalog".to_string(),
                "file".to_string(),
                1_000,
                false,
                RotationConfig::NoRotation,
            )),
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err();
        assert_eq!(
            error.to_string(),
            "LiveNodeConfig.streaming is not supported by the Rust live runtime yet"
        );
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_data_engine_qsize() {
        let config = LiveNodeConfig {
            data_engine: LiveDataEngineConfig {
                qsize: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err();
        assert_eq!(
            error.to_string(),
            "LiveDataEngineConfig.qsize is not supported by the Rust live runtime yet"
        );
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_risk_engine_qsize() {
        let config = LiveNodeConfig {
            risk_engine: LiveRiskEngineConfig {
                qsize: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err();
        assert_eq!(
            error.to_string(),
            "LiveRiskEngineConfig.qsize is not supported by the Rust live runtime yet"
        );
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
            ..Default::default()
        };

        let converted: DataEngineConfig = config.into();

        assert!(!converted.time_bars_build_with_no_updates);
        assert!(!converted.time_bars_timestamp_on_close);
        assert!(converted.time_bars_skip_first_non_full_bar);
        assert_eq!(
            converted.time_bars_interval_type,
            BarIntervalType::RightOpen,
        );
        assert_eq!(converted.time_bars_build_delay, 1_500);
        assert!(converted.time_bars_origins.is_empty());
        assert!(converted.validate_data_sequence);
        assert!(converted.buffer_deltas);
        assert!(!converted.emit_quotes_from_book);
        assert!(!converted.emit_quotes_from_book_depths);
        assert_eq!(
            converted.external_clients,
            Some(vec![ClientId::from("EXTERNAL")]),
        );
        assert!(converted.debug);
    }

    #[rstest]
    fn test_live_data_engine_config_converts_time_bars_origins() {
        let config = LiveDataEngineConfig {
            time_bars_origins: HashMap::from([("Minute".to_string(), 5_000_000_000)]),
            emit_quotes_from_book: true,
            emit_quotes_from_book_depths: true,
            ..Default::default()
        };

        let converted: DataEngineConfig = config.into();

        assert_eq!(converted.time_bars_origins.len(), 1);
        assert_eq!(
            converted.time_bars_origins[&BarAggregation::Minute],
            Duration::from_nanos(5_000_000_000),
        );
        assert!(converted.emit_quotes_from_book);
        assert!(converted.emit_quotes_from_book_depths);
    }

    #[rstest]
    fn test_live_exec_engine_config_converts_to_exec_engine_config() {
        let config = LiveExecEngineConfig {
            load_cache: false,
            snapshot_positions_interval_secs: Some(30.0),
            ..Default::default()
        };

        let converted: ExecutionEngineConfig = config.into();

        assert!(!converted.load_cache);
        assert_eq!(converted.snapshot_positions_interval_secs, Some(30.0));
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
            ..Default::default()
        };

        let converted: RiskEngineConfig = config.into();

        assert!(converted.bypass);
        assert_eq!(
            converted.max_order_submit,
            RateLimit::new(12, 3_000_000_000)
        );
        assert_eq!(converted.max_order_modify, RateLimit::new(7, 5_000_000_000));
        assert_eq!(
            converted.max_notional_per_order[&"ETHUSDT.BINANCE".parse::<InstrumentId>().unwrap()],
            Decimal::from_str("1000.5").unwrap(),
        );
        assert!(converted.debug);
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_exec_engine_snapshot_orders() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecEngineConfig {
                snapshot_orders: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err();
        assert_eq!(
            error.to_string(),
            "LiveExecEngineConfig.snapshot_orders is not supported by the Rust live runtime yet"
        );
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_invalid_rate_limit() {
        let config = LiveNodeConfig {
            risk_engine: LiveRiskEngineConfig {
                max_order_submit_rate: "bad-rate".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("LiveRiskEngineConfig.max_order_submit_rate"));
    }

    #[rstest]
    #[case(-1.0)]
    #[case(f64::NAN)]
    #[case(f64::INFINITY)]
    #[case(f64::NEG_INFINITY)]
    fn test_validate_runtime_support_rejects_hostile_startup_delay(#[case] value: f64) {
        let config = LiveNodeConfig {
            exec_engine: LiveExecEngineConfig {
                reconciliation_startup_delay_secs: value,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("reconciliation_startup_delay_secs"));
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_invalid_reconciliation_instrument_id() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecEngineConfig {
                reconciliation_instrument_ids: Some(vec!["INVALID".to_string()]),
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("reconciliation_instrument_ids"));
    }

    #[rstest]
    fn test_parse_rate_limit_happy_path() {
        let limit = parse_rate_limit("150/00:00:02").unwrap();
        assert_eq!(limit, RateLimit::new(150, 2_000_000_000));
    }

    #[rstest]
    fn test_parse_rate_limit_rejects_trailing_component() {
        let err = parse_rate_limit("10/00:00:01:99").unwrap_err().to_string();
        assert!(err.contains("expected 'limit/HH:MM:SS'"));
    }

    #[rstest]
    fn test_parse_rate_limit_rejects_zero_limit() {
        let err = parse_rate_limit("0/00:00:01").unwrap_err().to_string();
        assert!(err.contains("limit must be greater than zero"));
    }

    #[rstest]
    fn test_parse_rate_limit_rejects_zero_interval() {
        let err = parse_rate_limit("100/00:00:00").unwrap_err().to_string();
        assert!(err.contains("interval must be greater than zero"));
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_exec_engine_qsize() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecEngineConfig {
                qsize: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err();
        assert_eq!(
            error.to_string(),
            "LiveExecEngineConfig.qsize is not supported by the Rust live runtime yet"
        );
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_data_engine_graceful_shutdown() {
        let config = LiveNodeConfig {
            data_engine: LiveDataEngineConfig {
                graceful_shutdown_on_error: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("graceful_shutdown_on_error"));
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_risk_engine_graceful_shutdown() {
        let config = LiveNodeConfig {
            risk_engine: LiveRiskEngineConfig {
                graceful_shutdown_on_error: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("graceful_shutdown_on_error"));
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_emulator() {
        let config = LiveNodeConfig {
            emulator: Some(OrderEmulatorConfig::default()),
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("emulator"));
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_loop_debug() {
        let config = LiveNodeConfig {
            loop_debug: true,
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("loop_debug"));
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_file_config() {
        use nautilus_common::logging::writer::FileWriterConfig;

        let config = LiveNodeConfig {
            logging: LoggerConfig {
                file_config: Some(FileWriterConfig::default()),
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("file_config"));
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_clear_log_file() {
        let config = LiveNodeConfig {
            logging: LoggerConfig {
                clear_log_file: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("clear_log_file"));
    }

    #[rstest]
    fn test_validate_runtime_support_rejects_invalid_time_bars_origins_key() {
        let config = LiveNodeConfig {
            data_engine: LiveDataEngineConfig {
                time_bars_origins: HashMap::from([("INVALID".to_string(), 1_000)]),
                ..Default::default()
            },
            ..Default::default()
        };

        let error = config.validate_runtime_support().unwrap_err().to_string();
        assert!(error.contains("time_bars_origins"));
    }

    #[rstest]
    fn test_live_exec_engine_config_defaults() {
        let config = LiveExecEngineConfig::default();

        assert!(config.load_cache);
        assert!(!config.snapshot_orders);
        assert!(!config.snapshot_positions);
        assert_eq!(config.snapshot_positions_interval_secs, None);
        assert_eq!(config.external_clients, None);
        assert!(!config.debug);
        assert!(!config.manage_own_order_books);
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
        assert_eq!(config.max_single_order_queries_per_cycle, 10);
        assert_eq!(config.position_check_threshold_ms, 5_000);
        assert_eq!(config.position_check_retries, 3);
        assert!(!config.purge_from_database);
        assert!(!config.graceful_shutdown_on_error);
        assert_eq!(config.qsize, 100_000);
    }

    #[rstest]
    fn test_live_data_engine_config_defaults() {
        let config = LiveDataEngineConfig::default();

        assert!(config.time_bars_build_with_no_updates);
        assert!(config.time_bars_timestamp_on_close);
        assert!(!config.time_bars_skip_first_non_full_bar);
        assert_eq!(config.time_bars_interval_type, BarIntervalType::LeftOpen);
        assert_eq!(config.time_bars_build_delay, 0);
        assert!(config.time_bars_origins.is_empty());
        assert!(!config.validate_data_sequence);
        assert!(!config.buffer_deltas);
        assert!(!config.emit_quotes_from_book);
        assert!(!config.emit_quotes_from_book_depths);
        assert_eq!(config.external_clients, None);
        assert!(!config.debug);
        assert!(!config.graceful_shutdown_on_error);
        assert_eq!(config.qsize, 100_000);
    }

    #[rstest]
    fn test_live_risk_engine_config_defaults() {
        let config = LiveRiskEngineConfig::default();

        assert!(!config.bypass);
        assert_eq!(config.max_order_submit_rate, DEFAULT_ORDER_RATE_LIMIT);
        assert_eq!(config.max_order_modify_rate, DEFAULT_ORDER_RATE_LIMIT);
        assert!(config.max_notional_per_order.is_empty());
        assert!(!config.debug);
        assert!(!config.graceful_shutdown_on_error);
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
        assert!(config.instrument_provider.load_ids.is_none());
        assert!(config.instrument_provider.filters.is_empty());
        assert!(config.instrument_provider.filter_callable.is_none());
        assert!(config.instrument_provider.log_warnings);
        assert!(!config.routing.default);
    }

    #[rstest]
    fn test_live_data_client_config_rejects_unknown_field() {
        let error = serde_json::from_str::<LiveDataClientConfig>(
            r#"{"handle_revised_bars":true,"unexpected":true}"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[rstest]
    fn test_live_data_client_config_rejects_unknown_nested_field() {
        let error = serde_json::from_str::<LiveDataClientConfig>(
            r#"{"instrument_provider":{"load_all":true,"instrument_provider":{"load_all":false}}}"#,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("unknown field `instrument_provider`")
        );
    }
}
