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

//! Configuration types for the backtest engine, venues, data, and run parameters.

use std::{fmt::Display, str::FromStr, time::Duration};

use ahash::AHashMap;
use nautilus_common::{
    cache::CacheConfig,
    config::{ConfigError, ConfigErrorCollector, ConfigResult},
    enums::Environment,
    logging::logger::LoggerConfig,
    msgbus::backing::MessageBusConfig,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::{
    engine::config::ExecutionEngineConfig,
    models::{
        fee::FeeModelAny,
        fill::FillModelAny,
        latency::{LatencyModel, LatencyModelAny},
    },
};
use nautilus_model::{
    accounts::margin_model::MarginModelAny,
    data::{BarSpecification, BarType},
    enums::{AccountType, BookType, OmsType, OtoTriggerMode},
    identifiers::{ClientId, InstrumentId, TraderId, Venue},
    types::{Currency, Money, Price},
};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_system::config::{NautilusKernelConfig, StreamingConfig};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::modules::{SimulationModule, SimulationModuleAny};

/// Represents a type of market data for catalog queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NautilusDataType {
    QuoteTick,
    TradeTick,
    Bar,
    OrderBookDelta,
    OrderBookDepth10,
    MarkPriceUpdate,
    IndexPriceUpdate,
    FundingRateUpdate,
    InstrumentStatus,
    OptionGreeks,
    InstrumentClose,
}

impl Display for NautilusDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl FromStr for NautilusDataType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            stringify!(QuoteTick) => Ok(Self::QuoteTick),
            stringify!(TradeTick) => Ok(Self::TradeTick),
            stringify!(Bar) => Ok(Self::Bar),
            stringify!(OrderBookDelta) => Ok(Self::OrderBookDelta),
            stringify!(OrderBookDepth10) => Ok(Self::OrderBookDepth10),
            stringify!(MarkPriceUpdate) => Ok(Self::MarkPriceUpdate),
            stringify!(IndexPriceUpdate) => Ok(Self::IndexPriceUpdate),
            stringify!(FundingRateUpdate) => Ok(Self::FundingRateUpdate),
            stringify!(InstrumentStatus) => Ok(Self::InstrumentStatus),
            stringify!(OptionGreeks) => Ok(Self::OptionGreeks),
            stringify!(InstrumentClose) => Ok(Self::InstrumentClose),
            _ => anyhow::bail!("Invalid `NautilusDataType`: '{s}'"),
        }
    }
}

/// Configuration for ``BacktestEngine`` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.backtest",
        from_py_object,
        unsendable
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "config fields mirror the existing Rust and Python backtest engine surfaces"
)]
#[derive(Debug, Clone, bon::Builder)]
pub struct BacktestEngineConfig {
    /// The kernel environment context.
    #[builder(default = Environment::Backtest)]
    pub environment: Environment,
    /// The trader ID for the node.
    #[builder(default)]
    pub trader_id: TraderId,
    /// If trading strategy state should be loaded from the database on start.
    #[builder(default)]
    pub load_state: bool,
    /// If trading strategy state should be saved to the database on stop.
    #[builder(default)]
    pub save_state: bool,
    /// If the system should request shutdown when an error log is emitted.
    ///
    /// Filtered or bypassed error logs still request shutdown.
    #[builder(default)]
    pub shutdown_on_error: bool,
    /// The logging configuration for the kernel.
    #[builder(default)]
    pub logging: LoggerConfig,
    /// The unique instance identifier for the kernel.
    pub instance_id: Option<UUID4>,
    /// The timeout for all clients to connect and initialize.
    #[builder(default = Duration::from_mins(1))]
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
    ///
    /// [`crate::engine::BacktestEngine`] always overrides
    /// `drop_instruments_on_reset` to `false` on this config so that
    /// successive runs can reuse the same dataset.
    pub cache: Option<CacheConfig>,
    /// The message bus configuration.
    pub msgbus: Option<MessageBusConfig>,
    /// The data engine configuration.
    pub data_engine: Option<DataEngineConfig>,
    /// The risk engine configuration.
    pub risk_engine: Option<RiskEngineConfig>,
    /// The execution engine configuration.
    pub exec_engine: Option<ExecutionEngineConfig>,
    /// The portfolio configuration.
    pub portfolio: Option<PortfolioConfig>,
    /// The configuration for streaming to feather files.
    pub streaming: Option<StreamingConfig>,
    /// If logging should be bypassed.
    #[builder(default)]
    pub bypass_logging: bool,
    /// If post backtest performance analysis should be run.
    #[builder(default = true)]
    pub run_analysis: bool,
}

impl NautilusKernelConfig for BacktestEngineConfig {
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

    fn shutdown_on_error(&self) -> bool {
        self.shutdown_on_error
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
        self.data_engine.clone()
    }

    fn risk_engine(&self) -> Option<RiskEngineConfig> {
        self.risk_engine.clone()
    }

    fn exec_engine(&self) -> Option<ExecutionEngineConfig> {
        self.exec_engine.clone()
    }

    fn portfolio(&self) -> Option<PortfolioConfig> {
        self.portfolio
    }

