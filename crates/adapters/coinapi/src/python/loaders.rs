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

//! Python access to standalone CoinAPI ingestion.

use std::{collections::HashMap, num::NonZeroUsize, path::PathBuf, sync::Arc};

use jiff::civil::Date;
use nautilus_core::{
    UnixNanos,
    python::{to_pyruntime_err, to_pyvalue_err},
    string::secret::SecretString,
};
use nautilus_model::{
    data::{Bar, BarType, OrderBookDelta, QuoteTick, TradeTick},
    identifiers::InstrumentId,
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
};
use nautilus_network::retry::RetryConfig;
use pyo3::{IntoPyObjectExt, prelude::*};
use ustr::Ustr;

use super::csv::{
    CoinApiDeltaStreamIterator, CoinApiQuoteStreamIterator, CoinApiTradeStreamIterator,
};
use crate::{
    config::CoinApiInstrumentSelection, http::client::CoinApiHttpConfig, loaders::CoinApiDataLoader,
};

/// Standalone ingestion with reusable instrument definitions and lazy credentials.
///
/// Register or request definitions before loading market data. Construction sends no requests.
/// History bounds are integer UNIX nanoseconds; limits must be positive.
/// Local decoding failures raise `ValueError`; downloads and REST failures raise `RuntimeError`.
/// Requested chunk sizes must be in `[1, 1_000_000]`.
#[derive(Debug)]
#[pyclass(
    name = "CoinApiDataLoader",
    module = "nautilus_trader.adapters.coinapi",
    frozen
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.coinapi")]
pub struct PyCoinApiDataLoader {
    inner: Arc<CoinApiDataLoader>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyCoinApiDataLoader {
    /// Creates a loader with optional credentials and HTTP controls.
    ///
    /// Missing credentials fall back to `COINAPI_API_KEY` on the first network request.
    /// Local files require no key. Construction sends no requests.
    #[new]
    #[pyo3(signature = (api_key=None, base_url=None, proxy_url=None, timeout_secs=None, max_concurrent_requests=None, credit_budget=None, max_retries=None, retry_delay_initial_ms=None, retry_delay_max_ms=None, retry_max_elapsed_ms=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        base_url: Option<String>,
        proxy_url: Option<String>,
        timeout_secs: Option<u64>,
        max_concurrent_requests: Option<usize>,
        credit_budget: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        retry_max_elapsed_ms: Option<u64>,
    ) -> Self {
        let defaults = CoinApiHttpConfig::default();

        let http_config = CoinApiHttpConfig {
            base_url: base_url.unwrap_or(defaults.base_url),
            proxy_url: proxy_url.map(SecretString::from),
            max_concurrent_requests: max_concurrent_requests
                .unwrap_or(defaults.max_concurrent_requests),
            credit_budget: credit_budget.unwrap_or(defaults.credit_budget),
            retry: RetryConfig {
                operation_timeout_ms: timeout_secs
                    .map(|value| value.saturating_mul(1_000))
                    .or(defaults.retry.operation_timeout_ms),
                max_retries: max_retries.unwrap_or(defaults.retry.max_retries),
                initial_delay_ms: retry_delay_initial_ms.unwrap_or(defaults.retry.initial_delay_ms),
                max_delay_ms: retry_delay_max_ms.unwrap_or(defaults.retry.max_delay_ms),
                max_elapsed_ms: retry_max_elapsed_ms.or(defaults.retry.max_elapsed_ms),
                ..defaults.retry
            },
        };

        let loader =
            CoinApiDataLoader::with_http_config(api_key.map(SecretString::from), http_config);

        Self {
            inner: Arc::new(loader),
        }
    }

    /// Registers an instrument and optional REST and numeric file-symbol mappings.
    #[pyo3(signature = (instrument, symbol_id=None, symbol_id_int=None))]
    fn add_instrument(
        &self,
        py: Python<'_>,
        instrument: Py<PyAny>,
        symbol_id: Option<&str>,
        symbol_id_int: Option<u64>,
    ) -> PyResult<()> {
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        self.inner
            .add_instrument(instrument, symbol_id.map(Ustr::from), symbol_id_int)
            .map_err(to_pyvalue_err)
    }

    /// Returns a registered instrument definition.
    fn instrument(&self, py: Python<'_>, instrument_id: InstrumentId) -> PyResult<Py<PyAny>> {
        let instrument = self
            .inner
            .instrument(instrument_id)
            .map_err(to_pyvalue_err)?;
        instrument_any_to_pyobject(py, instrument)
    }

