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

use std::collections::HashMap;

use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::BarSpecification,
    enums::{AccountType, BookType, OmsType, OtoTriggerMode},
    identifiers::{ClientId, InstrumentId, TraderId},
    types::Currency,
};
use ustr::Ustr;

use crate::config::{
    BacktestDataConfig, BacktestEngineConfig, BacktestRunConfig, BacktestVenueConfig,
    NautilusDataType,
};

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
        queue_position = None,
        oto_trigger_mode = None,
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
        queue_position: Option<bool>,
        oto_trigger_mode: Option<OtoTriggerMode>,
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
            queue_position,
            oto_trigger_mode,
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
        self.id()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