    fn streaming(&self) -> Option<StreamingConfig> {
        self.streaming.clone()
    }
}

impl Default for BacktestEngineConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Imperative-API configuration for registering a simulated venue on
/// [`crate::engine::BacktestEngine`].
///
/// Constructed via [`bon::Builder`] so callers only specify what differs from
/// the documented defaults. Field types mirror the internal
/// `SimulatedExchange` shapes (trait objects for modules/latency, typed
/// `Money` balances), which is why this is distinct from the YAML-friendly
/// [`BacktestVenueConfig`] used by `BacktestNode`.
#[allow(missing_debug_implementations)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "venue config fields mirror the existing imperative backtest API"
)]
#[derive(bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
pub struct SimulatedVenueConfig {
    pub venue: Venue,
    pub oms_type: OmsType,
    pub account_type: AccountType,
    pub book_type: BookType,
    pub starting_balances: Vec<Money>,
    pub base_currency: Option<Currency>,
    // Left optional so the engine can fall back to an account-type-appropriate
    // default (10x for margin, 1x otherwise) when the caller has no preference.
    pub default_leverage: Option<Decimal>,
    #[builder(default)]
    pub leverages: AHashMap<InstrumentId, Decimal>,
    pub margin_model: Option<MarginModelAny>,
    #[builder(default)]
    pub modules: Vec<Box<dyn SimulationModule>>,
    #[builder(default)]
    pub fill_model: FillModelAny,
    #[builder(default)]
    pub fee_model: FeeModelAny,
    pub latency_model: Option<Box<dyn LatencyModel>>,
    #[builder(default = false)]
    pub routing: bool,
    #[builder(default = true)]
    pub reject_stop_orders: bool,
    #[builder(default = true)]
    pub support_gtd_orders: bool,
    #[builder(default = true)]
    pub support_contingent_orders: bool,
    #[builder(default = true)]
    pub use_position_ids: bool,
    #[builder(default = false)]
    pub use_random_ids: bool,
    #[builder(default = true)]
    pub use_reduce_only: bool,
    #[builder(default = true)]
    pub use_message_queue: bool,
    #[builder(default = false)]
    pub use_market_order_acks: bool,
    #[builder(default = true)]
    pub bar_execution: bool,
    #[builder(default = false)]
    pub bar_adaptive_high_low_ordering: bool,
    #[builder(default = true)]
    pub trade_execution: bool,
    #[builder(default = false)]
    pub liquidity_consumption: bool,
    #[builder(default = false)]
    pub allow_cash_borrowing: bool,
    #[builder(default = false)]
    pub frozen_account: bool,
    #[builder(default = false)]
    pub queue_position: bool,
    #[builder(default = false)]
    pub oto_full_trigger: bool,
    #[builder(default = 0)]
    pub price_protection_points: u32,
    /// Settlement prices for expiring instruments keyed by instrument ID.
    #[builder(default)]
    pub settlement_prices: AHashMap<InstrumentId, Price>,
    /// If liquidation of positions should be triggered when maintenance margin is breached.
    #[builder(default = false)]
    pub liquidation_enabled: bool,
    /// The ratio of equity to maintenance margin at which liquidation is triggered.
    /// A value of 1.0 means liquidation triggers when equity <= `maintenance_margin`.
    #[builder(default = 1.0)]
    pub liquidation_trigger_ratio: f64,
    /// If open orders should be canceled before closing positions during liquidation.
    #[builder(default = true)]
    pub liquidation_cancel_open_orders: bool,
}

impl<S: simulated_venue_config_builder::IsComplete> SimulatedVenueConfigBuilder<S> {
    /// Validates and builds the [`SimulatedVenueConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`SimulatedVenueConfig::validate`]).
    pub fn build(self) -> ConfigResult<SimulatedVenueConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl SimulatedVenueConfig {
    /// Validates the venue configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        if self.starting_balances.is_empty() {
            errors.push(ConfigError::empty_field("starting_balances"));
        }

        if let Some(default_leverage) = self.default_leverage {
            errors.check(
                default_leverage > Decimal::ZERO,
                ConfigError::range(
                    "default_leverage",
                    format!("must be positive, was {default_leverage}"),
                ),
            );
        }

        errors.check(
            self.liquidation_trigger_ratio.is_finite() && self.liquidation_trigger_ratio > 0.0,
            ConfigError::range(
                "liquidation_trigger_ratio",
                format!(
                    "must be a positive finite value, was {}",
                    self.liquidation_trigger_ratio
                ),
            ),
        );

        errors.into_result()
    }
}

