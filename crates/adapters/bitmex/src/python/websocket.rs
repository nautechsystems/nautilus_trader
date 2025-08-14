// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use futures_util::StreamExt;
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::bar::BarType, identifiers::InstrumentId, python::data::data_to_pycapsule,
};
use pyo3::{IntoPyObjectExt, exceptions::PyRuntimeError, prelude::*};
use pyo3_async_runtimes::tokio::get_runtime;

use crate::websocket::{BitmexWebSocketClient, messages::BitmexWsMessage};

#[pymethods]
impl BitmexWebSocketClient {
    #[new]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, heartbeat=None))]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        Self::new(url, api_key, api_secret, heartbeat).map_err(to_pyvalue_err)
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
        callback: PyObject,
    ) -> PyResult<Bound<'py, PyAny>> {
        get_runtime().block_on(async {
            self.connect()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))
        })?;

        let stream = self.stream();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            tokio::pin!(stream);

            while let Some(msg) = stream.next().await {
                Python::with_gil(|py| {
                    match msg {
                        BitmexWsMessage::Data(data_vec) => {
                            for data in data_vec {
                                let py_obj = data_to_pycapsule(py, data);
                                call_python(py, &callback, py_obj);
                            }
                        }
                        BitmexWsMessage::OrderStatusReport(report) => {
                            if let Ok(py_obj) = (*report).into_py_any(py) {
                                call_python(py, &callback, py_obj);
                            }
                        }
                        BitmexWsMessage::FillReports(reports) => {
                            for report in reports {
                                if let Ok(py_obj) = report.into_py_any(py) {
                                    call_python(py, &callback, py_obj);
                                }
                            }
                        }
                        BitmexWsMessage::PositionStatusReport(report) => {
                            if let Ok(py_obj) = (*report).into_py_any(py) {
                                call_python(py, &callback, py_obj);
                            }
                        }
                        BitmexWsMessage::WalletUpdate { .. } => {
                            // TODO: Convert to appropriate Python object
                        }
                        BitmexWsMessage::MarginUpdate { .. } => {
                            // TODO: Convert to appropriate Python object
                        }
                    }
                });
            }

            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                log::error!("Error on close: {e}");
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

    #[pyo3(name = "subscribe_order_book_25")]
    fn py_subscribe_order_book_25<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_order_book_25(instrument_id).await {
                log::error!("Failed to subscribe to order book 25: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_order_book_depth10")]
    fn py_subscribe_order_book_depth10<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_order_book_depth10(instrument_id).await {
                log::error!("Failed to subscribe to order book depth 10: {e}");
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
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_trades(instrument_id).await {
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

    #[pyo3(name = "unsubscribe_order_book_25")]
    fn py_unsubscribe_order_book_25<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_order_book_25(instrument_id).await {
                log::error!("Failed to unsubscribe from order book 25: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_order_book_depth10")]
    fn py_unsubscribe_order_book_depth10<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_order_book_depth10(instrument_id).await {
                log::error!("Failed to unsubscribe from order book depth 10: {e}");
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
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_trades(instrument_id).await {
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
}

pub fn call_python(py: Python, callback: &PyObject, py_obj: PyObject) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        tracing::error!("Error calling Python: {e}");
    }
}
