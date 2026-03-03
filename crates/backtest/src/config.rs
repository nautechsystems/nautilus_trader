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

use std::{collections::HashMap, fmt::Display, str::FromStr, time::Duration};

use ahash::AHashMap;
use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::{
    data::{BarSpecification, BarType},
    enums::{AccountType, BookType, OmsType},
    identifiers::{ClientId, InstrumentId, TraderId},
    types::Currency,
};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_system::config::{NautilusKernelConfig, StreamingConfig};
use ustr::Ustr;

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
            stringify!(InstrumentClose) => Ok(Self::InstrumentClose),
            _ => anyhow::bail!("Invalid `NautilusDataType`: '{s}'"),
        }
    }
}

/// Configuration for ``BacktestEngine`` instances.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.backtest", from_py_object)
)]
pub struct BacktestEngineConfig {
    /// The kernel environment context.
    pub environment: Environment,
    /// The trader ID for the node.
    pub trader_id: TraderId,
    /// If trading strategy state should be loaded from the database on start.
    pub load_state: bool,
    /// If trading strategy state should be saved to the database on stop.
    pub save_state: bool,
    /// The logging configuration for the kernel.
    pub logging: LoggerConfig,
    /// The unique instance identifier for the kernel.
    pub instance_id: Option<UUID4>,
    /// The timeout for all clients to connect and initialize.
    pub timeout_connection: Duration,
    /// The timeout for execution state to reconcile.
    pub timeout_reconciliation: Duration,
    /// The timeout for portfolio to initialize margins and unrealized pnls.
    pub timeout_portfolio: Duration,
    /// The timeout for all engine clients to disconnect.
    pub timeout_disconnection: Duration,
    /// The delay after stopping the node to await residual events before final shutdown.
    pub delay_post_stop: Duration,
    /// The timeout to await pending tasks cancellation during shutdown.
    pub timeout_shutdown: Duration,
    /// The cache configuration.
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
    pub bypass_logging: bool,
    /// If post backtest performance analysis should be run.
    pub run_analysis: bool,
}

impl BacktestEngineConfig {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        environment: Environment,
        trader_id: TraderId,
        load_state: Option<bool>,
        save_state: Option<bool>,
        bypass_logging: Option<bool>,
        run_analysis: Option<bool>,
        timeout_connection: Option<u64>,
        timeout_reconciliation: Option<u64>,
        timeout_portfolio: Option<u64>,
        timeout_disconnection: Option<u64>,
        delay_post_stop: Option<u64>,
        timeout_shutdown: Option<u64>,
        logging: Option<LoggerConfig>,
        instance_id: Option<UUID4>,
        cache: Option<CacheConfig>,
        msgbus: Option<MessageBusConfig>,
        data_engine: Option<DataEngineConfig>,
        risk_engine: Option<RiskEngineConfig>,
        exec_engine: Option<ExecutionEngineConfig>,
        portfolio: Option<PortfolioConfig>,
        streaming: Option<StreamingConfig>,
    ) -> Self {
        Self {
            environment,
            trader_id,
            load_state: load_state.unwrap_or(false),
            save_state: save_state.unwrap_or(false),
            logging: logging.unwrap_or_default(),
            instance_id,
            timeout_connection: Duration::from_secs(timeout_connection.unwrap_or(60)),
            timeout_reconciliation: Duration::from_secs(timeout_reconciliation.unwrap_or(30)),
            timeout_portfolio: Duration::from_secs(timeout_portfolio.unwrap_or(10)),
            timeout_disconnection: Duration::from_secs(timeout_disconnection.unwrap_or(10)),
            delay_post_stop: Duration::from_secs(delay_post_stop.unwrap_or(10)),
            timeout_shutdown: Duration::from_secs(timeout_shutdown.unwrap_or(5)),
            cache,
            msgbus,
            data_engine,
            risk_engine,
            exec_engine,
            portfolio,
            streaming,
            bypass_logging: bypass_logging.unwrap_or(false),
            run_analysis: run_analysis.unwrap_or(true),
        }
    }
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
        self.portfolio.clone()
    }

    fn streaming(&self) -> Option<StreamingConfig> {
        self.streaming.clone()
    }
}