/// Represents a venue configuration for one specific backtest engine.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.backtest",
        from_py_object,
        unsendable
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "venue config fields mirror the existing Rust and Python backtest surfaces"
)]
#[derive(Debug, Clone, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
pub struct BacktestVenueConfig {
    /// The name of the venue.
    #[builder(into)]
    name: Ustr,
    /// The order management system type for the exchange. If ``HEDGING`` will generate new position IDs.
    oms_type: OmsType,
    /// The account type for the exchange.
    account_type: AccountType,
    /// The default order book type.
    book_type: BookType,
    /// The starting account balances (specify one for a single asset account).
    #[builder(default)]
    starting_balances: Vec<String>,
    /// If multi-venue routing should be enabled for the execution client.
    #[builder(default)]
    routing: bool,
    /// If the account for this exchange is frozen (balances will not change).
    #[builder(default)]
    frozen_account: bool,
    /// If stop orders are rejected on submission if trigger price is in the market.
    #[builder(default = true)]
    reject_stop_orders: bool,
    /// If orders with GTD time in force will be supported by the venue.
    #[builder(default = true)]
    support_gtd_orders: bool,
    /// If contingent orders will be supported/respected by the venue.
    /// If False, then it's expected the strategy will be managing any contingent orders.
    #[builder(default = true)]
    support_contingent_orders: bool,
    /// If venue position IDs will be generated on order fills.
    #[builder(default = true)]
    use_position_ids: bool,
    /// If venue order IDs and position IDs will be random UUID4's.
    /// Trade IDs are always deterministic and not affected by this flag.
    #[builder(default)]
    use_random_ids: bool,
    /// If the `reduce_only` execution instruction on orders will be honored.
    #[builder(default = true)]
    use_reduce_only: bool,
    /// If bars should be processed by the matching engine(s) (and move the market).
    #[builder(default = true)]
    bar_execution: bool,
    /// Determines whether the processing order of bar prices is adaptive based on a heuristic.
    /// This setting is only relevant when `bar_execution` is True.
    /// If False, bar prices are always processed in the fixed order: Open, High, Low, Close.
    /// If True, the processing order adapts with the heuristic:
    /// - If High is closer to Open than Low then the processing order is Open, High, Low, Close.
    /// - If Low is closer to Open than High then the processing order is Open, Low, High, Close.
    #[builder(default)]
    bar_adaptive_high_low_ordering: bool,
    /// If trades should be processed by the matching engine(s) (and move the market).
    #[builder(default = true)]
    trade_execution: bool,
    /// If `OrderAccepted` events should be generated for market orders.
    #[builder(default)]
    use_market_order_acks: bool,
    /// If order book liquidity consumption should be tracked per level.
    #[builder(default)]
    liquidity_consumption: bool,
    /// If negative cash balances are allowed (borrowing).
    #[builder(default)]
    allow_cash_borrowing: bool,
    /// If limit order queue position tracking is enabled during trade execution.
    #[builder(default)]
    queue_position: bool,
    /// When OTO child orders are released relative to parent fills.
    #[builder(default)]
    oto_trigger_mode: OtoTriggerMode,
    /// The account base currency for the exchange. Use `None` for multi-currency accounts.
    base_currency: Option<Currency>,
    /// The account default leverage (for margin accounts).
    #[builder(default = Decimal::ONE)]
    default_leverage: Decimal,
    /// The instrument specific leverage configuration (for margin accounts).
    leverages: Option<AHashMap<InstrumentId, Decimal>>,
    /// The margin model for the venue.
    margin_model: Option<MarginModelAny>,
    /// The simulation modules for the venue.
    #[builder(default)]
    modules: Vec<SimulationModuleAny>,
    /// The fill model for the venue.
    fill_model: Option<FillModelAny>,
    /// The latency model for the venue.
    latency_model: Option<LatencyModelAny>,
    /// The fee model for the venue.
    fee_model: Option<FeeModelAny>,
    /// Defines an exchange-calculated price boundary to prevent a market order from being
    /// filled at an extremely aggressive price.
    #[builder(default)]
    price_protection_points: u32,
    /// Settlement prices for expiring instruments keyed by instrument ID.
    settlement_prices: Option<AHashMap<InstrumentId, f64>>,
    /// If liquidation of positions should be triggered when maintenance margin is breached.
    #[builder(default)]
    liquidation_enabled: bool,
    /// The ratio of equity to maintenance margin at which liquidation is triggered.
    /// A value of 1.0 means liquidation triggers when equity <= `maintenance_margin`.
    #[builder(default = 1.0)]
    liquidation_trigger_ratio: f64,
    /// If open orders should be canceled before closing positions during liquidation.
    #[builder(default = true)]
    liquidation_cancel_open_orders: bool,
}

impl<S: backtest_venue_config_builder::IsComplete> BacktestVenueConfigBuilder<S> {
    /// Validates and builds the [`BacktestVenueConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`BacktestVenueConfig::validate`]).
    pub fn build(self) -> ConfigResult<BacktestVenueConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl BacktestVenueConfig {
    /// Validates the venue configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        if self.name.is_empty() {
            errors.push(ConfigError::empty_field("name"));
        } else if let Err(e) = Venue::new_checked(self.name.as_str()) {
            errors.push(ConfigError::invalid_value(
                "name",
                format!("must be a valid venue identifier ({e})"),
            ));
        }
        errors.check(
            self.default_leverage > Decimal::ZERO,
            ConfigError::range(
                "default_leverage",
                format!("must be positive, was {}", self.default_leverage),
            ),
        );

