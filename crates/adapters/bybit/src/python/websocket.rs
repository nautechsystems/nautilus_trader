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

use std::sync::Arc;

use futures_util::StreamExt;
use nautilus_core::{nanos::UnixNanos, python::to_pyruntime_err, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    instruments::Instrument,
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::{
    common::enums::{BybitEnvironment, BybitProductType},
    websocket::{
        client::BybitWebSocketClient,
        messages::{BybitWebSocketError, BybitWebSocketMessage},
        parse::{
            parse_kline_topic, parse_orderbook_deltas, parse_ticker_linear_quote,
            parse_ticker_option_quote, parse_ws_account_state, parse_ws_fill_report,
            parse_ws_kline_bar, parse_ws_order_status_report, parse_ws_position_status_report,
            parse_ws_trade_tick,
        },
    },
};

#[pymethods]
impl BybitWebSocketError {
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

    fn __repr__(&self) -> String {
        format!(
            "BybitWebSocketError(code={}, message='{}', conn_id={:?}, topic={:?})",
            self.code, self.message, self.conn_id, self.topic
        )
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
    #[pyo3(signature = (environment, api_key, api_secret, url=None, heartbeat=None))]
    fn py_new_private(
        environment: BybitEnvironment,
        api_key: String,
        api_secret: String,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        let credential = crate::common::credential::Credential::new(api_key, api_secret);
        Self::new_private(environment, credential, url, heartbeat)
    }

    #[staticmethod]
    #[pyo3(name = "new_trade")]
    #[pyo3(signature = (environment, api_key, api_secret, url=None, heartbeat=None))]
    fn py_new_trade(
        environment: BybitEnvironment,
        api_key: String,
        api_secret: String,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        let credential = crate::common::credential::Credential::new(api_key, api_secret);
        Self::new_trade(environment, credential, url, heartbeat)
    }

    #[pyo3(name = "is_active")]
    fn py_is_active<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(client.is_active().await) })
    }

    #[pyo3(name = "subscription_count")]
    fn py_subscription_count(&self) -> usize {
        self.subscription_count()
    }

    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.add_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: nautilus_model::identifiers::AccountId) {
        self.set_account_id(account_id);
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

            let instruments = Arc::clone(client.instruments());
            let account_id = client.account_id();

            tokio::spawn(async move {
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        BybitWebSocketMessage::Orderbook(msg) => {
                            let symbol = msg.data.s;
                            if let Some(instrument_entry) = instruments
                                .iter()
                                .find(|e| e.key().symbol.as_str() == symbol.as_str())
                            {
                                let instrument = instrument_entry.value();
                                let ts_init = get_atomic_clock_realtime().get_time_ns();

                                match parse_orderbook_deltas(&msg, instrument, ts_init) {
                                    Ok(deltas) => {
                                        Python::attach(|py| {
                                            let py_obj = data_to_pycapsule(
                                                py,
                                                Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                            );
                                            call_python(py, &callback, py_obj);
                                        });
                                    }
                                    Err(e) => {
                                        tracing::error!("Error parsing orderbook deltas: {e}");
                                    }
                                }
                            } else {
                                tracing::warn!("No instrument found for symbol {symbol}");
                            }
                        }
                        BybitWebSocketMessage::Trade(msg) => {
                            for trade in &msg.data {
                                let symbol = trade.s;
                                if let Some(instrument_entry) = instruments
                                    .iter()
                                    .find(|e| e.key().symbol.as_str() == symbol.as_str())
                                {
                                    let instrument = instrument_entry.value();
                                    let ts_init = get_atomic_clock_realtime().get_time_ns();

                                    match parse_ws_trade_tick(trade, instrument, ts_init) {
                                        Ok(tick) => {
                                            Python::attach(|py| {
                                                let py_obj =
                                                    data_to_pycapsule(py, Data::Trade(tick));
                                                call_python(py, &callback, py_obj);
                                            });
                                        }
                                        Err(e) => {
                                            tracing::error!("Error parsing trade tick: {e}");
                                        }
                                    }
                                } else {
                                    tracing::warn!("No instrument found for symbol {symbol}");
                                }
                            }
                        }
                        BybitWebSocketMessage::Kline(msg) => {
                            // Extract symbol and interval from topic (e.g., "kline.5.BTCUSDT")
                            let (interval_str, symbol) = match parse_kline_topic(&msg.topic) {
                                Ok(parts) => parts,
                                Err(e) => {
                                    tracing::warn!("Failed to parse kline topic: {e}");
                                    call_python_with_json(&callback, &msg);
                                    continue;
                                }
                            };

                            if let Some(instrument_entry) = instruments
                                .iter()
                                .find(|e| e.key().symbol.as_str() == symbol)
                            {
                                let instrument = instrument_entry.value();
                                let ts_init = get_atomic_clock_realtime().get_time_ns();

                                // Parse interval to create BarType
                                use std::num::NonZero;

                                use nautilus_model::{
                                    data::{BarSpecification, BarType},
                                    enums::{AggregationSource, BarAggregation, PriceType},
                                };

                                let (step, aggregation) = match interval_str.parse::<usize>() {
                                    Ok(minutes) if minutes > 0 => (minutes, BarAggregation::Minute),
                                    _ => {
                                        // Handle other intervals (D, W, M) if needed
                                        tracing::warn!(
                                            "Unsupported kline interval: {}",
                                            interval_str
                                        );
                                        call_python_with_json(&callback, &msg);
                                        continue;
                                    }
                                };

                                if let Some(non_zero_step) = NonZero::new(step) {
                                    let bar_spec = BarSpecification {
                                        step: non_zero_step,
                                        aggregation,
                                        price_type: PriceType::Last,
                                    };
                                    let bar_type = BarType::new(
                                        instrument.id(),
                                        bar_spec,
                                        AggregationSource::External,
                                    );

                                    for kline in &msg.data {
                                        match parse_ws_kline_bar(
                                            kline, instrument, bar_type, false, ts_init,
                                        ) {
                                            Ok(bar) => {
                                                Python::attach(|py| {
                                                    let py_obj =
                                                        data_to_pycapsule(py, Data::Bar(bar));
                                                    call_python(py, &callback, py_obj);
                                                });
                                            }
                                            Err(e) => {
                                                tracing::error!("Error parsing kline to bar: {e}");
                                            }
                                        }
                                    }
                                } else {
                                    tracing::error!("Invalid step value: {}", step);
                                    call_python_with_json(&callback, &msg);
                                }
                            } else {
                                tracing::warn!("No instrument found for symbol {symbol}");
                                call_python_with_json(&callback, &msg);
                            }
                        }
                        BybitWebSocketMessage::TickerLinear(msg) => {
                            let symbol = msg.data.symbol;
                            if let Some(instrument_entry) = instruments
                                .iter()
                                .find(|e| e.key().symbol.as_str() == symbol.as_str())
                            {
                                let instrument = instrument_entry.value();
                                let ts_init = get_atomic_clock_realtime().get_time_ns();

                                match parse_ticker_linear_quote(&msg, instrument, ts_init) {
                                    Ok(quote) => {
                                        Python::attach(|py| {
                                            let py_obj = data_to_pycapsule(py, Data::Quote(quote));
                                            call_python(py, &callback, py_obj);
                                        });
                                    }
                                    Err(e) => {
                                        tracing::error!("Error parsing linear ticker quote: {e}");
                                    }
                                }
                            } else {
                                tracing::warn!("No instrument found for symbol {symbol}");
                            }
                        }
                        BybitWebSocketMessage::TickerOption(msg) => {
                            let symbol = &msg.data.symbol;
                            if let Some(instrument_entry) = instruments
                                .iter()
                                .find(|e| e.key().symbol.as_str() == symbol)
                            {
                                let instrument = instrument_entry.value();
                                let ts_init = get_atomic_clock_realtime().get_time_ns();

                                match parse_ticker_option_quote(&msg, instrument, ts_init) {
                                    Ok(quote) => {
                                        Python::attach(|py| {
                                            let py_obj = data_to_pycapsule(py, Data::Quote(quote));
                                            call_python(py, &callback, py_obj);
                                        });
                                    }
                                    Err(e) => {
                                        tracing::error!("Error parsing option ticker quote: {e}");
                                    }
                                }
                            } else {
                                tracing::warn!("No instrument found for symbol {symbol}");
                            }
                        }
                        BybitWebSocketMessage::AccountOrder(msg) => {
                            if let Some(account_id) = account_id {
                                for order in &msg.data {
                                    let symbol = &order.symbol;
                                    if let Some(instrument_entry) = instruments
                                        .iter()
                                        .find(|e| e.key().symbol.as_str() == symbol)
                                    {
                                        let instrument = instrument_entry.value();
                                        let ts_init = get_atomic_clock_realtime().get_time_ns();

                                        match parse_ws_order_status_report(
                                            order, instrument, account_id, ts_init,
                                        ) {
                                            Ok(report) => {
                                                Python::attach(|py| {
                                                    if let Ok(py_obj) = report.into_py_any(py) {
                                                        call_python(py, &callback, py_obj);
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Error parsing order status report: {e}"
                                                );
                                            }
                                        }
                                    } else {
                                        tracing::warn!("No instrument found for symbol {symbol}");
                                    }
                                }
                            } else {
                                call_python_with_json(&callback, &msg);
                            }
                        }
                        BybitWebSocketMessage::AccountExecution(msg) => {
                            if let Some(account_id) = account_id {
                                for execution in &msg.data {
                                    let symbol = &execution.symbol;
                                    if let Some(instrument_entry) = instruments
                                        .iter()
                                        .find(|e| e.key().symbol.as_str() == symbol)
                                    {
                                        let instrument = instrument_entry.value();
                                        let ts_init = get_atomic_clock_realtime().get_time_ns();

                                        match parse_ws_fill_report(
                                            execution, account_id, instrument, ts_init,
                                        ) {
                                            Ok(report) => {
                                                Python::attach(|py| {
                                                    if let Ok(py_obj) = report.into_py_any(py) {
                                                        call_python(py, &callback, py_obj);
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                tracing::error!("Error parsing fill report: {e}");
                                            }
                                        }
                                    } else {
                                        tracing::warn!("No instrument found for symbol {symbol}");
                                    }
                                }
                            } else {
                                call_python_with_json(&callback, &msg);
                            }
                        }
                        BybitWebSocketMessage::AccountWallet(msg) => {
                            if let Some(account_id) = account_id {
                                for wallet in &msg.data {
                                    let ts_event =
                                        UnixNanos::from(msg.creation_time as u64 * 1_000_000);
                                    let ts_init = get_atomic_clock_realtime().get_time_ns();

                                    match parse_ws_account_state(
                                        wallet, account_id, ts_event, ts_init,
                                    ) {
                                        Ok(state) => {
                                            Python::attach(|py| {
                                                if let Ok(py_obj) = state.into_py_any(py) {
                                                    call_python(py, &callback, py_obj);
                                                }
                                            });
                                        }
                                        Err(e) => {
                                            tracing::error!("Error parsing account state: {e}");
                                        }
                                    }
                                }
                            } else {
                                call_python_with_json(&callback, &msg);
                            }
                        }
                        BybitWebSocketMessage::AccountPosition(msg) => {
                            if let Some(account_id) = account_id {
                                for position in &msg.data {
                                    let symbol = &position.symbol;
                                    if let Some(instrument_entry) = instruments
                                        .iter()
                                        .find(|e| e.key().symbol.as_str() == symbol)
                                    {
                                        let instrument = instrument_entry.value();
                                        let ts_init = get_atomic_clock_realtime().get_time_ns();

                                        match parse_ws_position_status_report(
                                            position, account_id, instrument, ts_init,
                                        ) {
                                            Ok(report) => {
                                                Python::attach(|py| {
                                                    if let Ok(py_obj) = report.into_py_any(py) {
                                                        call_python(py, &callback, py_obj);
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Error parsing position status report: {e}"
                                                );
                                            }
                                        }
                                    } else {
                                        tracing::warn!("No instrument found for symbol {symbol}");
                                    }
                                }
                            } else {
                                call_python_with_json(&callback, &msg);
                            }
                        }
                        BybitWebSocketMessage::Error(msg) => {
                            call_python_with_data(&callback, |py| {
                                msg.into_py_any(py).map(|obj| obj.into_bound(py))
                            });
                        }
                        BybitWebSocketMessage::Reconnected => {}
                        BybitWebSocketMessage::Pong => {}
                        BybitWebSocketMessage::Response(msg) => {
                            call_python_with_json(&callback, &msg);
                        }
                        BybitWebSocketMessage::Auth(msg) => {
                            call_python_with_json(&callback, &msg);
                        }
                        BybitWebSocketMessage::Subscription(msg) => {
                            call_python_with_json(&callback, &msg);
                        }
                        BybitWebSocketMessage::Raw(value) => {
                            tracing::debug!("Received raw/unhandled message: {value}");
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
        symbol: String,
        depth: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_orderbook(symbol, depth)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orderbook")]
    fn py_unsubscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        depth: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_orderbook(symbol, depth)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades(symbol)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_trades(symbol)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_ticker")]
    fn py_subscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_ticker(symbol)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_ticker")]
    fn py_unsubscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_ticker(symbol)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_klines")]
    fn py_subscribe_klines<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        interval: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_klines(symbol, interval)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_klines")]
    fn py_unsubscribe_klines<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        interval: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_klines(symbol, interval)
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
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force=None,
        price=None,
        post_only=None,
        reduce_only=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        product_type: crate::common::enums::BybitProductType,
        instrument_id: nautilus_model::identifiers::InstrumentId,
        client_order_id: nautilus_model::identifiers::ClientOrderId,
        order_side: nautilus_model::enums::OrderSide,
        order_type: nautilus_model::enums::OrderType,
        quantity: nautilus_model::types::Quantity,
        time_in_force: Option<nautilus_model::enums::TimeInForce>,
        price: Option<nautilus_model::types::Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_order(
                    product_type,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    post_only,
                    reduce_only,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (
        product_type,
        instrument_id,
        venue_order_id=None,
        client_order_id=None,
        quantity=None,
        price=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        product_type: crate::common::enums::BybitProductType,
        instrument_id: nautilus_model::identifiers::InstrumentId,
        venue_order_id: Option<nautilus_model::identifiers::VenueOrderId>,
        client_order_id: Option<nautilus_model::identifiers::ClientOrderId>,
        quantity: Option<nautilus_model::types::Quantity>,
        price: Option<nautilus_model::types::Price>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .modify_order(
                    product_type,
                    instrument_id,
                    venue_order_id,
                    client_order_id,
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
        instrument_id,
        venue_order_id=None,
        client_order_id=None,
    ))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        product_type: crate::common::enums::BybitProductType,
        instrument_id: nautilus_model::identifiers::InstrumentId,
        venue_order_id: Option<nautilus_model::identifiers::VenueOrderId>,
        client_order_id: Option<nautilus_model::identifiers::ClientOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order_by_id(product_type, instrument_id, venue_order_id, client_order_id)
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

fn call_python_with_json<T: serde::Serialize>(callback: &Py<PyAny>, msg: &T) {
    Python::attach(|py| match serde_json::to_string(msg) {
        Ok(json_str) => {
            if let Err(e) = callback.call1(py, (json_str,)) {
                tracing::error!("Error calling Python callback: {e}");
            }
        }
        Err(e) => {
            tracing::error!("Error serializing message to JSON: {e}");
        }
    });
}
