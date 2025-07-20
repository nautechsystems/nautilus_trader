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

use std::str::FromStr;

use futures_util::StreamExt;
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    enums::{OrderSide, OrderType, PositionSide},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    python::{
        data::data_to_pycapsule,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    },
    types::{Price, Quantity},
};
use pyo3::{IntoPyObjectExt, exceptions::PyRuntimeError, prelude::*};
use pyo3_async_runtimes::tokio::get_runtime;

use crate::{
    common::enums::{OKXInstrumentType, OKXTradeMode},
    websocket::{
        OKXWebSocketClient,
        messages::{ExecutionReport, NautilusWsMessage, OKXWebSocketError},
    },
};

#[pyo3::pymethods]
impl OKXWebSocketError {
    #[getter]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[getter]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[getter]
    pub fn conn_id(&self) -> Option<&str> {
        self.conn_id.as_deref()
    }

    #[getter]
    pub fn ts_event(&self) -> u64 {
        self.timestamp
    }

    fn __repr__(&self) -> String {
        format!(
            "OKXWebSocketError(code='{}', message='{}', conn_id={:?}, ts_event={})",
            self.code, self.message, self.conn_id, self.timestamp
        )
    }
}

#[pymethods]
impl OKXWebSocketClient {
    #[new]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, api_passphrase=None, account_id=None, heartbeat=None))]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        Self::new(
            url,
            api_key,
            api_secret,
            api_passphrase,
            account_id,
            heartbeat,
        )
        .map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "with_credentials")]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, api_passphrase=None, account_id=None, heartbeat=None))]
    fn py_with_credentials(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        Self::with_credentials(
            url,
            api_key,
            api_secret,
            api_passphrase,
            account_id,
            heartbeat,
        )
        .map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_env")]
    fn py_from_env() -> PyResult<Self> {
        Self::from_env().map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        self.url()
    }

    #[getter]
    #[pyo3(name = "api_key")]
    #[must_use]
    pub fn py_api_key(&self) -> Option<&str> {
        self.api_key()
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&mut self) -> bool {
        self.is_active()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&mut self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        instruments: Vec<PyObject>,
        callback: PyObject,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut instruments_any = Vec::new();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }

        get_runtime().block_on(async {
            self.connect(instruments_any)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))
        })?;

        let stream = self.stream();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            tokio::pin!(stream);

            while let Some(msg) = stream.next().await {
                match msg {
                    NautilusWsMessage::Instrument(msg) => {
                        call_python_with_data(&callback, |py| instrument_any_to_pyobject(py, *msg));
                    }
                    NautilusWsMessage::Data(msg) => Python::with_gil(|py| {
                        for data in msg {
                            let py_obj = data_to_pycapsule(py, data);
                            call_python(py, &callback, py_obj);
                        }
                    }),
                    NautilusWsMessage::OrderRejected(msg) => {
                        call_python_with_data(&callback, |py| msg.into_py_any(py))
                    }
                    NautilusWsMessage::OrderCancelRejected(msg) => {
                        call_python_with_data(&callback, |py| msg.into_py_any(py))
                    }
                    NautilusWsMessage::OrderModifyRejected(msg) => {
                        call_python_with_data(&callback, |py| msg.into_py_any(py))
                    }
                    NautilusWsMessage::ExecutionReports(msg) => {
                        for report in msg {
                            match report {
                                ExecutionReport::Order(report) => {
                                    call_python_with_data(&callback, |py| report.into_py_any(py))
                                }
                                ExecutionReport::Fill(report) => {
                                    call_python_with_data(&callback, |py| report.into_py_any(py))
                                }
                            };
                        }
                    }
                    NautilusWsMessage::Deltas(msg) => Python::with_gil(|py| {
                        let py_obj =
                            data_to_pycapsule(py, Data::Deltas(OrderBookDeltas_API::new(msg)));
                        call_python(py, &callback, py_obj);
                    }),
                    NautilusWsMessage::AccountUpdate(msg) => {
                        call_python_with_data(&callback, |py| msg.py_to_dict(py));
                    }
                    NautilusWsMessage::Error(msg) => {
                        call_python_with_data(&callback, |py| msg.into_py_any(py));
                    }
                    NautilusWsMessage::Raw(msg) => {
                        tracing::debug!("Received raw message, skipping: {msg}");
                    }
                }
            }

            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) -> PyResult<()> {
        get_runtime().block_on(async {
            if let Err(e) = self.close().await {
                log::error!("Error on close: {e}");
            }
        });

        Ok(())
    }

    #[pyo3(name = "subscribe_instruments")]
    fn py_subscribe_instruments<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instruments(instrument_type).await {
                log::error!("Failed to subscribe to instruments '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_instrument")]
    fn py_subscribe_instrument<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instrument(instrument_id).await {
                log::error!("Failed to subscribe to instrument {instrument_id}: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_order_book")]
    fn py_subscribe_order_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_order_book(instrument_id).await {
                log::error!("Failed to subscribe to order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_quotes(instrument_id).await {
                log::error!("Failed to subscribe to quotes: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        aggregated: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_trades(instrument_id, aggregated).await {
                log::error!("Failed to subscribe to trades: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_bars(bar_type).await {
                log::error!("Failed to subscribe to bars: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_order_book")]
    fn py_unsubscribe_order_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_order_book(instrument_id).await {
                log::error!("Failed to unsubscribe from order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_quotes(instrument_id).await {
                log::error!("Failed to unsubscribe from quotes: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        aggregated: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_trades(instrument_id, aggregated).await {
                log::error!("Failed to unsubscribe from trades: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_bars(bar_type).await {
                log::error!("Failed to unsubscribe from bars: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_ticker")]
    fn py_subscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_ticker(instrument_id).await {
                log::error!("Failed to subscribe to ticker: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_ticker")]
    fn py_unsubscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_ticker(instrument_id).await {
                log::error!("Failed to unsubscribe from ticker: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_mark_prices")]
    fn py_subscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_mark_prices(instrument_id).await {
                log::error!("Failed to subscribe to mark prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_mark_prices")]
    fn py_unsubscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_mark_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from mark prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_index_prices")]
    fn py_subscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_index_prices(instrument_id).await {
                log::error!("Failed to subscribe to index prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_index_prices")]
    fn py_unsubscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_index_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from index prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_orders")]
    fn py_subscribe_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_orders(instrument_type).await {
                log::error!("Failed to subscribe to orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orders")]
    fn py_unsubscribe_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_orders(instrument_type).await {
                log::error!("Failed to unsubscribe from orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_fills")]
    fn py_subscribe_fills<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_fills(instrument_type).await {
                log::error!("Failed to subscribe to fills '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_fills")]
    fn py_unsubscribe_fills<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_fills(instrument_type).await {
                log::error!("Failed to unsubscribe from fills '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_account")]
    fn py_subscribe_account<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_account().await {
                log::error!("Failed to subscribe to account: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_account")]
    fn py_unsubscribe_account<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_account().await {
                log::error!("Failed to unsubscribe from account: {e}");
            }
            Ok(())
        })
    }

    /// Submits a new order via WebSocket.
    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        td_mode,
        client_order_id,
        order_side,
        order_type,
        quantity,
        price=None,
        trigger_price=None,
        post_only=None,
        reduce_only=None,
        position_side=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        td_mode: OKXTradeMode,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        position_side: Option<PositionSide>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    td_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    price,
                    trigger_price,
                    post_only,
                    reduce_only,
                    position_side,
                )
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Cancels an existing order via WebSocket.
    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id=None,
        position_side=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        position_side: Option<PositionSide>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    position_side,
                )
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Modify an existing order via WebSocket.
    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        new_client_order_id,
        price=None,
        quantity=None,
        venue_order_id=None,
        position_side=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        new_client_order_id: ClientOrderId,
        price: Option<Price>,
        quantity: Option<Quantity>,
        venue_order_id: Option<VenueOrderId>,
        position_side: Option<PositionSide>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .modify_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    new_client_order_id,
                    price,
                    quantity,
                    venue_order_id,
                    position_side,
                )
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Submits multiple orders via WebSocket.
    #[allow(clippy::type_complexity)]
    #[pyo3(name = "batch_submit_orders")]
    fn py_batch_submit_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<PyObject>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut domain_orders = Vec::with_capacity(orders.len());

        for obj in orders {
            let (
                instrument_type,
                instrument_id,
                td_mode,
                client_order_id,
                order_side,
                order_type,
                quantity,
                position_side,
                price,
                trigger_price,
                post_only,
                reduce_only,
            ): (
                String,
                InstrumentId,
                String,
                ClientOrderId,
                OrderSide,
                OrderType,
                Quantity,
                Option<PositionSide>,
                Option<Price>,
                Option<Price>,
                Option<bool>,
                Option<bool>,
            ) = obj
                .extract(py)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            let inst_type =
                OKXInstrumentType::from_str(&instrument_type).map_err(to_pyvalue_err)?;
            let trade_mode = OKXTradeMode::from_str(&td_mode).map_err(to_pyvalue_err)?;

            domain_orders.push((
                inst_type,
                instrument_id,
                trade_mode,
                client_order_id,
                order_side,
                position_side,
                order_type,
                quantity,
                price,
                trigger_price,
                post_only,
                reduce_only,
            ));
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_submit_orders(domain_orders)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Cancels multiple orders via WebSocket.
    #[pyo3(name = "batch_cancel_orders")]
    fn py_batch_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<PyObject>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut domain_orders = Vec::with_capacity(orders.len());

        for obj in orders {
            let (instrument_type, instrument_id, client_order_id, order_id, position_side): (
                String,
                InstrumentId,
                Option<ClientOrderId>,
                Option<String>,
                Option<PositionSide>,
            ) = obj
                .extract(py)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            let inst_type =
                OKXInstrumentType::from_str(&instrument_type).map_err(to_pyvalue_err)?;
            domain_orders.push((
                inst_type,
                instrument_id,
                client_order_id,
                order_id,
                position_side,
            ));
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_cancel_orders(domain_orders)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Modifies multiple orders via WebSocket.
    #[pyo3(name = "batch_modify_orders")]
    fn py_batch_modify_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<PyObject>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut domain_orders = Vec::with_capacity(orders.len());

        for obj in orders {
            let (
                instrument_type,
                instrument_id,
                client_order_id,
                new_client_order_id,
                price,
                quantity,
                position_side,
            ): (
                String,
                InstrumentId,
                ClientOrderId,
                ClientOrderId,
                Option<Price>,
                Option<Quantity>,
                Option<PositionSide>,
            ) = obj
                .extract(py)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            let inst_type =
                OKXInstrumentType::from_str(&instrument_type).map_err(to_pyvalue_err)?;
            domain_orders.push((
                inst_type,
                instrument_id,
                client_order_id,
                new_client_order_id,
                price,
                quantity,
                position_side,
            ));
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_modify_orders(domain_orders)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }
}

pub fn call_python(py: Python, callback: &PyObject, py_obj: PyObject) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        tracing::error!("Error calling Python: {e}");
    }
}

fn call_python_with_data<F>(callback: &PyObject, data_converter: F)
where
    F: FnOnce(Python) -> PyResult<PyObject>,
{
    Python::with_gil(|py| match data_converter(py) {
        Ok(py_obj) => call_python(py, callback, py_obj),
        Err(e) => tracing::error!("Failed to convert data to Python object: {e}"),
    });
}
