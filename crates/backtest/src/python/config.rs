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
    msgbus::MessageBusConfig, python::config_error_to_pyvalue_err,
};
use nautilus_core::{UUID4, UnixNanos, python::to_pyvalue_err};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::{
    engine::config::ExecutionEngineConfig,
    models::{fill::FillModelAny, latency::LatencyModelAny},
    python::{
        fee::{fee_model_any_to_pyobject, pyobject_to_fee_model_any},
        fill::pyobject_to_fill_model_any,
    },
};
use nautilus_model::{
    accounts::margin_model::MarginModelAny,
    data::BarSpecification,
    enums::{AccountType, BookType, OmsType, OtoTriggerMode},
    identifiers::{ClientId, InstrumentId, TraderId},
    types::Currency,
};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_trading::ImportableControllerConfig;
use pyo3::{IntoPyObjectExt, Py, PyAny, PyResult, Python};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::engine::{
    pyobject_to_latency_model_any, pyobject_to_margin_model_any, pyobject_to_simulation_module_any,
};
use crate::{
    config::{
        BacktestDataConfig, BacktestEngineConfig, BacktestRunConfig, BacktestVenueConfig,
        NautilusDataType,
    },
    modules::SimulationModuleAny,
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
        shutdown_on_error = None,
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
        controller = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        trader_id: Option<TraderId>,
        load_state: Option<bool>,
        save_state: Option<bool>,
        shutdown_on_error: Option<bool>,
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
        controller: Option<ImportableControllerConfig>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            environment: Environment::Backtest,
            trader_id: trader_id.unwrap_or_default(),
            load_state: load_state.unwrap_or(defaults.load_state),
            save_state: save_state.unwrap_or(defaults.save_state),
            shutdown_on_error: shutdown_on_error.unwrap_or(defaults.shutdown_on_error),
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
            controller,
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
    #[pyo3(name = "shutdown_on_error")]
    const fn py_shutdown_on_error(&self) -> bool {
        self.shutdown_on_error
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
    #[pyo3(name = "timeout_connection")]
    fn py_timeout_connection(&self) -> f64 {
        self.timeout_connection.as_secs_f64()
    }

    #[getter]
    #[pyo3(name = "timeout_reconciliation")]
    fn py_timeout_reconciliation(&self) -> f64 {
        self.timeout_reconciliation.as_secs_f64()
    }

    #[getter]
    #[pyo3(name = "timeout_portfolio")]
    fn py_timeout_portfolio(&self) -> f64 {
        self.timeout_portfolio.as_secs_f64()
    }

    #[getter]
    #[pyo3(name = "timeout_disconnection")]
    fn py_timeout_disconnection(&self) -> f64 {
        self.timeout_disconnection.as_secs_f64()
    }

    #[getter]
    #[pyo3(name = "delay_post_stop")]
    fn py_delay_post_stop(&self) -> f64 {
        self.delay_post_stop.as_secs_f64()
    }

    #[getter]
    #[pyo3(name = "timeout_shutdown")]
    fn py_timeout_shutdown(&self) -> f64 {
        self.timeout_shutdown.as_secs_f64()
    }

    #[getter]
    #[pyo3(name = "logging")]
    fn py_logging(&self) -> LoggerConfig {
        self.logging.clone()
    }

    #[getter]
    #[pyo3(name = "instance_id")]
    const fn py_instance_id(&self) -> Option<UUID4> {
        self.instance_id
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

    #[getter]
    #[pyo3(name = "controller")]
    fn py_controller(&self) -> Option<ImportableControllerConfig> {
        self.controller.clone()
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
        liquidation_enabled = None,
        liquidation_trigger_ratio = None,
        liquidation_cancel_open_orders = None,
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
        liquidation_enabled: Option<bool>,
        liquidation_trigger_ratio: Option<f64>,
        liquidation_cancel_open_orders: Option<bool>,
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
            .map(|obj| Python::attach(|py| pyobject_to_fill_model_any(obj.bind(py))))
            .transpose()?;
        let latency_model = latency_model
            .map(|obj| Python::attach(|py| pyobject_to_latency_model_any(py, obj.bind(py))))
            .transpose()?;
        let fee_model = fee_model
            .map(|obj| Python::attach(|py| pyobject_to_fee_model_any(obj.bind(py))))
            .transpose()?;

        Self::builder()
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
            .maybe_liquidation_enabled(liquidation_enabled)
            .maybe_liquidation_trigger_ratio(liquidation_trigger_ratio)
            .maybe_liquidation_cancel_open_orders(liquidation_cancel_open_orders)
            .build()
            .map_err(config_error_to_pyvalue_err)
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
    #[pyo3(name = "routing")]
    fn py_routing(&self) -> bool {
        self.routing()
    }

    #[getter]
    #[pyo3(name = "frozen_account")]
    fn py_frozen_account(&self) -> bool {
        self.frozen_account()
    }

    #[getter]
    #[pyo3(name = "reject_stop_orders")]
    fn py_reject_stop_orders(&self) -> bool {
        self.reject_stop_orders()
    }

    #[getter]
    #[pyo3(name = "support_gtd_orders")]
    fn py_support_gtd_orders(&self) -> bool {
        self.support_gtd_orders()
    }

    #[getter]
    #[pyo3(name = "support_contingent_orders")]
    fn py_support_contingent_orders(&self) -> bool {
        self.support_contingent_orders()
    }

    #[getter]
    #[pyo3(name = "use_position_ids")]
    fn py_use_position_ids(&self) -> bool {
        self.use_position_ids()
    }

    #[getter]
    #[pyo3(name = "use_random_ids")]
    fn py_use_random_ids(&self) -> bool {
        self.use_random_ids()
    }

    #[getter]
    #[pyo3(name = "use_reduce_only")]
    fn py_use_reduce_only(&self) -> bool {
        self.use_reduce_only()
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

    #[getter]
    #[pyo3(name = "bar_adaptive_high_low_ordering")]
    fn py_bar_adaptive_high_low_ordering(&self) -> bool {
        self.bar_adaptive_high_low_ordering()
    }

    #[getter]
    #[pyo3(name = "use_market_order_acks")]
    fn py_use_market_order_acks(&self) -> bool {
        self.use_market_order_acks()
    }

    #[getter]
    #[pyo3(name = "liquidity_consumption")]
    fn py_liquidity_consumption(&self) -> bool {
        self.liquidity_consumption()
    }

    #[getter]
    #[pyo3(name = "allow_cash_borrowing")]
    fn py_allow_cash_borrowing(&self) -> bool {
        self.allow_cash_borrowing()
    }

    #[getter]
    #[pyo3(name = "queue_position")]
    fn py_queue_position(&self) -> bool {
        self.queue_position()
    }

    #[getter]
    #[pyo3(name = "oto_trigger_mode")]
    fn py_oto_trigger_mode(&self) -> OtoTriggerMode {
        self.oto_trigger_mode()
    }

    #[getter]
    #[pyo3(name = "base_currency")]
    fn py_base_currency(&self) -> Option<Currency> {
        self.base_currency()
    }

    #[getter]
    #[pyo3(name = "default_leverage")]
    fn py_default_leverage(&self) -> Decimal {
        self.default_leverage()
    }

    #[getter]
    #[pyo3(name = "leverages")]
    fn py_leverages(&self) -> Option<HashMap<InstrumentId, Decimal>> {
        self.leverages().map(|leverages| {
            leverages
                .iter()
                .map(|(key, value)| (*key, *value))
                .collect()
        })
    }

    #[getter]
    #[pyo3(name = "margin_model")]
    fn py_margin_model(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.margin_model()
            .map(|model| margin_model_any_to_pyobject(py, model))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "modules")]
    fn py_modules(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.modules()
            .iter()
            .map(|module| simulation_module_any_to_pyobject(py, module))
            .collect()
    }

    #[getter]
    #[pyo3(name = "fill_model")]
    fn py_fill_model(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.fill_model()
            .map(|model| fill_model_any_to_pyobject(py, model))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "latency_model")]
    fn py_latency_model(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.latency_model()
            .map(|model| latency_model_any_to_pyobject(py, model))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "fee_model")]
    fn py_fee_model(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.fee_model()
            .map(|model| fee_model_any_to_pyobject(py, model))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "price_protection_points")]
    fn py_price_protection_points(&self) -> u32 {
        self.price_protection_points()
    }

    #[getter]
    #[pyo3(name = "settlement_prices")]
    fn py_settlement_prices(&self) -> Option<HashMap<InstrumentId, f64>> {
        self.settlement_prices()
            .map(|prices| prices.iter().map(|(key, value)| (*key, *value)).collect())
    }

    #[getter]
    #[pyo3(name = "liquidation_enabled")]
    fn py_liquidation_enabled(&self) -> bool {
        self.liquidation_enabled()
    }

    #[getter]
    #[pyo3(name = "liquidation_trigger_ratio")]
    fn py_liquidation_trigger_ratio(&self) -> f64 {
        self.liquidation_trigger_ratio()
    }

    #[getter]
    #[pyo3(name = "liquidation_cancel_open_orders")]
    fn py_liquidation_cancel_open_orders(&self) -> bool {
        self.liquidation_cancel_open_orders()
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
            .map_err(to_pyvalue_err)?;
        Self::builder()
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
            .build()
            .map_err(config_error_to_pyvalue_err)
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

    #[getter]
    #[pyo3(name = "catalog_fs_protocol")]
    fn py_catalog_fs_protocol(&self) -> Option<&str> {
        self.catalog_fs_protocol()
    }

    #[getter]
    #[pyo3(name = "catalog_fs_storage_option_keys")]
    fn py_catalog_fs_storage_option_keys(&self) -> Option<Vec<String>> {
        self.catalog_fs_storage_options().map(|options| {
            let mut keys = options.keys().cloned().collect::<Vec<_>>();
            keys.sort_unstable();
            keys
        })
    }

    #[getter]
    #[pyo3(name = "catalog_fs_rust_storage_option_keys")]
    fn py_catalog_fs_rust_storage_option_keys(&self) -> Option<Vec<String>> {
        self.catalog_fs_rust_storage_options().map(|options| {
            let mut keys = options.keys().cloned().collect::<Vec<_>>();
            keys.sort_unstable();
            keys
        })
    }

    #[getter]
    #[pyo3(name = "instrument_ids")]
    fn py_instrument_ids(&self) -> Option<Vec<InstrumentId>> {
        self.instrument_ids().map(<[InstrumentId]>::to_vec)
    }

    #[getter]
    #[pyo3(name = "start_time")]
    fn py_start_time(&self) -> Option<u64> {
        self.start_time().map(|timestamp| timestamp.as_u64())
    }

    #[getter]
    #[pyo3(name = "end_time")]
    fn py_end_time(&self) -> Option<u64> {
        self.end_time().map(|timestamp| timestamp.as_u64())
    }

    #[getter]
    #[pyo3(name = "filter_expr")]
    fn py_filter_expr(&self) -> Option<&str> {
        self.filter_expr()
    }

    #[getter]
    #[pyo3(name = "client_id")]
    fn py_client_id(&self) -> Option<ClientId> {
        self.client_id()
    }

    #[getter]
    #[pyo3(name = "metadata")]
    fn py_metadata(&self) -> Option<HashMap<String, String>> {
        self.metadata().map(|metadata| {
            metadata
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
    }

    #[getter]
    #[pyo3(name = "bar_spec")]
    fn py_bar_spec(&self) -> Option<BarSpecification> {
        self.bar_spec()
    }

    #[getter]
    #[pyo3(name = "bar_types")]
    fn py_bar_types(&self) -> Option<Vec<String>> {
        self.bar_types().map(<[String]>::to_vec)
    }

    #[getter]
    #[pyo3(name = "optimize_file_loading")]
    fn py_optimize_file_loading(&self) -> bool {
        self.optimize_file_loading()
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
    ) -> pyo3::PyResult<Self> {
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
            .map_err(config_error_to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "id")]
    fn py_id(&self) -> &str {
        self.id()
    }

    #[getter]
    #[pyo3(name = "venues")]
    fn py_venues(&self) -> Vec<BacktestVenueConfig> {
        self.venues().to_vec()
    }

    #[getter]
    #[pyo3(name = "data")]
    fn py_data(&self) -> Vec<BacktestDataConfig> {
        self.data().to_vec()
    }

    #[getter]
    #[pyo3(name = "engine")]
    fn py_engine(&self) -> BacktestEngineConfig {
        self.engine().clone()
    }

    #[getter]
    #[pyo3(name = "chunk_size")]
    fn py_chunk_size(&self) -> Option<usize> {
        self.chunk_size()
    }

    #[getter]
    #[pyo3(name = "raise_exception")]
    fn py_raise_exception(&self) -> bool {
        self.raise_exception()
    }

    #[getter]
    #[pyo3(name = "dispose_on_completion")]
    fn py_dispose_on_completion(&self) -> bool {
        self.dispose_on_completion()
    }

    #[getter]
    #[pyo3(name = "start")]
    fn py_start(&self) -> Option<u64> {
        self.start().map(|timestamp| timestamp.as_u64())
    }

    #[getter]
    #[pyo3(name = "end")]
    fn py_end(&self) -> Option<u64> {
        self.end().map(|timestamp| timestamp.as_u64())
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

fn margin_model_any_to_pyobject(py: Python<'_>, model: &MarginModelAny) -> PyResult<Py<PyAny>> {
    match model {
        MarginModelAny::Standard(model) => (*model).into_py_any(py),
        MarginModelAny::Leveraged(model) => (*model).into_py_any(py),
    }
}

fn simulation_module_any_to_pyobject(
    py: Python<'_>,
    module: &SimulationModuleAny,
) -> PyResult<Py<PyAny>> {
    match module {
        SimulationModuleAny::FXRolloverInterest(module) => module.clone().into_py_any(py),
    }
}

fn latency_model_any_to_pyobject(py: Python<'_>, model: &LatencyModelAny) -> PyResult<Py<PyAny>> {
    match model {
        LatencyModelAny::Static(model) => model.clone().into_py_any(py),
    }
}

fn fill_model_any_to_pyobject(py: Python<'_>, model: &FillModelAny) -> PyResult<Py<PyAny>> {
    match model {
        FillModelAny::Default(model) => model.clone().into_py_any(py),
        FillModelAny::BestPrice(model) => model.clone().into_py_any(py),
        FillModelAny::OneTickSlippage(model) => model.clone().into_py_any(py),
        FillModelAny::Probabilistic(model) => model.clone().into_py_any(py),
        FillModelAny::TwoTier(model) => model.clone().into_py_any(py),
        FillModelAny::ThreeTier(model) => model.clone().into_py_any(py),
        FillModelAny::LimitOrderPartialFill(model) => model.clone().into_py_any(py),
        FillModelAny::SizeAware(model) => model.clone().into_py_any(py),
        FillModelAny::CompetitionAware(model) => model.clone().into_py_any(py),
        FillModelAny::VolumeSensitive(model) => model.clone().into_py_any(py),
        FillModelAny::MarketHours(model) => model.clone().into_py_any(py),
    }
}
