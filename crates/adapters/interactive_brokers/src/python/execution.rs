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

use std::collections::HashMap;

use nautilus_common::{clients::ExecutionClient, live::get_runtime};
use nautilus_core::python::to_pyruntime_err;
use nautilus_model::{
    enums::OrderSide,
    identifiers::{
        ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId, StrategyId, TraderId,
        VenueOrderId,
    },
    python::orders::pyobject_to_order_any,
    types::{Price, Quantity},
};
use pyo3::prelude::*;

use crate::execution::InteractiveBrokersExecutionClient;

#[cfg(feature = "python")]
#[pymethods]
impl InteractiveBrokersExecutionClient {
    #[new]
    #[pyo3(signature = (_msgbus, _cache, _clock, instrument_provider, config))]
    fn py_new(
        _msgbus: Py<PyAny>,
        _cache: Py<PyAny>,
        _clock: Py<PyAny>,
        instrument_provider: crate::providers::instruments::InteractiveBrokersInstrumentProvider,
        config: crate::config::InteractiveBrokersExecClientConfig,
    ) -> PyResult<Self> {
        Self::new_for_python(config, instrument_provider).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "set_event_callback")]
    fn py_set_event_callback(&self, callback: Py<PyAny>) {
        self.register_python_event_callback(callback);
    }

    /// Returns the client ID.
    #[getter]
    pub fn client_id(&self) -> ClientId {
        ExecutionClient::client_id(self)
    }

    /// Returns whether the client is connected.
    #[getter]
    pub fn is_connected(&self) -> bool {
        ExecutionClient::is_connected(self)
    }

    /// Returns whether the client is disconnected.
    #[getter]
    pub fn is_disconnected(&self) -> bool {
        !ExecutionClient::is_connected(self)
    }

    #[pyo3(name = "connect")]
    fn py_connect(&mut self) -> PyResult<()> {
        get_runtime()
            .block_on(ExecutionClient::connect(self))
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect(&mut self) -> PyResult<()> {
        get_runtime()
            .block_on(ExecutionClient::disconnect(self))
            .map_err(to_pyruntime_err)
    }

    /// Submit a single order.
    ///
    /// # Arguments
    ///
    /// * `trader_id` - The trader ID
    /// * `order` - The order to submit (as PyO3 OrderAny)
    /// * `instrument_id` - The instrument ID
    /// * `strategy_id` - The strategy ID
    /// * `exec_algorithm_id` - Optional execution algorithm ID
    /// * `position_id` - Optional position ID
    /// * `params` - Optional parameters dictionary
    ///
    /// # Errors
    ///
    /// Returns an error if submission fails.
    #[pyo3(name = "submit_order")]
    fn py_submit_order(
        &self,
        py: Python,
        trader_id: TraderId,
        order: Py<PyAny>,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        position_id: Option<PositionId>,
        params: Option<HashMap<String, String>>,
    ) -> PyResult<()> {
        let order_any = pyobject_to_order_any(py, order)?;
        self.submit_order_for_python(
            trader_id,
            order_any,
            instrument_id,
            strategy_id,
            exec_algorithm_id,
            position_id,
            params,
        )
        .map_err(to_pyruntime_err)
    }

    /// Submit a list of orders (OCA group).
    ///
    /// # Arguments
    ///
    /// * `trader_id` - The trader ID
    /// * `strategy_id` - The strategy ID
    /// * `orders` - List of orders (Python list of Order objects)
    /// * `exec_algorithm_id` - Optional execution algorithm ID
    /// * `position_id` - Optional position ID
    /// * `params` - Optional parameters dictionary
    ///
    /// # Errors
    ///
    /// Returns an error if submission fails.
    #[pyo3(name = "submit_order_list")]
    fn py_submit_order_list(
        &self,
        py: Python,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<Py<PyAny>>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        position_id: Option<PositionId>,
        params: Option<HashMap<String, String>>,
    ) -> PyResult<()> {
        let mut order_anys = Vec::new();
        for order_py in orders {
            order_anys.push(pyobject_to_order_any(py, order_py)?);
        }
        self.submit_order_list_for_python(
            trader_id,
            strategy_id,
            order_anys,
            exec_algorithm_id,
            position_id,
            params,
        )
        .map_err(to_pyruntime_err)
    }

    /// Modify an existing order.
    ///
    /// # Arguments
    ///
    /// * `client_order_id` - The client order ID to modify
    /// * `venue_order_id` - The venue order ID
    /// * `instrument_id` - The instrument ID
    /// * `quantity` - Optional new quantity
    /// * `price` - Optional new price
    /// * `trigger_price` - Optional new trigger price
    ///
    /// # Errors
    ///
    /// Returns an error if modification fails.
    #[pyo3(name = "modify_order")]
    fn py_modify_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        params: Option<HashMap<String, String>>,
    ) -> PyResult<()> {
        self.modify_order_for_python(
            trader_id,
            strategy_id,
            client_order_id,
            venue_order_id,
            instrument_id,
            quantity,
            price,
            trigger_price,
            params,
        )
        .map_err(to_pyruntime_err)
    }

    /// Cancel a specific order.
    ///
    /// # Arguments
    ///
    /// * `client_order_id` - The client order ID to cancel
    /// * `venue_order_id` - The venue order ID
    /// * `instrument_id` - The instrument ID
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    #[pyo3(name = "cancel_order")]
    fn py_cancel_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        params: Option<HashMap<String, String>>,
    ) -> PyResult<()> {
        self.cancel_order_for_python(
            trader_id,
            strategy_id,
            client_order_id,
            venue_order_id,
            instrument_id,
            params,
        )
        .map_err(to_pyruntime_err)
    }

    /// Cancel all orders for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    #[pyo3(name = "cancel_all_orders")]
    fn py_cancel_all_orders(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        params: Option<HashMap<String, String>>,
    ) -> PyResult<()> {
        self.cancel_all_orders_for_python(trader_id, strategy_id, instrument_id, order_side, params)
            .map_err(to_pyruntime_err)
    }

    /// Batch cancel multiple orders.
    ///
    /// # Arguments
    ///
    /// * `client_order_ids` - List of client order IDs to cancel
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    #[pyo3(name = "batch_cancel_orders")]
    fn py_batch_cancel_orders(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_ids: Vec<ClientOrderId>,
        params: Option<HashMap<String, String>>,
    ) -> PyResult<()> {
        self.batch_cancel_orders_for_python(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_ids,
            params,
        )
        .map_err(to_pyruntime_err)
    }

    /// Query the status of an account.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    #[pyo3(name = "query_account")]
    fn py_query_account(&self, trader_id: TraderId) -> PyResult<()> {
        self.query_account_for_python(trader_id)
            .map_err(to_pyruntime_err)
    }

    /// Query the status of an order.
    ///
    /// # Arguments
    ///
    /// * `trader_id` - The trader ID
    /// * `strategy_id` - The strategy ID
    /// * `instrument_id` - The instrument ID
    /// * `client_order_id` - The client order ID
    /// * `venue_order_id` - Optional venue order ID (IB order id)
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    #[pyo3(name = "query_order")]
    fn py_query_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<()> {
        self.query_order_for_python(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
        )
        .map_err(to_pyruntime_err)
    }

    /// Generate execution mass status (order reports, fill reports, position reports).
    ///
    /// # Arguments
    ///
    /// * `lookback_mins` - Optional lookback in minutes for closed orders/fills/positions
    ///
    /// # Returns
    ///
    /// Returns an ExecutionMassStatus if successful, None otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if generation fails.
    #[pyo3(name = "generate_mass_status")]
    fn py_generate_mass_status<'py>(
        &self,
        py: Python<'py>,
        lookback_mins: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self;
        let fut = async move { client.generate_mass_status(lookback_mins).await };
        let rt = get_runtime();

        match rt.block_on(fut) {
            Ok(Some(mass_status)) => {
                let py_mass_status = Py::new(py, mass_status).map_err(to_pyruntime_err)?;
                Ok(py_mass_status.bind(py).as_any().to_owned())
            }
            Ok(None) => Ok(py.None().bind(py).as_any().to_owned()),
            Err(e) => Err(to_pyruntime_err(format!(
                "Failed to generate mass status: {e}"
            ))),
        }
    }

    /// Generate a single order status report.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - Optional instrument ID
    /// * `client_order_id` - Optional client order ID
    /// * `venue_order_id` - Optional venue order ID
    ///
    /// # Returns
    ///
    /// Returns the order status report if found, `None` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    #[pyo3(name = "generate_order_status_report")]
    fn py_generate_order_status_report<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<InstrumentId>,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self;
        let fut = async move {
            client
                .generate_order_status_report_for_python(
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                )
                .await
        };
        let rt = get_runtime();

        match rt.block_on(fut) {
            Ok(Some(report)) => {
                let py_report = Py::new(py, report).map_err(to_pyruntime_err)?;
                Ok(py_report.bind(py).as_any().to_owned())
            }
            Ok(None) => Ok(py.None().bind(py).as_any().to_owned()),
            Err(e) => Err(to_pyruntime_err(format!(
                "Failed to generate order_status_report: {e}"
            ))),
        }
    }

    /// Generate multiple order status reports.
    ///
    /// # Arguments
    ///
    /// * `open_only` - Whether to return only open orders
    /// * `instrument_id` - Optional instrument ID to filter by
    /// * `start` - Optional start timestamp (Unix nanoseconds)
    /// * `end` - Optional end timestamp (Unix nanoseconds)
    ///
    /// # Returns
    ///
    /// Returns a list of order status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    #[pyo3(name = "generate_order_status_reports")]
    fn py_generate_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        open_only: bool,
        instrument_id: Option<InstrumentId>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self;
        let fut = async move {
            client
                .generate_order_status_reports_for_python(open_only, instrument_id, start, end)
                .await
        };
        let rt = get_runtime();

        match rt.block_on(fut) {
            Ok(reports) => {
                let py_reports: Result<Vec<_>, _> =
                    reports.into_iter().map(|r| Py::new(py, r)).collect();
                let py_list = pyo3::types::PyList::new(py, py_reports.map_err(to_pyruntime_err)?)
                    .map_err(to_pyruntime_err)?;
                Ok(py_list.as_any().to_owned())
            }
            Err(e) => Err(to_pyruntime_err(format!(
                "Failed to generate order status reports: {e}"
            ))),
        }
    }

    /// Generate fill reports.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - Optional instrument ID to filter by
    /// * `venue_order_id` - Optional venue order ID to filter by
    /// * `start` - Optional start timestamp (Unix nanoseconds)
    /// * `end` - Optional end timestamp (Unix nanoseconds)
    ///
    /// # Returns
    ///
    /// Returns a list of fill reports.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    #[pyo3(name = "generate_fill_reports")]
    fn py_generate_fill_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<InstrumentId>,
        venue_order_id: Option<VenueOrderId>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self;
        let fut = async move {
            client
                .generate_fill_reports_for_python(instrument_id, venue_order_id, start, end)
                .await
        };
        let rt = get_runtime();

        match rt.block_on(fut) {
            Ok(reports) => {
                // Convert to Python list
                let py_reports: Result<Vec<_>, _> =
                    reports.into_iter().map(|r| Py::new(py, r)).collect();
                let py_list = pyo3::types::PyList::new(py, py_reports.map_err(to_pyruntime_err)?)
                    .map_err(to_pyruntime_err)?;
                Ok(py_list.as_any().to_owned())
            }
            Err(e) => Err(to_pyruntime_err(format!(
                "Failed to generate fill reports: {e}"
            ))),
        }
    }

    /// Generate position status reports.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - Optional instrument ID to filter by
    /// * `start` - Optional start timestamp (Unix nanoseconds)
    /// * `end` - Optional end timestamp (Unix nanoseconds)
    ///
    /// # Returns
    ///
    /// Returns a list of position status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    #[pyo3(name = "generate_position_status_reports")]
    fn py_generate_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<InstrumentId>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self;
        let fut = async move {
            client
                .generate_position_status_reports_for_python(instrument_id, start, end)
                .await
        };
        let rt = get_runtime();

        match rt.block_on(fut) {
            Ok(reports) => {
                // Convert to Python list
                let py_reports: Result<Vec<_>, _> =
                    reports.into_iter().map(|r| Py::new(py, r)).collect();
                let py_list = pyo3::types::PyList::new(py, py_reports.map_err(to_pyruntime_err)?)
                    .map_err(to_pyruntime_err)?;
                Ok(py_list.as_any().to_owned())
            }
            Err(e) => Err(to_pyruntime_err(format!(
                "Failed to generate position status reports: {e}"
            ))),
        }
    }
}
