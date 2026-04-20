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

//! Python bindings for backtest configuration types.

use std::{collections::HashMap, time::Duration};

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::{
    data::BarSpecification,
    enums::{AccountType, BookType, OmsType, OtoTriggerMode},
    identifiers::{ClientId, InstrumentId, TraderId},
    types::Currency,
};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;
use pyo3::{Py, PyAny, Python};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::engine::{
    pyobject_to_fee_model_any, pyobject_to_fill_model_any, pyobject_to_latency_model_any,
    pyobject_to_margin_model_any, pyobject_to_simulation_module_any,
};
use crate::config::{
    BacktestDataConfig, BacktestEngineConfig, BacktestRunConfig, BacktestVenueConfig,
    NautilusDataType,
};

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl BacktestEngineConfig {
    /// Configuration for ``BacktestEngine`` instances.
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
        cache = None,
        msgbus = None,
        data_engine = None,
        risk_engine = None,
        exec_engine = None,
        portfolio = None,
    ))]
    #[expect(clippy::too_many_arguments)]
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
        cache: Option<CacheConfig>,
        msgbus: Option<MessageBusConfig>,
        data_engine: Option<DataEngineConfig>,
        risk_engine: Option<RiskEngineConfig>,
        exec_engine: Option<ExecutionEngineConfig>,
        portfolio: Option<PortfolioConfig>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            environment: Environment::Backtest,
            trader_id: trader_id.unwrap_or_default(),
            load_state: load_state.unwrap_or(defaults.load_state),
            save_state: save_state.unwrap_or(defaults.save_state),
            bypass_logging: bypass_logging.unwrap_or(defaults.bypass_logging),
            run_analysis: run_analysis.unwrap_or(defaults.run_analysis),
            timeout_connection: Duration::from_secs(timeout_connection.unwrap_or(60)),
            timeout_reconciliation: Duration::from_secs(timeout_reconciliation.unwrap_or(30)),
            timeout_portfolio: Duration::from_secs(timeout_portfolio.unwrap_or(10)),
            timeout_disconnection: Duration::from_secs(timeout_disconnection.unwrap_or(10)),
            delay_post_stop: Duration::from_secs(delay_post_stop.unwrap_or(10)),
            timeout_shutdown: Duration::from_secs(timeout_shutdown.unwrap_or(5)),
            logging: logging.unwrap_or_default(),
            instance_id,
            cache,
            msgbus,
            data_engine,
            risk_engine,
            exec_engine,
            portfolio,
            streaming: None,
        }
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

    #[getter]
    #[pyo3(name = "cache")]
    fn py_cache(&self) -> Option<CacheConfig> {
        self.cache.clone()
    }

    #[getter]
    #[pyo3(name = "msgbus")]
    fn py_msgbus(&self) -> Option<MessageBusConfig> {
        self.msgbus.clone()
    }

    #[getter]
    #[pyo3(name = "data_engine")]
    fn py_data_engine(&self) -> Option<DataEngineConfig> {
        self.data_engine.clone()
    }

    #[getter]
    #[pyo3(name = "risk_engine")]
    fn py_risk_engine(&self) -> Option<RiskEngineConfig> {
        self.risk_engine.clone()
    }

    #[getter]
    #[pyo3(name = "exec_engine")]
    fn py_exec_engine(&self) -> Option<ExecutionEngineConfig> {
        self.exec_engine.clone()
    }

    #[getter]
    #[pyo3(name = "portfolio")]
    const fn py_portfolio(&self) -> Option<PortfolioConfig> {
        self.portfolio
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl BacktestVenueConfig {
    /// Represents a venue configuration for one specific backtest engine.
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
        queue_position = None,
        oto_trigger_mode = None,
        base_currency = None,
        default_leverage = None,
        leverages = None,
        margin_model = None,
        modules = None,
        fill_model = None,
        latency_model = None,
        fee_model = None,
        price_protection_points = None,
        settlement_prices = None,
    ))]
    #[expect(clippy::too_many_arguments)]
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
        queue_position: Option<bool>,
        oto_trigger_mode: Option<OtoTriggerMode>,
        base_currency: Option<Currency>,
        default_leverage: Option<Decimal>,
        leverages: Option<HashMap<InstrumentId, Decimal>>,
        margin_model: Option<Py<PyAny>>,
        modules: Option<Vec<Py<PyAny>>>,
        fill_model: Option<Py<PyAny>>,
        latency_model: Option<Py<PyAny>>,
        fee_model: Option<Py<PyAny>>,
        price_protection_points: Option<u32>,
        settlement_prices: Option<HashMap<InstrumentId, f64>>,
    ) -> pyo3::PyResult<Self> {
        let margin_model = margin_model
            .map(|obj| Python::attach(|py| pyobject_to_margin_model_any(py, obj.bind(py))))
            .transpose()?;
        let modules = modules
            .map(|objs| {
                objs.into_iter()
                    .map(|obj| {
                        Python::attach(|py| pyobject_to_simulation_module_any(py, obj.bind(py)))
                    })
                    .collect::<pyo3::PyResult<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        let fill_model = fill_model
            .map(|obj| Python::attach(|py| pyobject_to_fill_model_any(py, obj.bind(py))))
            .transpose()?;
        let latency_model = latency_model
            .map(|obj| Python::attach(|py| pyobject_to_latency_model_any(py, obj.bind(py))))
            .transpose()?;
        let fee_model = fee_model
            .map(|obj| Python::attach(|py| pyobject_to_fee_model_any(py, obj.bind(py))))
            .transpose()?;

        Ok(Self::builder()
            .name(Ustr::from(name))
            .oms_type(oms_type)
            .account_type(account_type)
            .book_type(book_type)
            .starting_balances(starting_balances)
            .maybe_routing(routing)
            .maybe_frozen_account(frozen_account)
            .maybe_reject_stop_orders(reject_stop_orders)
            .maybe_support_gtd_orders(support_gtd_orders)
            .maybe_support_contingent_orders(support_contingent_orders)
            .maybe_use_position_ids(use_position_ids)
            .maybe_use_random_ids(use_random_ids)
            .maybe_use_reduce_only(use_reduce_only)
            .maybe_bar_execution(bar_execution)
            .maybe_bar_adaptive_high_low_ordering(bar_adaptive_high_low_ordering)
            .maybe_trade_execution(trade_execution)
            .maybe_use_market_order_acks(use_market_order_acks)
            .maybe_liquidity_consumption(liquidity_consumption)
            .maybe_allow_cash_borrowing(allow_cash_borrowing)
            .maybe_queue_position(queue_position)
            .maybe_oto_trigger_mode(oto_trigger_mode)
            .maybe_base_currency(base_currency)
            .maybe_default_leverage(default_leverage)
            .maybe_leverages(leverages.map(|m| m.into_iter().collect()))
            .maybe_margin_model(margin_model)
            .modules(modules)
            .maybe_fill_model(fill_model)
            .maybe_latency_model(latency_model)
            .maybe_fee_model(fee_model)
            .maybe_price_protection_points(price_protection_points)
            .maybe_settlement_prices(settlement_prices.map(|m| m.into_iter().collect()))
            .build())
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        self.name().as_str()
    }

    #[getter]
    #[pyo3(name = "oms_type")]
    fn py_oms_type(&self) -> OmsType {
        self.oms_type()
    }

    #[getter]
    #[pyo3(name = "account_type")]
    fn py_account_type(&self) -> AccountType {
        self.account_type()
    }

    #[getter]
    #[pyo3(name = "book_type")]
    fn py_book_type(&self) -> BookType {
        self.book_type()
    }

    #[getter]
    #[pyo3(name = "starting_balances")]
    fn py_starting_balances(&self) -> Vec<String> {
        self.starting_balances().to_vec()
    }

    #[getter]
    #[pyo3(name = "bar_execution")]
    fn py_bar_execution(&self) -> bool {
        self.bar_execution()
    }

    #[getter]
    #[pyo3(name = "trade_execution")]
    fn py_trade_execution(&self) -> bool {
        self.trade_execution()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl BacktestDataConfig {
    /// Represents the data configuration for one specific backtest run.
    #[new]
    #[pyo3(signature = (
        data_type,
        catalog_path,
        catalog_fs_protocol = None,
        catalog_fs_storage_options = None,
        catalog_fs_rust_storage_options = None,
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
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        data_type: &str,
        catalog_path: String,
        catalog_fs_protocol: Option<String>,
        catalog_fs_storage_options: Option<HashMap<String, String>>,
        catalog_fs_rust_storage_options: Option<HashMap<String, String>>,
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
        Ok(Self::builder()
            .data_type(data_type)
            .catalog_path(catalog_path)
            .maybe_catalog_fs_protocol(catalog_fs_protocol)
            .maybe_catalog_fs_storage_options(
                catalog_fs_storage_options.map(|m| m.into_iter().collect()),
            )
            .maybe_catalog_fs_rust_storage_options(
                catalog_fs_rust_storage_options.map(|m| m.into_iter().collect()),
            )
            .maybe_instrument_id(instrument_id)
            .maybe_instrument_ids(instrument_ids)
            .maybe_start_time(start_time.map(UnixNanos::from))
            .maybe_end_time(end_time.map(UnixNanos::from))
            .maybe_filter_expr(filter_expr)
            .maybe_client_id(client_id)
            .maybe_metadata(metadata.map(|m| m.into_iter().collect()))
            .maybe_bar_spec(bar_spec)
            .maybe_bar_types(bar_types)
            .maybe_optimize_file_loading(optimize_file_loading)
            .build())
    }

    #[getter]
    #[pyo3(name = "data_type")]
    fn py_data_type(&self) -> String {
        self.data_type().to_string()
    }

    #[getter]
    #[pyo3(name = "catalog_path")]
    fn py_catalog_path(&self) -> &str {
        self.catalog_path()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> Option<InstrumentId> {
        self.instrument_id()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl BacktestRunConfig {
    /// Represents the configuration for one specific backtest run.
    /// This includes a backtest engine with its actors and strategies, with the external inputs of venues and data.
    #[new]
    #[pyo3(signature = (
        venues,
        data,
        engine = None,
        id = None,
        chunk_size = None,
        raise_exception = None,
        dispose_on_completion = None,
        start = None,
        end = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        venues: Vec<BacktestVenueConfig>,
        data: Vec<BacktestDataConfig>,
        engine: Option<BacktestEngineConfig>,
        id: Option<String>,
        chunk_size: Option<usize>,
        raise_exception: Option<bool>,
        dispose_on_completion: Option<bool>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Self {
        Self::builder()
            .venues(venues)
            .data(data)
            .maybe_engine(engine)
            .maybe_id(id)
            .maybe_chunk_size(chunk_size)
            .maybe_raise_exception(raise_exception)
            .maybe_dispose_on_completion(dispose_on_completion)
            .maybe_start(start.map(UnixNanos::from))
            .maybe_end(end.map(UnixNanos::from))
            .build()
    }

    #[getter]
    #[pyo3(name = "id")]
    fn py_id(&self) -> &str {
        self.id()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
