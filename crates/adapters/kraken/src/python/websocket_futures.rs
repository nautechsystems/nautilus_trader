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

//! Python bindings for the Kraken Futures WebSocket client.

use nautilus_common::live::get_runtime;
use nautilus_core::python::to_pyruntime_err;
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::{
    common::{
        credential::KrakenCredential,
        enums::{KrakenEnvironment, KrakenProductType},
        urls::get_kraken_ws_public_url,
    },
    websocket::futures::{client::KrakenFuturesWebSocketClient, messages::KrakenFuturesWsMessage},
};

#[pymethods]
impl KrakenFuturesWebSocketClient {
    #[new]
    #[pyo3(signature = (environment=None, base_url=None, heartbeat_secs=None, api_key=None, api_secret=None))]
    fn py_new(
        environment: Option<KrakenEnvironment>,
        base_url: Option<String>,
        heartbeat_secs: Option<u64>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> PyResult<Self> {
        let env = environment.unwrap_or(KrakenEnvironment::Mainnet);
        let demo = env == KrakenEnvironment::Demo;
        let url = base_url.unwrap_or_else(|| {
            get_kraken_ws_public_url(KrakenProductType::Futures, env).to_string()
        });
        let credential = KrakenCredential::resolve_futures(api_key, api_secret, demo);

        Ok(KrakenFuturesWebSocketClient::with_credentials(
            url,
            heartbeat_secs,
            credential,
        ))
    }

    #[getter]
    #[pyo3(name = "has_credentials")]
    #[must_use]
    pub fn py_has_credentials(&self) -> bool {
        self.has_credentials()
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        self.url()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        self.is_active()
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

    #[pyo3(name = "authenticate")]
    fn py_authenticate<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.authenticate().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "cache_instruments")]
    fn py_cache_instruments(&self, py: Python<'_>, instruments: Vec<Py<PyAny>>) -> PyResult<()> {
        let mut instruments_any = Vec::new();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }
        self.cache_instruments(instruments_any);
        Ok(())
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
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut instruments_any = Vec::new();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            // Cache instruments after connection is established
            client.cache_instruments(instruments_any);