impl Default for BacktestEngineConfig {
    fn default() -> Self {
        Self {
            environment: Environment::Backtest,
            trader_id: TraderId::default(),
            load_state: false,
            save_state: false,
            logging: LoggerConfig::default(),
            instance_id: None,
            timeout_connection: Duration::from_secs(60),
            timeout_reconciliation: Duration::from_secs(30),
            timeout_portfolio: Duration::from_secs(10),
            timeout_disconnection: Duration::from_secs(10),
            delay_post_stop: Duration::from_secs(10),
            timeout_shutdown: Duration::from_secs(5),
            cache: None,
            msgbus: None,
            data_engine: None,
            risk_engine: None,
            exec_engine: None,
            portfolio: None,
            streaming: None,
            bypass_logging: false,
            run_analysis: true,
        }
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BacktestEngineConfig {
    #[new]
    #[pyo3(signature = (
        trader_id = None,
        load_state = None,
        save_state = None,
        bypass_logging = None,
        run_analysis = None,
        timeout_connection = None,
        timeout_reconciliation = None,
        timeout_portfolio = None,
        timeout_disconnection = None,
        delay_post_stop = None,
        timeout_shutdown = None,
        logging = None,
        instance_id = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        trader_id: Option<TraderId>,
        load_state: Option<bool>,
        save_state: Option<bool>,
        bypass_logging: Option<bool>,
        run_analysis: Option<bool>,
        timeout_connection: Option<u64>,
        timeout_reconciliation: Option<u64>,
        timeout_portfolio: Option<u64>,
        timeout_disconnection: Option<u64>,
        delay_post_stop: Option<u64>,
        timeout_shutdown: Option<u64>,
        logging: Option<LoggerConfig>,
        instance_id: Option<UUID4>,
    ) -> Self {
        Self::new(
            Environment::Backtest,
            trader_id.unwrap_or_default(),
            load_state,
            save_state,
            bypass_logging,
            run_analysis,
            timeout_connection,
            timeout_reconciliation,
            timeout_portfolio,
            timeout_disconnection,
            delay_post_stop,
            timeout_shutdown,
            logging,
            instance_id,
            None, // cache
            None, // msgbus
            None, // data_engine
            None, // risk_engine
            None, // exec_engine
            None, // portfolio
            None, // streaming
        )
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "load_state")]
    const fn py_load_state(&self) -> bool {
        self.load_state
    }

    #[getter]
    #[pyo3(name = "save_state")]
    const fn py_save_state(&self) -> bool {
        self.save_state
    }

    #[getter]
    #[pyo3(name = "bypass_logging")]
    const fn py_bypass_logging(&self) -> bool {
        self.bypass_logging
    }

    #[getter]
    #[pyo3(name = "run_analysis")]
    const fn py_run_analysis(&self) -> bool {
        self.run_analysis
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

/// Represents a venue configuration for one specific backtest engine.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.backtest", from_py_object)
)]
pub struct BacktestVenueConfig {
    /// The name of the venue.
    name: Ustr,
    /// The order management system type for the exchange. If ``HEDGING`` will generate new position IDs.
    oms_type: OmsType,
    /// The account type for the exchange.
    account_type: AccountType,
    /// The default order book type.
    book_type: BookType,
    /// The starting account balances (specify one for a single asset account).
    starting_balances: Vec<String>,
    /// If multi-venue routing should be enabled for the execution client.
    routing: bool,
    /// If the account for this exchange is frozen (balances will not change).
    frozen_account: bool,
    /// If stop orders are rejected on submission if trigger price is in the market.
    reject_stop_orders: bool,
    /// If orders with GTD time in force will be supported by the venue.
    support_gtd_orders: bool,
    /// If contingent orders will be supported/respected by the venue.
    /// If False, then it's expected the strategy will be managing any contingent orders.
    support_contingent_orders: bool,
    /// If venue position IDs will be generated on order fills.
    use_position_ids: bool,
    /// If all venue generated identifiers will be random UUID4's.
    use_random_ids: bool,
    /// If the `reduce_only` execution instruction on orders will be honored.
    use_reduce_only: bool,
    /// If bars should be processed by the matching engine(s) (and move the market).
    bar_execution: bool,
    /// Determines whether the processing order of bar prices is adaptive based on a heuristic.
    /// This setting is only relevant when `bar_execution` is True.
    /// If False, bar prices are always processed in the fixed order: Open, High, Low, Close.
    /// If True, the processing order adapts with the heuristic:
    /// - If High is closer to Open than Low then the processing order is Open, High, Low, Close.
    /// - If Low is closer to Open than High then the processing order is Open, Low, High, Close.
    bar_adaptive_high_low_ordering: bool,
    /// If trades should be processed by the matching engine(s) (and move the market).
    trade_execution: bool,
    /// If `OrderAccepted` events should be generated for market orders.
    use_market_order_acks: bool,
    /// If order book liquidity consumption should be tracked per level.
    liquidity_consumption: bool,
    /// If negative cash balances are allowed (borrowing).
    allow_cash_borrowing: bool,
    /// The account base currency for the exchange. Use `None` for multi-currency accounts.
    base_currency: Option<Currency>,
    /// The account default leverage (for margin accounts).
    default_leverage: Option<f64>,
    /// The instrument specific leverage configuration (for margin accounts).
    leverages: Option<AHashMap<InstrumentId, f64>>,
    /// Defines an exchange-calculated price boundary to prevent a market order from being
    /// filled at an extremely aggressive price.
    price_protection_points: u32,
}

