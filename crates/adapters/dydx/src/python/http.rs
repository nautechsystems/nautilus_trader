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

//! Python bindings for dYdX HTTP client.

use std::str::FromStr;

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    python::instruments::instrument_any_to_pyobject,
};
use pyo3::{prelude::*, types::PyList};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::http::client::DydxHttpClient;

#[pymethods]
impl DydxHttpClient {
    /// Creates a new [`DydxHttpClient`] instance.
    #[new]
    #[pyo3(signature = (base_url=None, is_testnet=false))]
    fn py_new(base_url: Option<String>, is_testnet: bool) -> PyResult<Self> {
        // Mirror the Rust client's constructor signature with sensible defaults
        Self::new(
            base_url, None, // timeout_secs
            None, // proxy_url
            is_testnet, None, // retry_config
        )
        .map_err(to_pyvalue_err)
    }

    /// Returns `true` if the client is configured for testnet.
    #[pyo3(name = "is_testnet")]
    fn py_is_testnet(&self) -> bool {
        self.is_testnet()
    }

    /// Returns the base URL for the HTTP client.
    #[pyo3(name = "base_url")]
    fn py_base_url(&self) -> String {
        self.base_url().to_string()
    }

    /// Requests all available instruments from the dYdX Indexer API.
    #[pyo3(name = "request_instruments")]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        maker_fee: Option<String>,
        taker_fee: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let maker = maker_fee
            .as_ref()
            .map(|s| Decimal::from_str(s))
            .transpose()
            .map_err(to_pyvalue_err)?;

        let taker = taker_fee
            .as_ref()
            .map(|s| Decimal::from_str(s))
            .transpose()
            .map_err(to_pyvalue_err)?;

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments(None, maker, taker)
                .await
                .map_err(to_pyvalue_err)?;

