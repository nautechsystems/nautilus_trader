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

//! Python bindings for dYdX HTTP client.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, InstrumentId},
    instruments::InstrumentAny,
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
};
use pyo3::{
    prelude::*,
    types::{PyDict, PyList},
};
use rust_decimal::Decimal;

use crate::{common::enums::DydxNetwork, http::client::DydxHttpClient};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DydxHttpClient {
    /// Provides a higher-level HTTP client for the [dYdX v4](https://dydx.exchange) Indexer REST API.
    ///
    /// This client wraps the underlying `DydxRawHttpClient` to handle conversions
    /// into the Nautilus domain model, following the two-layer pattern established
    /// in OKX, Bybit, and BitMEX adapters.
    ///
    /// **Architecture:**
    /// - **Raw client** (`DydxRawHttpClient`): Low-level HTTP methods matching dYdX Indexer API endpoints.
    /// - **Domain client** (`DydxHttpClient`): High-level methods using Nautilus domain types.
    ///
    /// The domain client:
    /// - Wraps the raw client in an `Arc` for efficient cloning (required for Python bindings).
    /// - Maintains an instrument cache using `DashMap` for thread-safe concurrent access.
    /// - Provides standard cache methods: `cache_instruments()`, `cache_instrument()`, `get_instrument()`.
    /// - Tracks cache initialization state for optimizations.
    #[new]
    #[pyo3(signature = (base_url=None, network=DydxNetwork::Mainnet, proxy_url=None))]
    fn py_new(
        base_url: Option<String>,
        network: DydxNetwork,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            base_url, 60, // timeout_secs
            proxy_url, network, None, // retry_config
        )
        .map_err(to_pyvalue_err)
    }

    /// Returns `true` if this client is configured for testnet.
    #[pyo3(name = "is_testnet")]
    fn py_is_testnet(&self) -> bool {
        self.is_testnet()
    }

    /// Returns the base URL used by this client.
    #[pyo3(name = "base_url")]
    fn py_base_url(&self) -> String {
        self.base_url().to_string()
    }

    /// Requests instruments from the dYdX Indexer API and returns Nautilus domain types.
    ///
    /// This method does NOT automatically cache results. Use `fetch_and_cache_instruments()`
    /// for automatic caching, or call `cache_instruments()` manually with the results.
    #[pyo3(name = "request_instruments")]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        maker_fee: Option<&str>,
        taker_fee: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let maker = maker_fee
            .map(Decimal::from_str)
            .transpose()
            .map_err(to_pyvalue_err)?;

        let taker = taker_fee
            .map(Decimal::from_str)
            .transpose()
            .map_err(to_pyvalue_err)?;

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments(None, maker, taker)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_instruments: PyResult<Vec<Py<PyAny>>> = instruments
                    .into_iter()
                    .map(|inst| instrument_any_to_pyobject(py, inst))
                    .collect();
                py_instruments
            })
        })
    }

    /// Fetches instruments from the API and caches them.
    ///
    /// This is a convenience method that fetches instruments and populates both
    /// the symbol-based and CLOB pair ID-based caches.
    ///
    /// On success, existing caches are cleared and repopulated atomically.
    /// On failure, existing caches are preserved (no partial updates).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    #[pyo3(name = "fetch_and_cache_instruments")]
    fn py_fetch_and_cache_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .fetch_and_cache_instruments()
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Fetches a single instrument by ticker and caches it.
    ///
    /// This is used for on-demand fetching of newly discovered instruments
    /// via WebSocket.
    ///
    /// Returns `None` if the market is not found or inactive.
    #[pyo3(name = "fetch_instrument")]
    fn py_fetch_instrument<'py>(
        &self,
        py: Python<'py>,
        ticker: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match client.fetch_and_cache_single_instrument(&ticker).await {
                Ok(Some(instrument)) => {
                    Python::attach(|py| instrument_any_to_pyobject(py, instrument))
                }
                Ok(None) => Ok(Python::attach(|py| py.None())),
                Err(e) => Err(to_pyvalue_err(e)),
            }
        })
    }

    /// Gets an instrument from the cache by InstrumentId.
    #[pyo3(name = "get_instrument")]
    fn py_get_instrument(&self, py: Python<'_>, symbol: &str) -> PyResult<Option<Py<PyAny>>> {
        use nautilus_model::identifiers::{Symbol, Venue};
        let instrument_id = InstrumentId::new(Symbol::new(symbol), Venue::new("DYDX"));
        let instrument = self.get_instrument(&instrument_id);
        match instrument {
            Some(inst) => Ok(Some(instrument_any_to_pyobject(py, inst)?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "instrument_count")]
    fn py_instrument_count(&self) -> usize {
        self.cached_instruments_count()
    }

    #[pyo3(name = "instrument_symbols")]
    fn py_instrument_symbols(&self) -> Vec<String> {
        self.all_instrument_ids()
            .into_iter()
            .map(|id| id.symbol.to_string())
            .collect()
    }

    /// Caches multiple instruments (symbol lookup only).
    ///
    /// Use `fetch_and_cache_instruments()` for full caching with market params.
    /// Any existing instruments with the same symbols will be replaced.
    #[pyo3(name = "cache_instruments")]
    fn py_cache_instruments(
        &self,
        py: Python<'_>,
        py_instruments: Vec<Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let instruments: Vec<InstrumentAny> = py_instruments
            .into_iter()
            .map(|py_inst| {
                // Convert Bound<PyAny> to Py<PyAny> using unbind()
                pyobject_to_instrument_any(py, py_inst.unbind())
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_pyvalue_err)?;

        self.cache_instruments(instruments);
        Ok(())
    }

    #[pyo3(name = "get_orders")]
    #[pyo3(signature = (address, subaccount_number, market=None, limit=None))]
    fn py_get_orders<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
        market: Option<String>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .inner
                .get_orders(&address, subaccount_number, market.as_deref(), limit)
                .await
                .map_err(to_pyvalue_err)?;
            serde_json::to_string(&response).map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "get_fills")]
    #[pyo3(signature = (address, subaccount_number, market=None, limit=None))]
    fn py_get_fills<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
        market: Option<String>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .inner
                .get_fills(&address, subaccount_number, market.as_deref(), limit)
                .await
                .map_err(to_pyvalue_err)?;
            serde_json::to_string(&response).map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "get_subaccount")]
    fn py_get_subaccount<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .inner
                .get_subaccount(&address, subaccount_number)
                .await
                .map_err(to_pyvalue_err)?;
            serde_json::to_string(&response).map_err(to_pyvalue_err)
        })
    }

    /// Requests order status reports for a subaccount.
    ///
    /// Fetches orders from the dYdX Indexer API and converts them to Nautilus
    /// `OrderStatusReport` objects.
    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (address, subaccount_number, account_id, instrument_id=None))]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_order_status_reports(
                    &address,
                    subaccount_number,
                    account_id,
                    instrument_id,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests fill reports for a subaccount.
    ///
    /// Fetches fills from the dYdX Indexer API and converts them to Nautilus
    /// `FillReport` objects.
    #[pyo3(name = "request_fill_reports")]
    #[pyo3(signature = (address, subaccount_number, account_id, instrument_id=None))]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_fill_reports(&address, subaccount_number, account_id, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests position status reports for a subaccount.
    ///
    /// Fetches positions from the dYdX Indexer API and converts them to Nautilus
    /// `PositionStatusReport` objects.
    #[pyo3(name = "request_position_status_reports")]
    #[pyo3(signature = (address, subaccount_number, account_id, instrument_id=None))]
    fn py_request_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_position_status_reports(
                    &address,
                    subaccount_number,
                    account_id,
                    instrument_id,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests account state for a subaccount.
    ///
    /// Fetches the subaccount from the dYdX Indexer API and converts it to a Nautilus
    /// `AccountState` with balances and margin calculations.
    #[pyo3(name = "request_account_state")]
    fn py_request_account_state<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
        account_id: AccountId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_state = client
                .request_account_state(&address, subaccount_number, account_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(account_state.into_py_any_unwrap(py)))
        })
    }

    /// Requests historical bars for an instrument with optional pagination.
    ///
    /// Fetches candle data from the dYdX Indexer API and converts to Nautilus
    /// `Bar` objects. Supports time-chunked pagination for large date ranges.
    ///
    /// The resolution is derived internally from `bar_type` (no need to pass
    /// `DydxCandleResolution`). Incomplete bars (where `ts_event >= now`) are
    /// filtered out.
    ///
    /// Results are returned in chronological order (oldest first).
    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None, timestamp_on_close=true))]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
        timestamp_on_close: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(bar_type, start, end, limit, timestamp_on_close)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist = PyList::new(py, bars.into_iter().map(|b| b.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests historical trade ticks for an instrument with optional pagination.
    ///
    /// Fetches trade data from the dYdX Indexer API and converts them to Nautilus
    /// `TradeTick` objects. Supports cursor-based pagination using block height
    /// and client-side time filtering (the dYdX API has no timestamp filter).
    ///
    /// Results are returned in chronological order (oldest first).
    #[pyo3(name = "request_trade_ticks")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None))]
    fn py_request_trade_ticks<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_trade_ticks(instrument_id, start, end, limit)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist = PyList::new(py, trades.into_iter().map(|t| t.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests an order book snapshot for a symbol.
    ///
    /// Fetches order book data from the dYdX Indexer API and converts it to Nautilus
    /// `OrderBookDeltas`. The snapshot is represented as a sequence of deltas starting
    /// with a CLEAR action followed by ADD actions for each level.
    #[pyo3(name = "request_orderbook_snapshot")]
    fn py_request_orderbook_snapshot<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let deltas = client
                .request_orderbook_snapshot(instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(deltas.into_py_any_unwrap(py)))
        })
    }

    #[pyo3(name = "get_time")]
    fn py_get_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.inner.get_time().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| {
                let dict = PyDict::new(py);
                dict.set_item("iso", response.iso.to_string())?;
                dict.set_item("epoch", response.epoch_ms)?;
                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    #[pyo3(name = "get_height")]
    fn py_get_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.inner.get_height().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| {
                let dict = PyDict::new(py);
                dict.set_item("height", response.height)?;
                dict.set_item("time", response.time)?;
                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    #[pyo3(name = "get_transfers")]
    #[pyo3(signature = (address, subaccount_number, limit=None))]
    fn py_get_transfers<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .inner
                .get_transfers(&address, subaccount_number, limit)
                .await
                .map_err(to_pyvalue_err)?;
            serde_json::to_string(&response).map_err(to_pyvalue_err)
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "DydxHttpClient(base_url='{}', is_testnet={}, cached_instruments={})",
            self.base_url(),
            self.is_testnet(),
            self.cached_instruments_count()
        )
    }
}