            // Take ownership of the receiver
            if let Some(mut rx) = client.take_output_rx() {
                get_runtime().spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        Python::attach(|py| match msg {
                            KrakenFuturesWsMessage::MarkPrice(update) => {
                                let py_obj = data_to_pycapsule(py, Data::from(update));
                                if let Err(e) = callback.call1(py, (py_obj,)) {
                                    tracing::error!("Error calling Python callback: {e}");
                                }
                            }
                            KrakenFuturesWsMessage::IndexPrice(update) => {
                                let py_obj = data_to_pycapsule(py, Data::from(update));
                                if let Err(e) = callback.call1(py, (py_obj,)) {
                                    tracing::error!("Error calling Python callback: {e}");
                                }
                            }
                            KrakenFuturesWsMessage::Quote(quote) => {
                                let py_obj = data_to_pycapsule(py, Data::from(quote));
                                if let Err(e) = callback.call1(py, (py_obj,)) {
                                    tracing::error!("Error calling Python callback: {e}");
                                }
                            }
                            KrakenFuturesWsMessage::Trade(trade) => {
                                let py_obj = data_to_pycapsule(py, Data::from(trade));
                                if let Err(e) = callback.call1(py, (py_obj,)) {
                                    tracing::error!("Error calling Python callback: {e}");
                                }
                            }
                            KrakenFuturesWsMessage::BookDeltas(deltas) => {
                                let py_obj = data_to_pycapsule(
                                    py,
                                    Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                );
                                if let Err(e) = callback.call1(py, (py_obj,)) {
                                    tracing::error!("Error calling Python callback: {e}");
                                }
                            }
                            KrakenFuturesWsMessage::OrderAccepted(event) => {
                                match event.into_py_any(py) {
                                    Ok(py_obj) => {
                                        if let Err(e) = callback.call1(py, (py_obj,)) {
                                            tracing::error!("Error calling Python callback: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to convert OrderAccepted to Python: {e}"
                                        );
                                    }
                                }
                            }
                            KrakenFuturesWsMessage::OrderCanceled(event) => {
                                match event.into_py_any(py) {
                                    Ok(py_obj) => {
                                        if let Err(e) = callback.call1(py, (py_obj,)) {
                                            tracing::error!("Error calling Python callback: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to convert OrderCanceled to Python: {e}"
                                        );
                                    }
                                }
                            }
                            KrakenFuturesWsMessage::OrderExpired(event) => {
                                match event.into_py_any(py) {
                                    Ok(py_obj) => {
                                        if let Err(e) = callback.call1(py, (py_obj,)) {
                                            tracing::error!("Error calling Python callback: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to convert OrderExpired to Python: {e}"
                                        );
                                    }
                                }
                            }
                            KrakenFuturesWsMessage::OrderUpdated(event) => {
                                match event.into_py_any(py) {
                                    Ok(py_obj) => {
                                        if let Err(e) = callback.call1(py, (py_obj,)) {
                                            tracing::error!("Error calling Python callback: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to convert OrderUpdated to Python: {e}"
                                        );
                                    }
                                }
                            }
                            KrakenFuturesWsMessage::OrderStatusReport(report) => {
                                match (*report).into_py_any(py) {
                                    Ok(py_obj) => {
                                        if let Err(e) = callback.call1(py, (py_obj,)) {
                                            tracing::error!("Error calling Python callback: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to convert OrderStatusReport to Python: {e}"
                                        );
                                    }
                                }
                            }
                            KrakenFuturesWsMessage::FillReport(report) => {
                                match (*report).into_py_any(py) {
                                    Ok(py_obj) => {
                                        if let Err(e) = callback.call1(py, (py_obj,)) {
                                            tracing::error!("Error calling Python callback: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to convert FillReport to Python: {e}"
                                        );
                                    }
                                }
                            }
                            KrakenFuturesWsMessage::Reconnected => {
                                tracing::info!("WebSocket reconnected");
                            }
                        });
                    }
                });
            }

            Ok(())
        })
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.close().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book")]
    #[pyo3(signature = (instrument_id, depth=None))]
    fn py_subscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(instrument_id, depth)
                .await
                .map_err(to_pyruntime_err)?;
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
            client
                .subscribe_quotes(instrument_id)
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

    #[pyo3(name = "subscribe_mark_price")]
    fn py_subscribe_mark_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_mark_price(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_index_price")]
    fn py_subscribe_index_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_index_price(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book")]
    fn py_unsubscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_book(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
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
            client
                .unsubscribe_quotes(instrument_id)
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

    #[pyo3(name = "unsubscribe_mark_price")]
    fn py_unsubscribe_mark_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_mark_price(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_index_price")]
    fn py_unsubscribe_index_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_index_price(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&self, account_id: AccountId) {
        self.set_account_id(account_id);
    }

    #[pyo3(name = "cache_client_order")]
    fn py_cache_client_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    ) {
        self.cache_client_order(
            client_order_id,
            venue_order_id,
            instrument_id,
            trader_id,
            strategy_id,
        );
    }

    #[pyo3(name = "sign_challenge")]
    fn py_sign_challenge(&self, challenge: &str) -> PyResult<String> {
        self.sign_challenge(challenge).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "authenticate_with_challenge")]
    fn py_authenticate_with_challenge<'py>(
        &self,
        py: Python<'py>,
        challenge: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .authenticate_with_challenge(&challenge)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "set_auth_credentials")]
    fn py_set_auth_credentials<'py>(
        &self,
        py: Python<'py>,
        original_challenge: String,
        signed_challenge: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .set_auth_credentials(original_challenge, signed_challenge)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_open_orders")]
    fn py_subscribe_open_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_open_orders()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_fills")]
    fn py_subscribe_fills<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_fills().await.map_err(to_pyruntime_err)?;
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
}