        if let Some(leverages) = &self.leverages {
            for (instrument_id, leverage) in leverages {
                errors.check(
                    *leverage > Decimal::ZERO,
                    ConfigError::range(
                        "leverages",
                        format!("leverage for {instrument_id} must be positive, was {leverage}"),
                    ),
                );
            }
        }
        errors.check(
            self.liquidation_trigger_ratio.is_finite() && self.liquidation_trigger_ratio > 0.0,
            ConfigError::range(
                "liquidation_trigger_ratio",
                format!(
                    "must be a positive finite value, was {}",
                    self.liquidation_trigger_ratio
                ),
            ),
        );

        for balance in &self.starting_balances {
            if let Err(reason) = balance.parse::<Money>() {
                errors.push(ConfigError::invalid_format(
                    "starting_balances",
                    format!("a valid money string, was '{balance}' ({reason})"),
                ));
            }
        }

        errors.into_result()
    }

    #[must_use]
    pub fn name(&self) -> Ustr {
        self.name
    }

    #[must_use]
    pub fn oms_type(&self) -> OmsType {
        self.oms_type
    }

    #[must_use]
    pub fn account_type(&self) -> AccountType {
        self.account_type
    }

    #[must_use]
    pub fn book_type(&self) -> BookType {
        self.book_type
    }

    #[must_use]
    pub fn starting_balances(&self) -> &[String] {
        &self.starting_balances
    }

    #[must_use]
    pub fn routing(&self) -> bool {
        self.routing
    }

    #[must_use]
    pub fn frozen_account(&self) -> bool {
        self.frozen_account
    }

    #[must_use]
    pub fn reject_stop_orders(&self) -> bool {
        self.reject_stop_orders
    }

    #[must_use]
    pub fn support_gtd_orders(&self) -> bool {
        self.support_gtd_orders
    }

    #[must_use]
    pub fn support_contingent_orders(&self) -> bool {
        self.support_contingent_orders
    }

    #[must_use]
    pub fn use_position_ids(&self) -> bool {
        self.use_position_ids
    }

    #[must_use]
    pub fn use_random_ids(&self) -> bool {
        self.use_random_ids
    }

    #[must_use]
    pub fn use_reduce_only(&self) -> bool {
        self.use_reduce_only
    }

    #[must_use]
    pub fn bar_execution(&self) -> bool {
        self.bar_execution
    }

    #[must_use]
    pub fn bar_adaptive_high_low_ordering(&self) -> bool {
        self.bar_adaptive_high_low_ordering
    }

    #[must_use]
    pub fn trade_execution(&self) -> bool {
        self.trade_execution
    }

    #[must_use]
    pub fn use_market_order_acks(&self) -> bool {
        self.use_market_order_acks
    }

    #[must_use]
    pub fn liquidity_consumption(&self) -> bool {
        self.liquidity_consumption
    }

    #[must_use]
    pub fn allow_cash_borrowing(&self) -> bool {
        self.allow_cash_borrowing
    }

    #[must_use]
    pub fn queue_position(&self) -> bool {
        self.queue_position
    }

    #[must_use]
    pub fn oto_trigger_mode(&self) -> OtoTriggerMode {
        self.oto_trigger_mode
    }

    #[must_use]
    pub fn base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    #[must_use]
    pub fn default_leverage(&self) -> Decimal {
        self.default_leverage
    }

    #[must_use]
    pub fn leverages(&self) -> Option<&AHashMap<InstrumentId, Decimal>> {
        self.leverages.as_ref()
    }

    #[must_use]
    pub fn margin_model(&self) -> Option<&MarginModelAny> {
        self.margin_model.as_ref()
    }

    #[must_use]
    pub fn modules(&self) -> &[SimulationModuleAny] {
        &self.modules
    }

    #[must_use]
    pub fn fill_model(&self) -> Option<&FillModelAny> {
        self.fill_model.as_ref()
    }

    #[must_use]
    pub fn latency_model(&self) -> Option<&LatencyModelAny> {
        self.latency_model.as_ref()
    }

    #[must_use]
    pub fn fee_model(&self) -> Option<&FeeModelAny> {
        self.fee_model.as_ref()
    }

    #[must_use]
    pub fn price_protection_points(&self) -> u32 {
        self.price_protection_points
    }

    #[must_use]
    pub fn settlement_prices(&self) -> Option<&AHashMap<InstrumentId, f64>> {
        self.settlement_prices.as_ref()
    }

    #[must_use]
    pub fn liquidation_enabled(&self) -> bool {
        self.liquidation_enabled
    }

    #[must_use]
    pub fn liquidation_trigger_ratio(&self) -> f64 {
        self.liquidation_trigger_ratio
    }

    #[must_use]
    pub fn liquidation_cancel_open_orders(&self) -> bool {
        self.liquidation_cancel_open_orders
    }
}

