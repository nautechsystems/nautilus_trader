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

//! Python bindings for the Lighter WebSocket client.

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use pyo3::{IntoPyObjectExt, prelude::*};
use pyo3_async_runtimes::tokio::future_into_py;

use crate::{
    common::LighterNetwork,
    python::http::PyLighterHttpClient,
    websocket::{LighterWebSocketClient, NautilusWsMessage},
};

#[pyclass(name = "LighterWebSocketClient", module = "nautilus_pyo3.lighter")]
#[derive(Clone)]
pub struct PyLighterWebSocketClient {
    pub(crate) inner: LighterWebSocketClient,
}

#[pymethods]
impl PyLighterWebSocketClient {
    #[new]
    #[pyo3(signature = (is_testnet=false, base_url_override=None, http_client=None))]
    fn py_new(
        is_testnet: bool,
        base_url_override: Option<String>,
        http_client: Option<PyLighterHttpClient>,
    ) -> PyResult<Self> {
        let network = LighterNetwork::from(is_testnet);
        let meta = http_client.map(|c| c.inner);

        Ok(Self {
            inner: LighterWebSocketClient::new(network, base_url_override.as_deref(), meta),
        })
    }

    #[getter]
    fn py_url(&self) -> String {
        self.inner.url().to_string()
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(
        &self,
        py: Python<'_>,
        instrument: Py<PyAny>,
        market_index: Option<u32>,
    ) -> PyResult<()> {
        let inst = pyobject_to_instrument_any(py, instrument)?;
        self.inner.cache_instrument(inst, market_index);
        Ok(())
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &self,
        py: Python<'py>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            client.inner.cache_instrument(inst_any, None);
        }
        let callback_ref = callback.clone_ref(py);

        future_into_py(py, async move {
            client.inner.connect().await.map_err(to_pyvalue_err)?;

            tokio::spawn(async move {
                let cb = callback_ref;
                while let Some(event) = client.inner.next_event().await {
                    dispatch_event(&cb, event);
                }
            });

            Ok(())
        })
    }

    #[pyo3(name = "wait_until_active")]
    fn py_wait_until_active<'py>(
        &self,
        py: Python<'py>,
        timeout_ms: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            client
                .inner
                .wait_until_active(timeout_ms)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "subscribe_order_book")]
    fn py_subscribe_order_book<'py>(
        &self,
        py: Python<'py>,
        market_index: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .subscribe_order_book(market_index)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        market_index: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .subscribe_trades(market_index)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "subscribe_market_stats")]
    fn py_subscribe_market_stats<'py>(
        &self,
        py: Python<'py>,
        market_index: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .subscribe_market_stats(market_index)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "unsubscribe_order_book")]
    fn py_unsubscribe_order_book<'py>(
        &self,
        py: Python<'py>,
        market_index: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .unsubscribe_order_book(market_index)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        market_index: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .unsubscribe_trades(market_index)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "unsubscribe_market_stats")]
    fn py_unsubscribe_market_stats<'py>(
        &self,
        py: Python<'py>,
        market_index: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .unsubscribe_market_stats(market_index)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client.close().await;
            Ok(())
        })
    }
}

fn dispatch_event(callback: &Py<PyAny>, event: NautilusWsMessage) {
    match event {
        NautilusWsMessage::Deltas(deltas) => {
            Python::attach(|py| {
                let capsule = data_to_pycapsule(py, Data::Deltas(OrderBookDeltas_API::new(deltas)));
                if let Err(err) = callback.call1(py, (capsule,)) {
                    tracing::error!("Error invoking Lighter WS callback for deltas: {err}");
                }
            });
        }
        NautilusWsMessage::Quote(quote) => {
            Python::attach(|py| {
                let capsule = data_to_pycapsule(py, Data::Quote(quote));
                if let Err(err) = callback.call1(py, (capsule,)) {
                    tracing::error!("Error invoking Lighter WS callback for quote: {err}");
                }
            });
        }
        NautilusWsMessage::Trades(trades) => {
            Python::attach(|py| {
                for tick in trades {
                    let capsule = data_to_pycapsule(py, Data::Trade(tick));
                    if let Err(err) = callback.call1(py, (capsule,)) {
                        tracing::error!("Error invoking Lighter WS callback for trade: {err}");
                    }
                }
            });
        }
        NautilusWsMessage::MarkPrice(mark_price) => {
            Python::attach(|py| {
                let capsule = data_to_pycapsule(py, Data::MarkPriceUpdate(mark_price));
                if let Err(err) = callback.call1(py, (capsule,)) {
                    tracing::error!("Error invoking Lighter WS callback for mark price: {err}");
                }
            });
        }
        NautilusWsMessage::IndexPrice(index_price) => {
            Python::attach(|py| {
                let capsule = data_to_pycapsule(py, Data::IndexPriceUpdate(index_price));
                if let Err(err) = callback.call1(py, (capsule,)) {
                    tracing::error!("Error invoking Lighter WS callback for index price: {err}");
                }
            });
        }
        NautilusWsMessage::FundingRate(funding_rate) => {
            Python::attach(|py| {
                if let Err(err) = funding_rate
                    .into_py_any(py)
                    .and_then(|obj| callback.call1(py, (obj,)))
                {
                    tracing::error!("Error invoking Lighter WS callback for funding rate: {err}");
                }
            });
        }
    }
}
