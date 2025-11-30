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

use nautilus_core::python::to_pyruntime_err;
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    identifiers::InstrumentId,
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use pyo3::prelude::*;

use crate::{
    common::{
        enums::{KrakenEnvironment, KrakenProductType},
        urls::get_kraken_ws_public_url,
    },
    websocket::futures::client::{KrakenFuturesWebSocketClient, KrakenFuturesWsMessage},
};

#[pymethods]
impl KrakenFuturesWebSocketClient {
    #[new]
    #[pyo3(signature = (environment=None, base_url=None, heartbeat_secs=None))]
    fn py_new(
        environment: Option<KrakenEnvironment>,
        base_url: Option<String>,
        heartbeat_secs: Option<u64>,
    ) -> PyResult<Self> {
        let env = environment.unwrap_or(KrakenEnvironment::Mainnet);
        let url = base_url.unwrap_or_else(|| {
            get_kraken_ws_public_url(KrakenProductType::Futures, env).to_string()
        });

        Ok(KrakenFuturesWebSocketClient::new(url, heartbeat_secs))
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

    /// Cache instruments for price precision lookup (bulk replace).
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

    /// Cache a single instrument for price precision lookup (upsert).
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
                tokio::spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        Python::attach(|py| {
                            let py_obj = match msg {
                                KrakenFuturesWsMessage::MarkPrice(update) => {
                                    data_to_pycapsule(py, Data::from(update))
                                }
                                KrakenFuturesWsMessage::IndexPrice(update) => {
                                    data_to_pycapsule(py, Data::from(update))
                                }
                                KrakenFuturesWsMessage::Quote(quote) => {
                                    data_to_pycapsule(py, Data::from(quote))
                                }
                                KrakenFuturesWsMessage::Trade(trade) => {
                                    data_to_pycapsule(py, Data::from(trade))
                                }
                                KrakenFuturesWsMessage::BookDeltas(deltas) => data_to_pycapsule(
                                    py,
                                    Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                ),
                            };
                            if let Err(e) = callback.call1(py, (py_obj,)) {
                                tracing::error!("Error calling Python callback: {e}");
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
}
