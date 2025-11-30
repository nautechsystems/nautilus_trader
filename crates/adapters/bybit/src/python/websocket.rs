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

//! Python bindings for the Bybit WebSocket client.

use futures_util::StreamExt;
use nautilus_core::python::to_pyruntime_err;
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::{
    common::enums::{BybitEnvironment, BybitProductType},
    python::params::{BybitWsAmendOrderParams, BybitWsPlaceOrderParams},
    websocket::{
        client::BybitWebSocketClient,
        messages::{BybitWebSocketError, NautilusWsMessage},
    },
};

#[pymethods]
impl BybitWebSocketError {
    fn __repr__(&self) -> String {
        format!(
            "BybitWebSocketError(code={}, message='{}', conn_id={:?}, topic={:?})",
            self.code, self.message, self.conn_id, self.topic
        )
    }

    #[getter]
    pub fn code(&self) -> i64 {
        self.code
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
    pub fn topic(&self) -> Option<&str> {
        self.topic.as_deref()
    }

    #[getter]
    pub fn req_id(&self) -> Option<&str> {
        self.req_id.as_deref()
    }
}

#[pymethods]
impl BybitWebSocketClient {
    #[staticmethod]
    #[pyo3(name = "new_public")]
    #[pyo3(signature = (product_type, environment, url=None, heartbeat=None))]
    fn py_new_public(
        product_type: BybitProductType,
        environment: BybitEnvironment,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        Self::new_public_with(product_type, environment, url, heartbeat)
    }

    #[staticmethod]
    #[pyo3(name = "new_private")]
    #[pyo3(signature = (environment, api_key=None, api_secret=None, url=None, heartbeat=None))]
    fn py_new_private(
        environment: BybitEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        Self::new_private(environment, api_key, api_secret, url, heartbeat)
    }

    #[staticmethod]
    #[pyo3(name = "new_trade")]
    #[pyo3(signature = (environment, api_key=None, api_secret=None, url=None, heartbeat=None))]
    fn py_new_trade(
        environment: BybitEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        Self::new_trade(environment, api_key, api_secret, url, heartbeat)
    }

    #[getter]
    #[pyo3(name = "api_key_masked")]
    #[must_use]
    pub fn py_api_key_masked(&self) -> Option<String> {
        self.credential().map(|c| c.api_key_masked())
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        self.is_active()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "subscription_count")]
    fn py_subscription_count(&self) -> usize {
        self.subscription_count()
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: AccountId) {
        self.set_account_id(account_id);
    }