/// Represents the data configuration for one specific backtest run.
#[derive(Debug, Clone, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.backtest",
        from_py_object,
        unsendable
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
pub struct BacktestDataConfig {
    /// The type of data to query from the catalog.
    data_type: NautilusDataType,
    /// The path to the data catalog.
    catalog_path: String,
    /// The `fsspec` filesystem protocol for the catalog.
    catalog_fs_protocol: Option<String>,
    /// The filesystem storage options for the catalog (e.g. cloud auth credentials).
    catalog_fs_storage_options: Option<AHashMap<String, String>>,
    /// Rust-specific storage options for the catalog backend.
    catalog_fs_rust_storage_options: Option<AHashMap<String, String>>,
    /// The instrument ID for the data configuration (single).
    instrument_id: Option<InstrumentId>,
    /// Multiple instrument IDs for the data configuration.
    instrument_ids: Option<Vec<InstrumentId>>,
    /// The start time for the data configuration.
    start_time: Option<UnixNanos>,
    /// The end time for the data configuration.
    end_time: Option<UnixNanos>,
    /// The additional filter expressions for the data catalog query.
    filter_expr: Option<String>,
    /// The client ID for the data configuration.
    client_id: Option<ClientId>,
    /// The metadata for the data catalog query.
    #[allow(dead_code)]
    metadata: Option<AHashMap<String, String>>,
    /// The bar specification for the data catalog query.
    bar_spec: Option<BarSpecification>,
    /// Explicit bar type strings for the data catalog query (e.g. "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    bar_types: Option<Vec<String>>,
    /// If directory-based file registration should be used for more efficient loading.
    #[builder(default)]
    optimize_file_loading: bool,
}

impl<S: backtest_data_config_builder::IsComplete> BacktestDataConfigBuilder<S> {
    /// Validates and builds the [`BacktestDataConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`BacktestDataConfig::validate`]).
    pub fn build(self) -> ConfigResult<BacktestDataConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl BacktestDataConfig {
    /// Validates the data configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        if self.catalog_path.trim().is_empty() {
            errors.push(ConfigError::empty_field("catalog_path"));
        }

        if let (Some(start), Some(end)) = (self.start_time, self.end_time) {
            errors.check(
                start <= end,
                ConfigError::range(
                    "start_time",
                    format!("must be <= end_time, was {start} > {end}"),
                ),
            );
        }

        let has_identifier = self.instrument_id.is_some()
            || self
                .instrument_ids
                .as_ref()
                .is_some_and(|ids| !ids.is_empty())
            || self.bar_types.as_ref().is_some_and(|bars| !bars.is_empty());
        errors.check(
            has_identifier,
            ConfigError::required_one_of(["instrument_id", "instrument_ids", "bar_types"]),
        );

