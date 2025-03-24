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

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::BarSpecification,
    enums::{AccountType, BookType, OmsType},
    identifiers::{ClientId, InstrumentId},
    types::Currency,
};
use nautilus_system::config::NautilusKernelConfig;
use ustr::Ustr;

/// Configuration for ``BacktestEngine`` instances.
#[derive(Debug, Clone)]
pub struct BacktestEngineConfig {
    /// The kernel configuration for the backtest engine.
    pub kernel: NautilusKernelConfig,
    /// If logging should be bypassed.
    bypass_logging: bool,
    /// If post backtest performance analysis should be run.
    run_analysis: bool,
}

impl BacktestEngineConfig {
    #[must_use]
    pub fn new(
        kernel: NautilusKernelConfig,
        bypass_logging: Option<bool>,
        run_analysis: Option<bool>,
    ) -> Self {
        Self {
            kernel,
            bypass_logging: bypass_logging.unwrap_or(false),
            run_analysis: run_analysis.unwrap_or(true),
        }
    }
}

impl Default for BacktestEngineConfig {
    fn default() -> Self {
        Self {
            kernel: NautilusKernelConfig::default(),
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
