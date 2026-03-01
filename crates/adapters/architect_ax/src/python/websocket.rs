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

//! Python bindings for Ax WebSocket clients.

use futures_util::StreamExt;
use nautilus_common::live::get_runtime;
use nautilus_core::python::{call_python_threadsafe, to_pyruntime_err};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::{
    common::enums::{AxCandleWidth, AxMarketDataLevel},
    websocket::{
        data::AxMdWebSocketClient,
        messages::{AxOrdersWsMessage, NautilusDataWsMessage, NautilusExecWsMessage},
        orders::AxOrdersWebSocketClient,
    },
};

#[pymethods]
impl AxMdWebSocketClient {
    #[new]
    #[pyo3(signature = (url, auth_token, heartbeat=None))]
    fn py_new(url: String, auth_token: String, heartbeat: Option<u64>) -> Self {
        Self::new(url, auth_token, heartbeat)
    }

    #[staticmethod]
    #[pyo3(name = "without_auth")]
    #[pyo3(signature = (url, heartbeat=None))]
    fn py_without_auth(url: String, heartbeat: Option<u64>) -> Self {
        Self::without_auth(url, heartbeat)
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        self.url()
    }

    #[pyo3(name = "is_active")]
    #[must_use]
    pub fn py_is_active(&self) -> bool {
        self.is_active()
    }

    #[pyo3(name = "is_closed")]
    #[must_use]
    pub fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "subscription_count")]
    #[must_use]
    pub fn py_subscription_count(&self) -> usize {
        self.subscription_count()
    }

    #[pyo3(name = "set_auth_token")]
    fn py_set_auth_token(&mut self, token: String) {
        self.set_auth_token(token);
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            let stream = client.stream();

            get_runtime().spawn(async move {
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        NautilusDataWsMessage::Data(data_vec) => {
                            Python::attach(|py| {
                                for data in data_vec {
                                    let py_obj = data_to_pycapsule(py, data);
                                    call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                }
                            });
                        }
                        NautilusDataWsMessage::Deltas(deltas) => {
                            Python::attach(|py| {
                                let py_obj = data_to_pycapsule(
                                    py,
                                    Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                );
                                call_python_threadsafe(py, &call_soon, &callback, py_obj);
                            });
                        }
                        NautilusDataWsMessage::Bar(bar) => {
                            Python::attach(|py| {
                                let py_obj = data_to_pycapsule(py, Data::Bar(bar));
                                call_python_threadsafe(py, &call_soon, &callback, py_obj);
                            });
                        }
                        NautilusDataWsMessage::Heartbeat => {
                            // Heartbeats are handled internally, no need to forward
                        }
                        NautilusDataWsMessage::Error(err) => {
                            log::error!("AX WebSocket error: {err:?}");
                        }
                        NautilusDataWsMessage::Reconnected => {
                            log::info!("AX WebSocket reconnected");
                        }
                    }
                }
            });

            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_deltas")]
    fn py_subscribe_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        level: AxMarketDataLevel,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book_deltas(&symbol, level)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_quotes(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "unsubscribe_book_deltas")]
    fn py_unsubscribe_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_book_deltas(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = bar_type.instrument_id().symbol.to_string();
        let width = AxCandleWidth::try_from(&bar_type.spec()).map_err(to_pyruntime_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_candles(&symbol, width)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_quotes(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_trades(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = bar_type.instrument_id().symbol.to_string();
        let width = AxCandleWidth::try_from(&bar_type.spec()).map_err(to_pyruntime_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_candles(&symbol, width)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await;
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.close().await;
            Ok(())
        })
    }
}

#[pymethods]
impl AxOrdersWebSocketClient {
    #[new]
    #[pyo3(signature = (url, account_id, trader_id, heartbeat=None))]
    fn py_new(
        url: String,
        account_id: AccountId,
        trader_id: TraderId,
        heartbeat: Option<u64>,
    ) -> Self {
        Self::new(url, account_id, trader_id, heartbeat)
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        self.url()
    }

    #[getter]
    #[pyo3(name = "account_id")]
    #[must_use]
    pub fn py_account_id(&self) -> AccountId {
        self.account_id()
    }

    #[pyo3(name = "is_active")]
    #[must_use]
    pub fn py_is_active(&self) -> bool {
        self.is_active()
    }

    #[pyo3(name = "is_closed")]
    #[must_use]
    pub fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    #[pyo3(name = "register_external_order")]
    fn py_register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
    ) -> bool {
        self.register_external_order(client_order_id, venue_order_id, instrument_id, strategy_id)
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        callback: Py<PyAny>,
        bearer_token: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .connect(&bearer_token)
                .await
                .map_err(to_pyruntime_err)?;

            let stream = client.stream();

            get_runtime().spawn(async move {
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        AxOrdersWsMessage::Nautilus(exec_msg) => {
                            handle_exec_message(&call_soon, &callback, exec_msg);
                        }
                        AxOrdersWsMessage::PlaceOrderResponse(resp) => {
                            log::debug!(
                                "Place order response: rid={}, oid={}",
                                resp.rid,
                                resp.res.oid
                            );
                        }
                        AxOrdersWsMessage::CancelOrderResponse(resp) => {
                            log::debug!(
                                "Cancel order response: rid={}, received={}",
                                resp.rid,
                                resp.res.cxl_rx
                            );
                        }
                        AxOrdersWsMessage::OpenOrdersResponse(resp) => {
                            log::debug!(
                                "Open orders response: rid={}, count={}",
                                resp.rid,
                                resp.res.len()
                            );
                        }
                        AxOrdersWsMessage::Error(err) => {
                            log::error!(
                                "AX orders WebSocket error: code={:?}, message={}, rid={:?}",
                                err.code,
                                err.message,
                                err.request_id
                            );
                        }
                        AxOrdersWsMessage::Reconnected => {
                            log::info!("AX orders WebSocket reconnected");
                        }
                        AxOrdersWsMessage::Authenticated => {
                            log::info!("AX orders WebSocket authenticated");
                        }
                    }
                }
            });

            Ok(())
        })
    }

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force,
        price=None,
        trigger_price=None,
        post_only=false,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    post_only,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (client_order_id, venue_order_id=None))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order(client_order_id, venue_order_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "get_open_orders")]
    fn py_get_open_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_open_orders().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await;
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.close().await;
            Ok(())
        })
    }
}