        errors.into_result()
    }

    #[must_use]
    pub const fn data_type(&self) -> NautilusDataType {
        self.data_type
    }

    #[must_use]
    pub fn catalog_path(&self) -> &str {
        &self.catalog_path
    }

    #[must_use]
    pub fn catalog_fs_protocol(&self) -> Option<&str> {
        self.catalog_fs_protocol.as_deref()
    }

    #[must_use]
    pub fn catalog_fs_storage_options(&self) -> Option<&AHashMap<String, String>> {
        self.catalog_fs_storage_options.as_ref()
    }

    #[must_use]
    pub fn catalog_fs_rust_storage_options(&self) -> Option<&AHashMap<String, String>> {
        self.catalog_fs_rust_storage_options.as_ref()
    }

    #[must_use]
    pub fn instrument_id(&self) -> Option<InstrumentId> {
        self.instrument_id
    }

    #[must_use]
    pub fn instrument_ids(&self) -> Option<&[InstrumentId]> {
        self.instrument_ids.as_deref()
    }

    #[must_use]
    pub fn start_time(&self) -> Option<UnixNanos> {
        self.start_time
    }

    #[must_use]
    pub fn end_time(&self) -> Option<UnixNanos> {
        self.end_time
    }

    #[must_use]
    pub fn filter_expr(&self) -> Option<&str> {
        self.filter_expr.as_deref()
    }

    #[must_use]
    pub fn client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[must_use]
    pub fn bar_spec(&self) -> Option<BarSpecification> {
        self.bar_spec
    }

    #[must_use]
    pub fn bar_types(&self) -> Option<&[String]> {
        self.bar_types.as_deref()
    }

    #[must_use]
    pub fn optimize_file_loading(&self) -> bool {
        self.optimize_file_loading
    }

    /// Constructs identifier strings for catalog queries.
    ///
    /// Follows the same logic as Python's `BacktestDataConfig.query`:
    /// - For bars: prefer `bar_types`, else construct from instrument(s) + `bar_spec` + "-EXTERNAL"
    /// - For other types: use `instrument_id` or `instrument_ids`
    #[must_use]
    pub fn query_identifiers(&self) -> Option<Vec<String>> {
        if self.data_type == NautilusDataType::Bar {
            if let Some(bar_types) = &self.bar_types
                && !bar_types.is_empty()
            {
                return Some(bar_types.clone());
            }

            // Construct from instrument_id + bar_spec
            if let Some(bar_spec) = &self.bar_spec {
                if let Some(id) = self.instrument_id {
                    return Some(vec![format!("{id}-{bar_spec}-EXTERNAL")]);
                }

                if let Some(ids) = &self.instrument_ids {
                    let bar_types: Vec<String> = ids
                        .iter()
                        .map(|id| format!("{id}-{bar_spec}-EXTERNAL"))
                        .collect();

                    if !bar_types.is_empty() {
                        return Some(bar_types);
                    }
                }
            }
        }

        // Fallback: instrument_id or instrument_ids
        if let Some(id) = self.instrument_id {
            return Some(vec![id.to_string()]);
        }

        if let Some(ids) = &self.instrument_ids {
            let strs: Vec<String> = ids.iter().map(ToString::to_string).collect();
            if !strs.is_empty() {
                return Some(strs);
            }
        }

        None
    }

    /// Returns all instrument IDs referenced by this config.
    ///
    /// For `bar_types`, extracts the instrument ID from each bar type string.
    ///
    /// # Errors
    ///
    /// Returns an error if any bar type string cannot be parsed.
    pub fn get_instrument_ids(&self) -> anyhow::Result<Vec<InstrumentId>> {
        if let Some(id) = self.instrument_id {
            return Ok(vec![id]);
        }

        if let Some(ids) = &self.instrument_ids {
            return Ok(ids.clone());
        }

        if let Some(bar_types) = &self.bar_types {
            let ids = bar_types
                .iter()
                .map(|bt| {
                    bt.parse::<BarType>()
                        .map(|b| b.instrument_id())
                        .map_err(|_| anyhow::anyhow!("Invalid bar type string: '{bt}'"))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            return Ok(ids);
        }
        Ok(Vec::new())
    }
}

/// Represents the configuration for one specific backtest run.
/// This includes a backtest engine with its actors and strategies, with the external inputs of venues and data.
#[derive(Debug, Clone, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.backtest",
        from_py_object,
        unsendable
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
pub struct BacktestRunConfig {
    /// The unique identifier for this run configuration.
    #[builder(default = UUID4::new().to_string())]
    id: String,
    /// The venue configurations for the backtest run.
    venues: Vec<BacktestVenueConfig>,
    /// The data configurations for the backtest run.
    data: Vec<BacktestDataConfig>,
    /// The backtest engine configuration (the core system kernel).
    #[builder(default)]
    engine: BacktestEngineConfig,
    /// The number of data points to process in each chunk during streaming mode.
    /// If `None`, the backtest will run without streaming, loading all data at once.
    chunk_size: Option<usize>,
    /// If exceptions during build or run should interrupt processing.
    #[builder(default)]
    raise_exception: bool,
    /// If the backtest engine should be disposed on completion of the run.
    /// If `True`, then will drop data and all state.
    /// If `False`, then will *only* drop data.
    #[builder(default = true)]
    dispose_on_completion: bool,
    /// The start datetime (UTC) for the backtest run.
    /// If `None` engine runs from the start of the data.
    start: Option<UnixNanos>,
    /// The end datetime (UTC) for the backtest run.
    /// If `None` engine runs to the end of the data.
    end: Option<UnixNanos>,
}

impl<S: backtest_run_config_builder::IsComplete> BacktestRunConfigBuilder<S> {
    /// Validates and builds the [`BacktestRunConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`BacktestRunConfig::validate`]).
    pub fn build(self) -> ConfigResult<BacktestRunConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl BacktestRunConfig {
    /// Validates the run configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        if self.venues.is_empty() {
            errors.push(ConfigError::empty_field("venues"));
        }

        if let (Some(start), Some(end)) = (self.start, self.end) {
            errors.check(
                start <= end,
                ConfigError::range("start", format!("must be <= end, was {start} > {end}")),
            );
        }

        if let Some(chunk_size) = self.chunk_size {
            errors.check(
                chunk_size > 0,
                ConfigError::range("chunk_size", format!("must be positive, was {chunk_size}")),
            );
        }

        errors.into_result()
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn venues(&self) -> &[BacktestVenueConfig] {
        &self.venues
    }

    #[must_use]
    pub fn data(&self) -> &[BacktestDataConfig] {
        &self.data
    }

    #[must_use]
    pub fn engine(&self) -> &BacktestEngineConfig {
        &self.engine
    }

    #[must_use]
    pub fn chunk_size(&self) -> Option<usize> {
        self.chunk_size
    }

    #[must_use]
    pub fn raise_exception(&self) -> bool {
        self.raise_exception
    }

    #[must_use]
    pub fn dispose_on_completion(&self) -> bool {
        self.dispose_on_completion
    }

    #[must_use]
    pub fn start(&self) -> Option<UnixNanos> {
        self.start
    }

    #[must_use]
    pub fn end(&self) -> Option<UnixNanos> {
        self.end
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    macro_rules! minimal_builder {
        () => {
            BacktestVenueConfig::builder()
                .name("SIM")
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
        };
    }

    #[rstest]
    fn test_minimal_config_is_valid() {
        assert!(minimal_builder!().build().is_ok());
    }

    #[rstest]
    fn test_empty_name_rejected() {
        let result = BacktestVenueConfig::builder()
            .name("")
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .build();
        assert!(matches!(result, Err(ConfigError::EmptyField { field }) if field == "name"));
    }

    #[rstest]
    #[case("   ")]
    #[case("vénue")]
    fn test_invalid_venue_name_rejected(#[case] name: &str) {
        let result = BacktestVenueConfig::builder()
            .name(name)
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .build();
        assert!(matches!(result, Err(ConfigError::InvalidValue { field, .. }) if field == "name"));
    }

    #[rstest]
    #[case(Decimal::ZERO)]
    #[case(Decimal::from(-1))]
    fn test_non_positive_default_leverage_rejected(#[case] leverage: Decimal) {
        let result = minimal_builder!().default_leverage(leverage).build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "default_leverage")
        );
    }

    #[rstest]
    fn test_non_positive_instrument_leverage_rejected() {
        let mut leverages = AHashMap::new();
        leverages.insert(InstrumentId::from("ESZ21.GLBX"), Decimal::ZERO);
        let result = minimal_builder!().leverages(leverages).build();
        assert!(matches!(result, Err(ConfigError::Range { field, .. }) if field == "leverages"));
    }

    #[rstest]
    #[case(0.0)]
    #[case(-1.0)]
    #[case(f64::INFINITY)]
    #[case(f64::NAN)]
    fn test_invalid_liquidation_trigger_ratio_rejected(#[case] ratio: f64) {
        let result = minimal_builder!().liquidation_trigger_ratio(ratio).build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "liquidation_trigger_ratio")
        );
    }

    #[rstest]
    fn test_unparsable_starting_balance_rejected() {
        let result = minimal_builder!()
            .starting_balances(vec!["not a balance".to_string()])
            .build();
        assert!(
            matches!(result, Err(ConfigError::InvalidFormat { field, .. }) if field == "starting_balances")
        );
    }

    #[rstest]
    fn test_valid_starting_balance_accepted() {
        let result = minimal_builder!()
            .starting_balances(vec!["1_000_000 USD".to_string()])
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_multiple_violations_collected() {
        let result = BacktestVenueConfig::builder()
            .name("")
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .default_leverage(Decimal::ZERO)
            .starting_balances(vec!["bad".to_string()])
            .build();
        let ConfigError::Multiple { errors } = result.unwrap_err() else {
            panic!("expected ConfigError::Multiple");
        };
        assert_eq!(errors.len(), 3);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::EmptyField { field } if field == "name"))
        );
        assert!(
            errors.iter().any(
                |e| matches!(e, ConfigError::Range { field, .. } if field == "default_leverage")
            )
        );
        assert!(errors.iter().any(
            |e| matches!(e, ConfigError::InvalidFormat { field, .. } if field == "starting_balances")
        ));
    }

    #[rstest]
    fn test_minimal_data_config_is_valid() {
        let result = BacktestDataConfig::builder()
            .data_type(NautilusDataType::QuoteTick)
            .catalog_path("/tmp/catalog".to_string())
            .instrument_id(InstrumentId::from("ETH/USDT.BINANCE"))
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    #[case("")]
    #[case("   ")]
    fn test_empty_catalog_path_rejected(#[case] catalog_path: &str) {
        let result = BacktestDataConfig::builder()
            .data_type(NautilusDataType::QuoteTick)
            .catalog_path(catalog_path.to_string())
            .instrument_id(InstrumentId::from("ETH/USDT.BINANCE"))
            .build();
        assert!(
            matches!(result, Err(ConfigError::EmptyField { field }) if field == "catalog_path")
        );
    }

    #[rstest]
    fn test_inverted_time_range_rejected() {
        let result = BacktestDataConfig::builder()
            .data_type(NautilusDataType::QuoteTick)
            .catalog_path("/tmp/catalog".to_string())
            .instrument_id(InstrumentId::from("ETH/USDT.BINANCE"))
            .start_time(UnixNanos::from(5_000_000_000u64))
            .end_time(UnixNanos::from(1_000_000_000u64))
            .build();
        assert!(matches!(result, Err(ConfigError::Range { field, .. }) if field == "start_time"));
    }

    #[rstest]
    fn test_equal_time_range_accepted() {
        let result = BacktestDataConfig::builder()
            .data_type(NautilusDataType::QuoteTick)
            .catalog_path("/tmp/catalog".to_string())
            .instrument_id(InstrumentId::from("ETH/USDT.BINANCE"))
            .start_time(UnixNanos::from(1_000_000_000u64))
            .end_time(UnixNanos::from(1_000_000_000u64))
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_missing_identifier_rejected() {
        let result = BacktestDataConfig::builder()
            .data_type(NautilusDataType::QuoteTick)
            .catalog_path("/tmp/catalog".to_string())
            .build();
        assert!(matches!(result, Err(ConfigError::RequiredOneOf { fields }) if fields.len() == 3));
    }

    #[rstest]
    fn test_empty_instrument_ids_rejected() {
        let result = BacktestDataConfig::builder()
            .data_type(NautilusDataType::QuoteTick)
            .catalog_path("/tmp/catalog".to_string())
            .instrument_ids(vec![])
            .build();
        assert!(matches!(result, Err(ConfigError::RequiredOneOf { .. })));
    }

    #[rstest]
    fn test_bar_types_satisfies_identifier_requirement() {
        let result = BacktestDataConfig::builder()
            .data_type(NautilusDataType::Bar)
            .catalog_path("/tmp/catalog".to_string())
            .bar_types(vec!["ETH/USDT.BINANCE-1-MINUTE-LAST-EXTERNAL".to_string()])
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_data_config_multiple_violations_collected() {
        let result = BacktestDataConfig::builder()
            .data_type(NautilusDataType::QuoteTick)
            .catalog_path(String::new())
            .start_time(UnixNanos::from(5_000_000_000u64))
            .end_time(UnixNanos::from(1_000_000_000u64))
            .build();
        let ConfigError::Multiple { errors } = result.unwrap_err() else {
            panic!("expected ConfigError::Multiple");
        };
        assert_eq!(errors.len(), 3);
    }

    macro_rules! minimal_sim_builder {
        () => {
            SimulatedVenueConfig::builder()
                .venue(Venue::from("SIM"))
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
                .starting_balances(vec![Money::from("1_000_000 USD")])
        };
    }

    #[rstest]
    fn test_minimal_sim_config_is_valid() {
        assert!(minimal_sim_builder!().build().is_ok());
    }

    #[rstest]
    fn test_empty_starting_balances_rejected() {
        let result = SimulatedVenueConfig::builder()
            .venue(Venue::from("SIM"))
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![])
            .build();
        assert!(
            matches!(result, Err(ConfigError::EmptyField { field }) if field == "starting_balances")
        );
    }

    #[rstest]
    #[case(Decimal::ZERO)]
    #[case(Decimal::from(-1))]
    fn test_non_positive_sim_default_leverage_rejected(#[case] leverage: Decimal) {
        let result = minimal_sim_builder!().default_leverage(leverage).build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "default_leverage")
        );
    }

    #[rstest]
    fn test_positive_sim_default_leverage_accepted() {
        assert!(
            minimal_sim_builder!()
                .default_leverage(Decimal::from(5))
                .build()
                .is_ok()
        );
    }

    #[rstest]
    #[case(0.0)]
    #[case(-1.0)]
    #[case(f64::INFINITY)]
    #[case(f64::NAN)]
    fn test_invalid_sim_liquidation_trigger_ratio_rejected(#[case] ratio: f64) {
        let result = minimal_sim_builder!()
            .liquidation_trigger_ratio(ratio)
            .build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "liquidation_trigger_ratio")
        );
    }

    fn minimal_venue() -> BacktestVenueConfig {
        minimal_builder!().build().unwrap()
    }

    #[rstest]
    fn test_minimal_run_config_is_valid() {
        let result = BacktestRunConfig::builder()
            .venues(vec![minimal_venue()])
            .data(vec![])
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_run_config_requires_venues() {
        let result = BacktestRunConfig::builder()
            .venues(vec![])
            .data(vec![])
            .build();
        assert!(matches!(result, Err(ConfigError::EmptyField { field }) if field == "venues"));
    }

    #[rstest]
    fn test_run_config_inverted_time_range_rejected() {
        let result = BacktestRunConfig::builder()
            .venues(vec![minimal_venue()])
            .data(vec![])
            .start(UnixNanos::from(5_000_000_000u64))
            .end(UnixNanos::from(1_000_000_000u64))
            .build();
        assert!(matches!(result, Err(ConfigError::Range { field, .. }) if field == "start"));
    }

    #[rstest]
    fn test_run_config_equal_time_range_accepted() {
        let result = BacktestRunConfig::builder()
            .venues(vec![minimal_venue()])
            .data(vec![])
            .start(UnixNanos::from(1_000_000_000u64))
            .end(UnixNanos::from(1_000_000_000u64))
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_run_config_accepts_chunk_size() {
        let config = BacktestRunConfig::builder()
            .venues(vec![minimal_venue()])
            .data(vec![])
            .chunk_size(10)
            .build()
            .unwrap();
        assert_eq!(config.chunk_size(), Some(10));
    }

    #[rstest]
    fn test_run_config_zero_chunk_size_rejected() {
        let result = BacktestRunConfig::builder()
            .venues(vec![minimal_venue()])
            .data(vec![])
            .chunk_size(0)
            .build();
        assert!(matches!(result, Err(ConfigError::Range { field, .. }) if field == "chunk_size"));
    }

    #[rstest]
    fn test_run_config_multiple_violations_collected() {
        let result = BacktestRunConfig::builder()
            .venues(vec![])
            .data(vec![])
            .start(UnixNanos::from(5_000_000_000u64))
            .end(UnixNanos::from(1_000_000_000u64))
            .build();
        let ConfigError::Multiple { errors } = result.unwrap_err() else {
            panic!("expected ConfigError::Multiple");
        };
        assert_eq!(errors.len(), 2);
    }
}