    #[pyo3(name = "set_mm_level")]
    fn py_set_mm_level(&self, mm_level: u8) {
        self.set_mm_level(mm_level);
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            let stream = client.stream();

            tokio::spawn(async move {
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        NautilusWsMessage::Data(data_vec) => {
                            Python::attach(|py| {
                                for data in data_vec {
                                    let py_obj = data_to_pycapsule(py, data);
                                    call_python(py, &callback, py_obj);
                                }
                            });
                        }
                        NautilusWsMessage::Deltas(deltas) => {
                            Python::attach(|py| {
                                let py_obj = data_to_pycapsule(
                                    py,
                                    Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                );
                                call_python(py, &callback, py_obj);
                            });
                        }
                        NautilusWsMessage::FundingRates(rates) => {
                            for rate in rates {
                                call_python_with_data(&callback, move |py| {
                                    rate.into_py_any(py).map(|obj| obj.into_bound(py))
                                });
                            }
                        }
                        NautilusWsMessage::OrderStatusReports(reports) => {
                            for report in reports {
                                call_python_with_data(&callback, move |py| {
                                    report.into_py_any(py).map(|obj| obj.into_bound(py))
                                });
                            }
                        }
                        NautilusWsMessage::FillReports(reports) => {
                            for report in reports {
                                call_python_with_data(&callback, move |py| {
                                    report.into_py_any(py).map(|obj| obj.into_bound(py))
                                });
                            }
                        }
                        NautilusWsMessage::PositionStatusReport(report) => {
                            call_python_with_data(&callback, move |py| {
                                report.into_py_any(py).map(|obj| obj.into_bound(py))
                            });
                        }
                        NautilusWsMessage::AccountState(state) => {
                            call_python_with_data(&callback, move |py| {
                                state.into_py_any(py).map(|obj| obj.into_bound(py))
                            });
                        }
                        NautilusWsMessage::OrderRejected(event) => {
                            call_python_with_data(&callback, move |py| {
                                event.into_py_any(py).map(|obj| obj.into_bound(py))
                            });
                        }
                        NautilusWsMessage::OrderCancelRejected(event) => {
                            call_python_with_data(&callback, move |py| {
                                event.into_py_any(py).map(|obj| obj.into_bound(py))
                            });
                        }
                        NautilusWsMessage::OrderModifyRejected(event) => {
                            call_python_with_data(&callback, move |py| {
                                event.into_py_any(py).map(|obj| obj.into_bound(py))
                            });
                        }
                        NautilusWsMessage::Error(err) => {
                            call_python_with_data(&callback, move |py| {
                                err.into_py_any(py).map(|obj| obj.into_bound(py))
                            });
                        }
                        NautilusWsMessage::Reconnected => {
                            tracing::info!("WebSocket reconnected");
                        }
                        NautilusWsMessage::Authenticated => {
                            tracing::info!("WebSocket authenticated");
                        }
                    }
                }
            });

            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                tracing::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe")]
    fn py_subscribe<'py>(
        &self,
        py: Python<'py>,
        topics: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe(topics).await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe")]
    fn py_unsubscribe<'py>(
        &self,
        py: Python<'py>,
        topics: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.unsubscribe(topics).await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_orderbook")]
    fn py_subscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_orderbook(instrument_id, depth)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orderbook")]
    fn py_unsubscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_orderbook(instrument_id, depth)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_trades(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
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
            client
                .subscribe_ticker(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
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
            client
                .unsubscribe_ticker(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_klines")]
    fn py_subscribe_klines<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_klines(instrument_id, interval)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_klines")]
    fn py_unsubscribe_klines<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_klines(instrument_id, interval)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_orders")]
    fn py_subscribe_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_orders().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orders")]
    fn py_unsubscribe_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_orders()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_executions")]
    fn py_subscribe_executions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_executions()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_executions")]
    fn py_unsubscribe_executions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_executions()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_positions")]
    fn py_subscribe_positions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_positions()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_positions")]
    fn py_unsubscribe_positions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_positions()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_wallet")]
    fn py_subscribe_wallet<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_wallet().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_wallet")]
    fn py_unsubscribe_wallet<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_wallet()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "wait_until_active")]
    fn py_wait_until_active<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .wait_until_active(timeout_secs)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (
        product_type,
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        is_quote_quantity=false,
        time_in_force=None,
        price=None,
        trigger_price=None,
        post_only=None,
        reduce_only=None,
        is_leverage=false,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        is_quote_quantity: bool,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        is_leverage: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_order(
                    product_type,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    is_quote_quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    post_only,
                    reduce_only,
                    is_leverage,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (
        product_type,
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id=None,
        quantity=None,
        price=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .modify_order(
                    product_type,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    quantity,
                    price,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (
        product_type,
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order_by_id(
                    product_type,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "build_place_order_params")]
    #[pyo3(signature = (
        product_type,
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        is_quote_quantity=false,
        time_in_force=None,
        price=None,
        trigger_price=None,
        post_only=None,
        reduce_only=None,
        is_leverage=false,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_build_place_order_params(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        is_quote_quantity: bool,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        is_leverage: bool,
    ) -> PyResult<BybitWsPlaceOrderParams> {
        let params = self
            .build_place_order_params(
                product_type,
                instrument_id,
                client_order_id,
                order_side,
                order_type,
                quantity,
                is_quote_quantity,
                time_in_force,
                price,
                trigger_price,
                post_only,
                reduce_only,
                is_leverage,
            )
            .map_err(to_pyruntime_err)?;
        Ok(params.into())
    }

    #[pyo3(name = "batch_cancel_orders")]
    #[pyo3(signature = (
        product_type,
        trader_id,
        strategy_id,
        instrument_ids,
        venue_order_ids,
        client_order_ids,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_batch_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_ids: Vec<InstrumentId>,
        venue_order_ids: Vec<Option<VenueOrderId>>,
        client_order_ids: Vec<Option<ClientOrderId>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_cancel_orders_by_id(
                    product_type,
                    trader_id,
                    strategy_id,
                    instrument_ids,
                    venue_order_ids,
                    client_order_ids,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "build_amend_order_params")]
    #[allow(clippy::too_many_arguments)]
    fn py_build_amend_order_params(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> PyResult<crate::python::params::BybitWsAmendOrderParams> {
        let params = self
            .build_amend_order_params(
                product_type,
                instrument_id,
                venue_order_id,
                client_order_id,
                quantity,
                price,
            )
            .map_err(to_pyruntime_err)?;
        Ok(params.into())
    }

    #[pyo3(name = "batch_modify_orders")]
    fn py_batch_modify_orders<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<BybitWsAmendOrderParams>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let order_params: Vec<_> = orders
                .into_iter()
                .map(|p| p.try_into())
                .collect::<Result<Vec<_>, _>>()
                .map_err(to_pyruntime_err)?;

            client
                .batch_amend_orders(trader_id, strategy_id, order_params)
                .await
                .map_err(to_pyruntime_err)?;

            Ok(())
        })
    }

    #[pyo3(name = "batch_place_orders")]
    fn py_batch_place_orders<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<BybitWsPlaceOrderParams>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let order_params: Vec<_> = orders
                .into_iter()
                .map(|p| p.try_into())
                .collect::<Result<Vec<_>, _>>()
                .map_err(to_pyruntime_err)?;

            client
                .batch_place_orders(trader_id, strategy_id, order_params)
                .await
                .map_err(to_pyruntime_err)?;

            Ok(())
        })
    }
}

fn call_python(py: Python, callback: &Py<PyAny>, py_obj: Py<PyAny>) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        tracing::error!("Error calling Python callback: {e}");
    }
}

fn call_python_with_data<F>(callback: &Py<PyAny>, data_fn: F)
where
    F: FnOnce(Python<'_>) -> PyResult<Bound<'_, PyAny>> + Send + 'static,
{
    Python::attach(|py| match data_fn(py) {
        Ok(data) => {
            if let Err(e) = callback.call1(py, (data,)) {
                tracing::error!("Error calling Python callback: {e}");
            }
        }
        Err(e) => {
            tracing::error!("Error converting data to Python: {e}");
        }
    });
}