impl BacktestVenueConfig {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        name: Ustr,
        oms_type: OmsType,
        account_type: AccountType,
        book_type: BookType,
        routing: Option<bool>,
        frozen_account: Option<bool>,
        reject_stop_orders: Option<bool>,
        support_gtd_orders: Option<bool>,
        support_contingent_orders: Option<bool>,
        use_position_ids: Option<bool>,
        use_random_ids: Option<bool>,
        use_reduce_only: Option<bool>,
        bar_execution: Option<bool>,
        bar_adaptive_high_low_ordering: Option<bool>,
        trade_execution: Option<bool>,
        use_market_order_acks: Option<bool>,
        liquidity_consumption: Option<bool>,
        allow_cash_borrowing: Option<bool>,
        starting_balances: Vec<String>,
        base_currency: Option<Currency>,
        default_leverage: Option<f64>,
        leverages: Option<AHashMap<InstrumentId, f64>>,
        price_protection_points: Option<u32>,
    ) -> Self {
        Self {
            name,
            oms_type,
            account_type,
            book_type,
            routing: routing.unwrap_or(false),
            frozen_account: frozen_account.unwrap_or(false),
            reject_stop_orders: reject_stop_orders.unwrap_or(true),
            support_gtd_orders: support_gtd_orders.unwrap_or(true),
            support_contingent_orders: support_contingent_orders.unwrap_or(true),
            use_position_ids: use_position_ids.unwrap_or(true),
            use_random_ids: use_random_ids.unwrap_or(false),
            use_reduce_only: use_reduce_only.unwrap_or(true),
            bar_execution: bar_execution.unwrap_or(true),
            bar_adaptive_high_low_ordering: bar_adaptive_high_low_ordering.unwrap_or(false),
            trade_execution: trade_execution.unwrap_or(true),
            use_market_order_acks: use_market_order_acks.unwrap_or(false),
            liquidity_consumption: liquidity_consumption.unwrap_or(false),
            allow_cash_borrowing: allow_cash_borrowing.unwrap_or(false),
            starting_balances,
            base_currency,
            default_leverage,
            leverages,
            price_protection_points: price_protection_points.unwrap_or(0),
        }
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
    pub fn base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    #[must_use]
    pub fn default_leverage(&self) -> Option<f64> {
        self.default_leverage
    }

    #[must_use]
    pub fn leverages(&self) -> Option<&AHashMap<InstrumentId, f64>> {
        self.leverages.as_ref()
    }

    #[must_use]
    pub fn price_protection_points(&self) -> u32 {
        self.price_protection_points
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BacktestVenueConfig {
    #[new]
    #[pyo3(signature = (
        name,
        oms_type,
        account_type,
        book_type,
        starting_balances,
        routing = None,
        frozen_account = None,
        reject_stop_orders = None,
        support_gtd_orders = None,
        support_contingent_orders = None,
        use_position_ids = None,
        use_random_ids = None,
        use_reduce_only = None,
        bar_execution = None,
        bar_adaptive_high_low_ordering = None,
        trade_execution = None,
        use_market_order_acks = None,
        liquidity_consumption = None,
        allow_cash_borrowing = None,
        base_currency = None,
        default_leverage = None,
        leverages = None,
        price_protection_points = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        name: &str,
        oms_type: OmsType,
        account_type: AccountType,
        book_type: BookType,
        starting_balances: Vec<String>,
        routing: Option<bool>,
        frozen_account: Option<bool>,
        reject_stop_orders: Option<bool>,
        support_gtd_orders: Option<bool>,
        support_contingent_orders: Option<bool>,
        use_position_ids: Option<bool>,
        use_random_ids: Option<bool>,
        use_reduce_only: Option<bool>,
        bar_execution: Option<bool>,
        bar_adaptive_high_low_ordering: Option<bool>,
        trade_execution: Option<bool>,
        use_market_order_acks: Option<bool>,
        liquidity_consumption: Option<bool>,
        allow_cash_borrowing: Option<bool>,
        base_currency: Option<Currency>,
        default_leverage: Option<f64>,
        leverages: Option<HashMap<InstrumentId, f64>>,
        price_protection_points: Option<u32>,
    ) -> Self {
        let leverages = leverages.map(|m| m.into_iter().collect());
        Self::new(
            Ustr::from(name),
            oms_type,
            account_type,
            book_type,
            routing,
            frozen_account,
            reject_stop_orders,
            support_gtd_orders,
            support_contingent_orders,
            use_position_ids,
            use_random_ids,
            use_reduce_only,
            bar_execution,
            bar_adaptive_high_low_ordering,
            trade_execution,
            use_market_order_acks,
            liquidity_consumption,
            allow_cash_borrowing,
            starting_balances,
            base_currency,
            default_leverage,
            leverages,
            price_protection_points,
        )
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        self.name.as_str()
    }

    #[getter]
    #[pyo3(name = "oms_type")]
    const fn py_oms_type(&self) -> OmsType {
        self.oms_type
    }

    #[getter]
    #[pyo3(name = "account_type")]
    const fn py_account_type(&self) -> AccountType {
        self.account_type
    }

    #[getter]
    #[pyo3(name = "book_type")]
    const fn py_book_type(&self) -> BookType {
        self.book_type
    }

    #[getter]
    #[pyo3(name = "starting_balances")]
    fn py_starting_balances(&self) -> Vec<String> {
        self.starting_balances.clone()
    }

    #[getter]
    #[pyo3(name = "bar_execution")]
    const fn py_bar_execution(&self) -> bool {
        self.bar_execution
    }

    #[getter]
    #[pyo3(name = "trade_execution")]
    const fn py_trade_execution(&self) -> bool {
        self.trade_execution
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

/// Represents the data configuration for one specific backtest run.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.backtest", from_py_object)
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
    optimize_file_loading: bool,
}

impl BacktestDataConfig {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        data_type: NautilusDataType,
        catalog_path: String,
        catalog_fs_protocol: Option<String>,
        catalog_fs_storage_options: Option<AHashMap<String, String>>,
        instrument_id: Option<InstrumentId>,
        instrument_ids: Option<Vec<InstrumentId>>,
        start_time: Option<UnixNanos>,
        end_time: Option<UnixNanos>,
        filter_expr: Option<String>,
        client_id: Option<ClientId>,
        metadata: Option<AHashMap<String, String>>,
        bar_spec: Option<BarSpecification>,
        bar_types: Option<Vec<String>>,
        optimize_file_loading: Option<bool>,
    ) -> Self {
        Self {
            data_type,
            catalog_path,
            catalog_fs_protocol,
            catalog_fs_storage_options,
            instrument_id,
            instrument_ids,
            start_time,
            end_time,
            filter_expr,
            client_id,
            metadata,
            bar_spec,
            bar_types,
            optimize_file_loading: optimize_file_loading.unwrap_or(false),
        }
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
    /// - For bars: prefer `bar_types`, else construct from instrument(s) + bar_spec + "-EXTERNAL"
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
    /// For bar_types, extracts the instrument ID from each bar type string.
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

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BacktestDataConfig {
    #[new]
    #[pyo3(signature = (
        data_type,
        catalog_path,
        catalog_fs_protocol = None,
        catalog_fs_storage_options = None,
        instrument_id = None,
        instrument_ids = None,
        start_time = None,
        end_time = None,
        filter_expr = None,
        client_id = None,
        metadata = None,
        bar_spec = None,
        bar_types = None,
        optimize_file_loading = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        data_type: &str,
        catalog_path: String,
        catalog_fs_protocol: Option<String>,
        catalog_fs_storage_options: Option<HashMap<String, String>>,
        instrument_id: Option<InstrumentId>,
        instrument_ids: Option<Vec<InstrumentId>>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        filter_expr: Option<String>,
        client_id: Option<ClientId>,
        metadata: Option<HashMap<String, String>>,
        bar_spec: Option<BarSpecification>,
        bar_types: Option<Vec<String>>,
        optimize_file_loading: Option<bool>,
    ) -> pyo3::PyResult<Self> {
        let data_type = data_type
            .parse::<NautilusDataType>()
            .map_err(nautilus_core::python::to_pyvalue_err)?;
        let catalog_fs_storage_options =
            catalog_fs_storage_options.map(|m| m.into_iter().collect());
        let metadata = metadata.map(|m| m.into_iter().collect());
        Ok(Self::new(
            data_type,
            catalog_path,
            catalog_fs_protocol,
            catalog_fs_storage_options,
            instrument_id,
            instrument_ids,
            start_time.map(UnixNanos::from),
            end_time.map(UnixNanos::from),
            filter_expr,
            client_id,
            metadata,
            bar_spec,
            bar_types,
            optimize_file_loading,
        ))
    }

    #[getter]
    #[pyo3(name = "data_type")]
    fn py_data_type(&self) -> String {
        self.data_type.to_string()
    }

    #[getter]
    #[pyo3(name = "catalog_path")]
    fn py_catalog_path(&self) -> &str {
        &self.catalog_path
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> Option<InstrumentId> {
        self.instrument_id
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

/// Represents the configuration for one specific backtest run.
/// This includes a backtest engine with its actors and strategies, with the external inputs of venues and data.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.backtest", from_py_object)
)]
pub struct BacktestRunConfig {
    /// The unique identifier for this run configuration.
    id: String,
    /// The venue configurations for the backtest run.
    venues: Vec<BacktestVenueConfig>,
    /// The data configurations for the backtest run.
    data: Vec<BacktestDataConfig>,
    /// The backtest engine configuration (the core system kernel).
    engine: BacktestEngineConfig,
    /// The number of data points to process in each chunk during streaming mode.
    /// If `None`, the backtest will run without streaming, loading all data at once.
    chunk_size: Option<usize>,
    /// If the backtest engine should be disposed on completion of the run.
    /// If `True`, then will drop data and all state.
    /// If `False`, then will *only* drop data.
    dispose_on_completion: bool,
    /// The start datetime (UTC) for the backtest run.
    /// If `None` engine runs from the start of the data.
    start: Option<UnixNanos>,
    /// The end datetime (UTC) for the backtest run.
    /// If `None` engine runs to the end of the data.
    end: Option<UnixNanos>,
}

impl BacktestRunConfig {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        id: Option<String>,
        venues: Vec<BacktestVenueConfig>,
        data: Vec<BacktestDataConfig>,
        engine: BacktestEngineConfig,
        chunk_size: Option<usize>,
        dispose_on_completion: Option<bool>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> Self {
        Self {
            id: id.unwrap_or_else(|| UUID4::new().to_string()),
            venues,
            data,
            engine,
            chunk_size,
            dispose_on_completion: dispose_on_completion.unwrap_or(true),
            start,
            end,
        }
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

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BacktestRunConfig {
    #[new]
    #[pyo3(signature = (
        venues,
        data,
        engine = None,
        id = None,
        chunk_size = None,
        dispose_on_completion = None,
        start = None,
        end = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        venues: Vec<BacktestVenueConfig>,
        data: Vec<BacktestDataConfig>,
        engine: Option<BacktestEngineConfig>,
        id: Option<String>,
        chunk_size: Option<usize>,
        dispose_on_completion: Option<bool>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Self {
        Self::new(
            id,
            venues,
            data,
            engine.unwrap_or_default(),
            chunk_size,
            dispose_on_completion,
            start.map(UnixNanos::from),
            end.map(UnixNanos::from),
        )
    }

    #[getter]
    #[pyo3(name = "id")]
    fn py_id(&self) -> &str {
        &self.id
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
