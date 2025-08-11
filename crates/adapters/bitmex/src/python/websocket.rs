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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{data::bar::BarType, identifiers::InstrumentId};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use pyo3_async_runtimes::tokio::get_runtime;

use crate::websocket::BitmexWebSocketClient;

#[pymethods]
impl BitmexWebSocketClient {
    #[new]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, heartbeat=None))]
    fn py_new(
        url: Option<&str>,
        api_key: Option<&str>,
        api_secret: Option<&str>,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        Self::new(url, api_key, api_secret, heartbeat).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        _callback: PyObject,
    ) -> PyResult<Bound<'py, PyAny>> {
        get_runtime().block_on(async {
            self.connect_data()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))
        })?;

        // TODO: Fix WebSocket stream lifetime issues
        // let stream = self.stream_data();
        // pyo3_async_runtimes::tokio::future_into_py(py, async move { ... })

        tracing::warn!("BitMEX WebSocket Python integration not yet implemented");
        Ok(py.None().into_bound(py))
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

// TODO: Probably move this into common
pub fn call_python(py: Python, callback: &PyObject, py_obj: PyObject) -> PyResult<()> {
    callback.call1(py, (py_obj,)).map_err(|e| {
        tracing::error!("Error calling Python: {e}");
        e
    })?;
    Ok(())
}
