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
use std::sync::Arc;

use nautilus_core::python::to_pyruntime_err;
use nautilus_model::{
    data::BarType, identifiers::InstrumentId, python::instruments::pyobject_to_instrument_any,
};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use tokio::sync::RwLock;
use ustr::Ustr;

use crate::websocket::HyperliquidWebSocketClient;

/// Wrapper for `HyperliquidWebSocketClient` with `Arc<RwLock>` for safe Python sharing.
///
/// This wrapper follows the same pattern as other exchange adapters (BitMEX, OKX, Bybit)
/// but uses `Arc<RwLock>` because the underlying `HyperliquidWebSocketClient` contains
/// non-cloneable fields (`mpsc::Receiver`).
#[pyclass(name = "HyperliquidWebSocketClient")]
#[derive(Clone, Debug)]
pub struct PyHyperliquidWebSocketClient {
    inner: Arc<RwLock<HyperliquidWebSocketClient>>,
    url: String,
}

#[pymethods]
impl PyHyperliquidWebSocketClient {
    #[new]
    #[pyo3(signature = (url))]
    fn py_new(url: String) -> PyResult<Self> {
        let client = HyperliquidWebSocketClient::new(url.clone());
        Ok(Self {
            inner: Arc::new(RwLock::new(client)),
            url,
        })
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        &self.url
    }

    #[pyo3(name = "is_active")]
    fn py_is_active<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let inner = client.inner.read().await;
            Ok(inner.is_active())
        })
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let inner = client.inner.read().await;
            Ok(inner.is_closed())
        })
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        instruments: Vec<Py<PyAny>>,
        _callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Parse instruments from Python objects
        let mut instruments_any = Vec::new();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            {
                let mut inner = client.inner.write().await;
                inner.ensure_connected().await.map_err(to_pyruntime_err)?;
            }

            // Spawn background task to handle incoming messages
            tokio::spawn(async move {
                loop {
                    let event = {
                        let mut inner = client.inner.write().await;
                        inner.next_event().await
                    };

                    match event {
                        Some(msg) => {
                            tracing::debug!("Received WebSocket message: {:?}", msg);
                            // TODO: Convert HyperliquidWsMessage to Nautilus data types
                            // and call the callback with the data
                        }
                        None => {
                            tracing::info!("WebSocket connection closed");
                            break;
                        }
                    }
                }
            });

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
            let start = std::time::Instant::now();
            loop {
                {
                    let inner = client.inner.read().await;
                    if inner.is_active() {
                        return Ok(());
                    }
                }

                if start.elapsed().as_secs_f64() >= timeout_secs {
                    return Err(PyRuntimeError::new_err(format!(
                        "WebSocket connection did not become active within {} seconds",
                        timeout_secs
                    )));
                }

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            if let Err(e) = inner.disconnect().await {
                log::error!("Error on close: {e}");
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
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner
                .subscribe_trades(coin)
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
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner
                .unsubscribe_trades(coin)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_order_book_deltas")]
    fn py_subscribe_order_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        _book_type: u8,
        _depth: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner.subscribe_book(coin).await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_order_book_deltas")]
    fn py_unsubscribe_order_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner
                .unsubscribe_book(coin)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_order_book_snapshots")]
    fn py_subscribe_order_book_snapshots<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        _book_type: u8,
        _depth: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner.subscribe_book(coin).await.map_err(to_pyruntime_err)?;
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
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner.subscribe_bbo(coin).await.map_err(to_pyruntime_err)?;
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
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner
                .unsubscribe_bbo(coin)
                .await
                .map_err(to_pyruntime_err)?;
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
        let coin = Ustr::from(bar_type.instrument_id().symbol.as_str());
        let interval = "1m".to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner
                .subscribe_candle(coin, interval)
                .await
                .map_err(to_pyruntime_err)?;
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
        let coin = Ustr::from(bar_type.instrument_id().symbol.as_str());
        let interval = "1m".to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner
                .unsubscribe_candle(coin, interval)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_order_updates")]
    fn py_subscribe_order_updates<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner
                .subscribe_order_updates(&user)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_user_events")]
    fn py_subscribe_user_events<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut inner = client.inner.write().await;
            inner
                .subscribe_user_events(&user)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }
}