    /// Retrieves the supplied selections and retains their definitions and mappings.
    #[pyo3(signature = (instruments, price_precisions=None))]
    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[list[typing.Any]]", imports = ("typing",)))]
    fn request_instruments<'py>(
        &self,
        py: Python<'py>,
        instruments: Vec<CoinApiInstrumentSelection>,
        price_precisions: Option<HashMap<InstrumentId, u8>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let loader = Arc::clone(&self.inner);
        let price_precisions = price_precisions.unwrap_or_default();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = loader
                .request_instruments(&instruments, &price_precisions)
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| {
                instruments
                    .into_iter()
                    .map(|instrument| instrument_any_to_pyobject(py, instrument))
                    .collect::<PyResult<Vec<_>>>()
            })
        })
    }

    /// Downloads a Flat Files object without requiring instrument definitions.
    ///
    /// Creates parent directories and replaces the destination only after a complete download.
    /// Uses the configured credentials, proxy, retry, timeout, and concurrency controls.
    /// `timeout_secs` bounds each attempt, including admission and the complete body; defaults to 30.
    /// `retry_max_elapsed_ms` bounds the whole call; defaults to 120_000. Raise both for large files.
    /// Each retry restarts the download and can incur additional billed transfer.
    /// Flat Files usage does not consume the local REST credit allowance. Failures raise `RuntimeError`.
    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[None]", imports = ("typing",)))]
    fn download_flat_file<'py>(
        &self,
        py: Python<'py>,
        bucket: String,
        object_key: String,
        filepath: PathBuf,
    ) -> PyResult<Bound<'py, PyAny>> {
        let loader = Arc::clone(&self.inner);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            loader
                .download_flat_file(&bucket, &object_key, &filepath)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Loads local book deltas using the registered instrument precision.
    ///
    /// Supply `date` or use a path with a `D-` partition; otherwise loading raises `ValueError`.
    #[pyo3(signature = (filepath, instrument_id, date=None, limit=None))]
    fn load_book_deltas(
        &self,
        filepath: PathBuf,
        instrument_id: InstrumentId,
        date: Option<Date>,
        limit: Option<usize>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        self.inner
            .load_book_deltas(filepath, instrument_id, date, limit)
            .map_err(to_pyvalue_err)
    }

    /// Streams local book deltas using the registered instrument precision.
    ///
    /// Chunks end at event boundaries and can exceed `chunk_size`.
    ///
    /// Supply `date` or use a path with a `D-` partition; otherwise loading raises `ValueError`.
    #[pyo3(signature = (filepath, instrument_id, date=None, chunk_size=100_000, limit=None))]
    fn stream_book_deltas(
        &self,
        filepath: PathBuf,
        instrument_id: InstrumentId,
        date: Option<Date>,
        chunk_size: usize,
        limit: Option<usize>,
    ) -> PyResult<CoinApiDeltaStreamIterator> {
        let stream = self
            .inner
            .stream_book_deltas(filepath, instrument_id, date, chunk_size, limit)
            .map_err(to_pyvalue_err)?;
        Ok(CoinApiDeltaStreamIterator {
            stream: Box::new(stream),
        })
    }

    /// Loads local quotes using the registered instrument precision.
    #[pyo3(signature = (filepath, instrument_id, limit=None))]
    fn load_quotes(
        &self,
        filepath: PathBuf,
        instrument_id: InstrumentId,
        limit: Option<usize>,
    ) -> PyResult<Vec<QuoteTick>> {
        self.inner
            .load_quotes(filepath, instrument_id, limit)
            .map_err(to_pyvalue_err)
    }

    /// Streams local quotes using the registered instrument precision.
    #[pyo3(signature = (filepath, instrument_id, chunk_size=100_000, limit=None))]
    fn stream_quotes(
        &self,
        filepath: PathBuf,
        instrument_id: InstrumentId,
        chunk_size: usize,
        limit: Option<usize>,
    ) -> PyResult<CoinApiQuoteStreamIterator> {
        let stream = self
            .inner
            .stream_quotes(filepath, instrument_id, chunk_size, limit)
            .map_err(to_pyvalue_err)?;
        Ok(CoinApiQuoteStreamIterator {
            stream: Box::new(stream),
        })
    }

    /// Loads local trades using the registered instrument precision.
    #[pyo3(signature = (filepath, instrument_id, limit=None))]
    fn load_trades(
        &self,
        filepath: PathBuf,
        instrument_id: InstrumentId,
        limit: Option<usize>,
    ) -> PyResult<Vec<TradeTick>> {
        self.inner
            .load_trades(filepath, instrument_id, limit)
            .map_err(to_pyvalue_err)
    }

    /// Streams local trades using the registered instrument precision.
    #[pyo3(signature = (filepath, instrument_id, chunk_size=100_000, limit=None))]
    fn stream_trades(
        &self,
        filepath: PathBuf,
        instrument_id: InstrumentId,
        chunk_size: usize,
        limit: Option<usize>,
    ) -> PyResult<CoinApiTradeStreamIterator> {
        let stream = self
            .inner
            .stream_trades(filepath, instrument_id, chunk_size, limit)
            .map_err(to_pyvalue_err)?;
        Ok(CoinApiTradeStreamIterator {
            stream: Box::new(stream),
        })
    }

    /// Loads consolidated OHLCV bars for registered numeric symbol IDs.
    ///
    /// Reads the period from `period_id` or the `TP-` file name; unmapped symbols are skipped.
    #[pyo3(signature = (filepath, period_id=None, limit=None))]
    fn load_bars(
        &self,
        filepath: PathBuf,
        period_id: Option<&str>,
        limit: Option<usize>,
    ) -> PyResult<Vec<Bar>> {
        self.inner
            .load_bars(filepath, period_id, limit)
            .map_err(to_pyvalue_err)
    }

    /// Retrieves a current L2 book using a registered REST symbol mapping.
    #[pyo3(signature = (instrument_id, depth=None))]
    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[nautilus_trader.model.OrderBook]", imports = ("typing", "nautilus_trader.model")))]
    fn request_book_snapshot<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let loader = Arc::clone(&self.inner);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let book = loader
                .request_book_snapshot(instrument_id, depth)
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| book.into_py_any(py))
        })
    }

    /// Retrieves historical quotes using retained metadata.
    ///
    /// Bounds are UNIX nanoseconds in `[start, end)`; conversion or retrieval failures raise
    /// without partial results.
    #[pyo3(signature = (instrument_id, start, end, limit=None))]
    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[list[nautilus_trader.model.QuoteTick]]", imports = ("typing", "nautilus_trader.model")))]
    fn request_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        start: u64,
        end: u64,
        limit: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let limit = limit
            .map(NonZeroUsize::try_from)
            .transpose()
            .map_err(to_pyvalue_err)?;
        let loader = Arc::clone(&self.inner);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let records = loader
                .request_quotes(
                    instrument_id,
                    UnixNanos::from(start),
                    UnixNanos::from(end),
                    limit,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| records.into_py_any(py))
        })
    }

    /// Retrieves historical trades using retained metadata.
    ///
    /// Bounds are UNIX nanoseconds in `[start, end)`; conversion or retrieval failures raise
    /// without partial results.
    #[pyo3(signature = (instrument_id, start, end, limit=None))]
    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[list[nautilus_trader.model.TradeTick]]", imports = ("typing", "nautilus_trader.model")))]
    fn request_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        start: u64,
        end: u64,
        limit: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let limit = limit
            .map(NonZeroUsize::try_from)
            .transpose()
            .map_err(to_pyvalue_err)?;
        let loader = Arc::clone(&self.inner);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let records = loader
                .request_trades(
                    instrument_id,
                    UnixNanos::from(start),
                    UnixNanos::from(end),
                    limit,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| records.into_py_any(py))
        })
    }

    /// Retrieves historical bars using retained metadata.
    ///
    /// Bounds are UNIX nanoseconds in `[start, end)`; conversion or retrieval failures raise
    /// without partial results.
    #[pyo3(signature = (bar_type, start, end, limit=None))]
    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[list[nautilus_trader.model.Bar]]", imports = ("typing", "nautilus_trader.model")))]
    fn request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
        start: u64,
        end: u64,
        limit: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let limit = limit
            .map(NonZeroUsize::try_from)
            .transpose()
            .map_err(to_pyvalue_err)?;
        let loader = Arc::clone(&self.inner);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let records = loader
                .request_bars(
                    bar_type,
                    UnixNanos::from(start),
                    UnixNanos::from(end),
                    limit,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| records.into_py_any(py))
        })
    }
}
