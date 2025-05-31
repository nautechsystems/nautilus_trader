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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::collections::HashMap;

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::{
    data::BarSpecification,
    enums::{AccountType, BookType, OmsType},
    identifiers::{ClientId, InstrumentId, TraderId},
    types::Currency,
};
use nautilus_persistence::config::StreamingConfig;
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_system::config::NautilusKernelConfig;
use ustr::Ustr;

/// Configuration for ``BacktestEngine`` instances.
#[derive(Debug, Clone)]
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
        timeout_connection: Option<u32>,
        timeout_reconciliation: Option<u32>,
        timeout_portfolio: Option<u32>,
        timeout_disconnection: Option<u32>,
        timeout_post_stop: Option<u32>,
        timeout_shutdown: Option<u32>,
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
            timeout_connection: timeout_connection.unwrap_or(60),
            timeout_reconciliation: timeout_reconciliation.unwrap_or(30),
            timeout_portfolio: timeout_portfolio.unwrap_or(10),
            timeout_disconnection: timeout_disconnection.unwrap_or(10),
            timeout_post_stop: timeout_post_stop.unwrap_or(10),
            timeout_shutdown: timeout_shutdown.unwrap_or(5),
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
            timeout_connection: 60,
            timeout_reconciliation: 30,
            timeout_portfolio: 10,
            timeout_disconnection: 10,
            timeout_post_stop: 10,
            timeout_shutdown: 5,
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

/// Represents a venue configuration for one specific backtest engine.
#[derive(Debug, Clone)]
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
    /// The account base currency for the exchange. Use `None` for multi-currency accounts.
    base_currency: Option<Currency>,
    /// The account default leverage (for margin accounts).
    default_leverage: Option<f64>,
    /// The instrument specific leverage configuration (for margin accounts).
    leverages: Option<HashMap<Currency, f64>>,
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
        starting_balances: Vec<String>,
        base_currency: Option<Currency>,
        default_leverage: Option<f64>,
        leverages: Option<HashMap<Currency, f64>>,
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
            trade_execution: trade_execution.unwrap_or(false),
            starting_balances,
            base_currency,
            default_leverage,
            leverages,
        }
    }
}

#[derive(Debug, Clone)]
/// Represents the data configuration for one specific backtest run.
pub struct BacktestDataConfig {
    /// The path to the data catalog.
    catalog_path: String,
    /// The `fsspec` filesystem protocol for the catalog.
    catalog_fs_protocol: Option<String>,
    /// The instrument ID for the data configuration.
    instrument_id: Option<InstrumentId>,
    /// The start time for the data configuration.
    start_time: Option<UnixNanos>,
    /// The end time for the data configuration.
    end_time: Option<UnixNanos>,
    /// The additional filter expressions for the data catalog query.
    filter_expr: Option<String>,
    /// The client ID for the data configuration.
    client_id: Option<ClientId>,
    /// The metadata for the data catalog query.
    metadata: Option<HashMap<String, String>>,
    /// The bar specification for the data catalog query.
    bar_spec: Option<BarSpecification>,
}

impl BacktestDataConfig {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        catalog_path: String,
        catalog_fs_protocol: Option<String>,
        instrument_id: Option<InstrumentId>,
        start_time: Option<UnixNanos>,
        end_time: Option<UnixNanos>,
        filter_expr: Option<String>,
        client_id: Option<ClientId>,
        metadata: Option<HashMap<String, String>>,
        bar_spec: Option<BarSpecification>,
    ) -> Self {
        Self {
            catalog_path,
            catalog_fs_protocol,
            instrument_id,
            start_time,
            end_time,
            filter_expr,
            client_id,
            metadata,
            bar_spec,
        }
    }
}

/// Represents the configuration for one specific backtest run.
/// This includes a backtest engine with its actors and strategies, with the external inputs of venues and data.
#[derive(Debug, Clone)]
pub struct BacktestRunConfig {
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
    #[must_use]
    pub fn new(
        venues: Vec<BacktestVenueConfig>,
        data: Vec<BacktestDataConfig>,
        engine: BacktestEngineConfig,
        chunk_size: Option<usize>,
        dispose_on_completion: Option<bool>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> Self {
        Self {
            venues,
            data,
            engine,
            chunk_size,
            dispose_on_completion: dispose_on_completion.unwrap_or(true),
            start,
            end,
        }
    }
}
