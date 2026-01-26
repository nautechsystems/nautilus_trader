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

//! Python bindings for the Binance Spot WebSocket client.

use futures_util::StreamExt;
use nautilus_common::live::get_runtime;
use nautilus_core::python::{call_python, to_pyruntime_err};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use pyo3::prelude::*;

use crate::spot::websocket::streams::{
    client::BinanceSpotWebSocketClient,
    messages::{BinanceSpotWsMessage, NautilusSpotDataWsMessage},
};

#[pymethods]
impl BinanceSpotWebSocketClient {
    #[new]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, heartbeat=None))]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        Self::new(url, api_key, api_secret, heartbeat).map_err(to_pyruntime_err)
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

            get_runtime().spawn(async move {
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        BinanceSpotWsMessage::Data(data_msg) => match data_msg {
                            NautilusSpotDataWsMessage::Data(data_vec) => {
                                Python::attach(|py| {
                                    for data in data_vec {
                                        let py_obj = data_to_pycapsule(py, data);
                                        call_python(py, &callback, py_obj);
                                    }
                                });
                            }
                            NautilusSpotDataWsMessage::Deltas(deltas) => {
                                Python::attach(|py| {
                                    let py_obj = data_to_pycapsule(
                                        py,
                                        Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                    );
                                    call_python(py, &callback, py_obj);
                                });
                            }
                            _ => {}
                        },
                        BinanceSpotWsMessage::Error(err) => {
                            log::warn!(
                                "Binance WebSocket error: code={}, msg={}",
                                err.code,
                                err.msg
                            );
                        }
                        BinanceSpotWsMessage::Reconnected => {
                            log::info!("Binance Spot WebSocket reconnected");
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
                log::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe")]
    fn py_subscribe<'py>(
        &self,
        py: Python<'py>,
        streams: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe(streams).await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe")]
    fn py_unsubscribe<'py>(
        &self,
        py: Python<'py>,
        streams: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe(streams)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }
}