            #[allow(deprecated)]
            Python::with_gil(|py| {
                let py_instruments: PyResult<Vec<Py<PyAny>>> = instruments
                    .into_iter()
                    .map(|inst| instrument_any_to_pyobject(py, inst))
                    .collect();
                py_instruments
            })
        })
    }

    /// Fetches all instruments from the API and caches them along with market params.
    ///
    /// This is the preferred method for initializing the HTTP client cache before
    /// submitting orders, as it caches both instruments and their associated market
    /// parameters needed for order quantization.
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

    /// Gets a cached instrument by symbol.
    #[pyo3(name = "get_instrument")]
    fn py_get_instrument(&self, py: Python<'_>, symbol: &str) -> PyResult<Option<Py<PyAny>>> {
        let symbol_ustr = Ustr::from(symbol);
        let instrument = self.get_instrument(&symbol_ustr);
        match instrument {
            Some(inst) => Ok(Some(instrument_any_to_pyobject(py, inst)?)),
            None => Ok(None),
        }
    }

    /// Returns the number of cached instruments.
    #[pyo3(name = "instrument_count")]
    fn py_instrument_count(&self) -> usize {
        self.instruments().len()
    }

    /// Returns all cached instrument symbols.
    #[pyo3(name = "instrument_symbols")]
    fn py_instrument_symbols(&self) -> Vec<String> {
        self.instruments()
            .iter()
            .map(|entry| entry.key().to_string())
            .collect()
    }

    /// Cache instruments in the HTTP client for use by order submitter.
    ///
    /// This method accepts a list of instrument Python objects returned from `request_instruments()`
    /// and caches them internally for use by the order submitter.
    #[pyo3(name = "cache_instruments")]
    fn py_cache_instruments(
        &self,
        py: Python<'_>,
        py_instruments: Vec<Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        use nautilus_model::{
            instruments::InstrumentAny, python::instruments::pyobject_to_instrument_any,
        };

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

    /// Fetches orders for a subaccount.
    ///
    /// Returns a JSON string containing the orders response.
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

    /// Fetches fills for a subaccount.
    ///
    /// Returns a JSON string containing the fills response.
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

    /// Fetches subaccount info including positions.
    ///
    /// Returns a JSON string containing the subaccount response.
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
    /// Returns Nautilus `OrderStatusReport` objects.
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
    /// Returns Nautilus `FillReport` objects.
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
    /// Returns Nautilus `PositionStatusReport` objects.
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

    /// Requests historical bars for a symbol.
    ///
    /// Fetches candle data and converts to Nautilus `Bar` objects.
    /// Results are ordered by timestamp ascending (oldest first).
    ///
    /// Parameters
    /// ----------
    /// bar_type : str
    ///     The bar type string (e.g., "ETH-USD-PERP.DYDX-1-MINUTE-LAST-EXTERNAL").
    /// resolution : str
    ///     The dYdX candle resolution (e.g., "1MIN", "5MINS", "1HOUR", "1DAY").
    /// limit : int, optional
    ///     Maximum number of bars to fetch.
    /// start : str, optional
    ///     Start time in ISO 8601 format.
    /// end : str, optional
    ///     End time in ISO 8601 format.
    ///
    /// Returns
    /// -------
    /// list[Bar]
    ///     List of Nautilus Bar objects.
    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, resolution, limit=None, start=None, end=None))]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: String,
        resolution: String,
        limit: Option<u32>,
        start: Option<String>,
        end: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        use std::str::FromStr;

        use chrono::DateTime;
        use nautilus_model::data::BarType;

        use crate::common::enums::DydxCandleResolution;

        let bar_type = BarType::from_str(&bar_type).map_err(to_pyvalue_err)?;
        let resolution = DydxCandleResolution::from_str(&resolution).map_err(to_pyvalue_err)?;

        let from_iso = start
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&chrono::Utc)))
            .transpose()
            .map_err(to_pyvalue_err)?;

        let to_iso = end
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&chrono::Utc)))
            .transpose()
            .map_err(to_pyvalue_err)?;

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(bar_type, resolution, limit, from_iso, to_iso)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist = PyList::new(py, bars.into_iter().map(|b| b.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests historical trade ticks for a symbol.
    ///
    /// Fetches trade data and converts to Nautilus `TradeTick` objects.
    /// Results are ordered by timestamp descending (newest first).
    ///
    /// Parameters
    /// ----------
    /// instrument_id : InstrumentId
    ///     The instrument ID to fetch trades for.
    /// limit : int, optional
    ///     Maximum number of trades to fetch.
    ///
    /// Returns
    /// -------
    /// list[TradeTick]
    ///     List of Nautilus TradeTick objects.
    #[pyo3(name = "request_trade_ticks")]
    #[pyo3(signature = (instrument_id, limit=None))]
    fn py_request_trade_ticks<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_trade_ticks(instrument_id, limit)
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
    /// Fetches order book data and converts to Nautilus `OrderBookDeltas`.
    /// The snapshot is represented as deltas starting with CLEAR followed by ADD actions.
    ///
    /// Parameters
    /// ----------
    /// instrument_id : InstrumentId
    ///     The instrument ID to fetch the order book for.
    ///
    /// Returns
    /// -------
    /// OrderBookDeltas
    ///     The order book snapshot as deltas.
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

    /// Get current server time from the dYdX Indexer.
    ///
    /// Returns
    /// -------
    /// dict
    ///     Dictionary containing 'iso' (ISO 8601 string) and 'epoch' (Unix timestamp float).
    #[pyo3(name = "get_time")]
    fn py_get_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.inner.get_time().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| {
                use pyo3::types::PyDict;
                let dict = PyDict::new(py);
                dict.set_item("iso", response.iso.to_string())?;
                dict.set_item("epoch", response.epoch_ms)?;
                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    /// Get current blockchain height from the dYdX Indexer.
    ///
    /// Returns
    /// -------
    /// dict
    ///     Dictionary containing 'height' (block number) and 'time' (ISO 8601 string).
    #[pyo3(name = "get_height")]
    fn py_get_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.inner.get_height().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| {
                use pyo3::types::PyDict;
                let dict = PyDict::new(py);
                dict.set_item("height", response.height)?;
                dict.set_item("time", response.time)?;
                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    /// Fetches transfer history for a subaccount.
    ///
    /// Returns a JSON string containing the transfers response.
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
            self.instruments().len()
        )
    }
}
