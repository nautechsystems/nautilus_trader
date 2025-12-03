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

//! Python bindings for the Hyperliquid WebSocket client.

use nautilus_core::python::to_pyruntime_err;
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    identifiers::{AccountId, InstrumentId},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use pyo3::{conversion::IntoPyObjectExt, exceptions::PyRuntimeError, prelude::*};

use crate::{
    common::HyperliquidProductType,
    websocket::{
        HyperliquidWebSocketClient,
        messages::{ExecutionReport, NautilusWsMessage},
    },
};

#[pymethods]
impl HyperliquidWebSocketClient {
    #[new]
    #[pyo3(signature = (url=None, testnet=false, product_type=HyperliquidProductType::Perp, account_id=None))]
    fn py_new(
        url: Option<String>,
        testnet: bool,
        product_type: HyperliquidProductType,
        account_id: Option<String>,
    ) -> PyResult<Self> {
        let account_id = account_id.map(|s| AccountId::from(s.as_str()));
        Ok(Self::new(url, testnet, product_type, account_id))
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> String {
        self.url().to_string()
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        self.is_active()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        !self.is_active()
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &self,
        py: Python<'py>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            self.cache_instrument(inst_any);
        }

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            tokio::spawn(async move {
                loop {
                    let event = client.next_event().await;

                    match event {
                        Some(msg) => {
                            tracing::trace!("Received WebSocket message: {msg:?}");

                            match msg {
                                NautilusWsMessage::Trades(trade_ticks) => {
                                    Python::attach(|py| {
                                        for tick in trade_ticks {
                                            let py_obj = data_to_pycapsule(py, Data::Trade(tick));
                                            if let Err(e) = callback.bind(py).call1((py_obj,)) {
                                                tracing::error!(
                                                    "Error calling Python callback: {}",
                                                    e
                                                );
                                            }
                                        }
                                    });
                                }
                                NautilusWsMessage::Quote(quote_tick) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(py, Data::Quote(quote_tick));
                                        if let Err(e) = callback.bind(py).call1((py_obj,)) {
                                            tracing::error!("Error calling Python callback: {}", e);
                                        }
                                    });
                                }
                                NautilusWsMessage::Deltas(deltas) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(
                                            py,
                                            Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                        );
                                        if let Err(e) = callback.bind(py).call1((py_obj,)) {
                                            tracing::error!("Error calling Python callback: {}", e);
                                        }
                                    });
                                }
                                NautilusWsMessage::Candle(bar) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(py, Data::Bar(bar));
                                        if let Err(e) = callback.bind(py).call1((py_obj,)) {
                                            tracing::error!("Error calling Python callback: {}", e);
                                        }
                                    });
                                }
                                NautilusWsMessage::MarkPrice(mark_price) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(
                                            py,
                                            Data::MarkPriceUpdate(mark_price),
                                        );
                                        if let Err(e) = callback.bind(py).call1((py_obj,)) {
                                            tracing::error!("Error calling Python callback: {}", e);
                                        }
                                    });
                                }
                                NautilusWsMessage::IndexPrice(index_price) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(
                                            py,
                                            Data::IndexPriceUpdate(index_price),
                                        );
                                        if let Err(e) = callback.bind(py).call1((py_obj,)) {
                                            tracing::error!("Error calling Python callback: {}", e);
                                        }
                                    });
                                }
                                NautilusWsMessage::FundingRate(funding_rate) => {
                                    Python::attach(|py| {
                                        if let Ok(py_obj) = funding_rate.into_py_any(py)
                                            && let Err(e) = callback.bind(py).call1((py_obj,))
                                        {
                                            tracing::error!("Error calling Python callback: {}", e);
                                        }
                                    });
                                }
                                NautilusWsMessage::ExecutionReports(reports) => {
                                    Python::attach(|py| {
                                        for report in reports {
                                            match report {
                                                ExecutionReport::Order(order_report) => {
                                                    tracing::debug!(
                                                        "Forwarding order status report: order_id={}, status={:?}",
                                                        order_report.venue_order_id,
                                                        order_report.order_status
                                                    );
                                                    match Py::new(py, order_report) {
                                                        Ok(py_obj) => {
                                                            if let Err(e) =
                                                                callback.bind(py).call1((py_obj,))
                                                            {
                                                                tracing::error!(
                                                                    "Error calling Python callback: {}",
                                                                    e
                                                                );
                                                            }
                                                        }
                                                        Err(e) => {
                                                            tracing::error!(
                                                                "Error converting OrderStatusReport to Python: {}",
                                                                e
                                                            );
                                                        }
                                                    }
                                                }
                                                ExecutionReport::Fill(fill_report) => {
                                                    tracing::debug!(
                                                        "Forwarding fill report: trade_id={}, side={:?}, qty={}, price={}",
                                                        fill_report.trade_id,
                                                        fill_report.order_side,
                                                        fill_report.last_qty,
                                                        fill_report.last_px
                                                    );
                                                    match Py::new(py, fill_report) {
                                                        Ok(py_obj) => {
                                                            if let Err(e) =
                                                                callback.bind(py).call1((py_obj,))
                                                            {
                                                                tracing::error!(
                                                                    "Error calling Python callback: {}",
                                                                    e
                                                                );
                                                            }
                                                        }
                                                        Err(e) => {
                                                            tracing::error!(
                                                                "Error converting FillReport to Python: {}",
                                                                e
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
                                _ => {
                                    tracing::debug!("Unhandled message type: {:?}", msg);
                                }
                            }
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
                if client.is_active() {
                    return Ok(());
                }

                if start.elapsed().as_secs_f64() >= timeout_secs {
                    return Err(PyRuntimeError::new_err(format!(
                        "WebSocket connection did not become active within {timeout_secs} seconds"
                    )));
                }

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.disconnect().await {
                tracing::error!("Error on close: {e}");
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
    fn py_subscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(instrument_id)
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

    #[pyo3(name = "subscribe_book_deltas")]
    fn py_subscribe_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        _book_type: u8,
        _depth: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book_deltas")]
    fn py_unsubscribe_book_deltas<'py>(
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

    #[pyo3(name = "subscribe_book_snapshots")]
    fn py_subscribe_book_snapshots<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        _book_type: u8,
        _depth: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(instrument_id)
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

    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_bars(bar_type)
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

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_bars(bar_type)
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
            client
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
            client
                .subscribe_user_events(&user)
                .await
                .map_err(to_pyruntime_err)?;
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
            client
                .subscribe_mark_prices(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
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
            client
                .unsubscribe_mark_prices(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
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
            client
                .subscribe_index_prices(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
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
            client
                .unsubscribe_index_prices(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_funding_rates")]
    fn py_subscribe_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_funding_rates(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_funding_rates")]
    fn py_unsubscribe_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_funding_rates(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }
}