fn handle_exec_message(call_soon: &Py<PyAny>, callback: &Py<PyAny>, msg: NautilusExecWsMessage) {
    match msg {
        NautilusExecWsMessage::OrderAccepted(event) => {
            call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
        }
        NautilusExecWsMessage::OrderFilled(event) => {
            call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
        }
        NautilusExecWsMessage::OrderCanceled(event) => {
            call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
        }
        NautilusExecWsMessage::OrderExpired(event) => {
            call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
        }
        NautilusExecWsMessage::OrderRejected(event) => {
            call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
        }
        NautilusExecWsMessage::OrderCancelRejected(event) => {
            call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
        }
        NautilusExecWsMessage::OrderStatusReports(reports) => {
            for report in reports {
                call_python_with_event(call_soon, callback, move |py| report.into_py_any(py));
            }
        }
        NautilusExecWsMessage::FillReports(reports) => {
            for report in reports {
                call_python_with_event(call_soon, callback, move |py| report.into_py_any(py));
            }
        }
    }
}

fn call_python_with_event<F>(call_soon: &Py<PyAny>, callback: &Py<PyAny>, event_fn: F)
where
    F: FnOnce(Python<'_>) -> PyResult<Py<PyAny>> + Send + 'static,
{
    Python::attach(|py| match event_fn(py) {
        Ok(py_obj) => {
            call_python_threadsafe(py, call_soon, callback, py_obj);
        }
        Err(e) => {
            log::error!("Error converting event to Python: {e}");
        }
    });
}
