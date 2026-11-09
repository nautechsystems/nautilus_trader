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

//! Python access to standalone FinFeed ingestion.

use std::{num::NonZeroUsize, sync::Arc};

use nautilus_core::{
    UnixNanos,
    python::{to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{data::BarType, identifiers::InstrumentId, instruments::BinaryOption};
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::{config::FinFeedDataClientConfig, loaders::FinFeedDataLoader};

/// Standalone ingestion with reusable instrument definitions and lazy credentials.
///
/// Register or request definitions before requesting market data. Construction sends no requests.
/// History bounds are integer UNIX nanoseconds; limits must be positive.
#[derive(Debug)]
#[pyclass(
    name = "FinFeedDataLoader",
    module = "nautilus_trader.adapters.finfeed",
    frozen
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedDataLoader {
    inner: Arc<FinFeedDataLoader>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedDataLoader {
    /// Creates a loader, using default HTTP settings when config is omitted.
    #[new]
    #[pyo3(signature = (config=None))]
    fn py_new(config: Option<FinFeedDataClientConfig>) -> Self {
        let config = config.unwrap_or_default();
        let loader = FinFeedDataLoader::new(config);

        Self {
            inner: Arc::new(loader),
        }
    }

    /// Registers a provider-owned outcome definition.
    fn add_instrument(&self, instrument: BinaryOption) -> PyResult<()> {
        self.inner
            .add_instrument(instrument)
            .map_err(to_pyvalue_err)
    }

    /// Returns a registered instrument definition.
    fn instrument(&self, instrument_id: InstrumentId) -> PyResult<BinaryOption> {
        self.inner.instrument(instrument_id).map_err(to_pyvalue_err)
    }

    /// Retrieves and retains a fresh definition for one outcome.
    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[nautilus_trader.model.BinaryOption]", imports = ("typing", "nautilus_trader.model")))]
    fn request_instrument<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let loader = Arc::clone(&self.inner);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            loader
                .request_instrument(instrument_id)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Retrieves and retains the configured outcome selections.
    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[list[nautilus_trader.model.BinaryOption]]", imports = ("typing", "nautilus_trader.model")))]
    fn request_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let loader = Arc::clone(&self.inner);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            loader.request_instruments().await.map_err(to_pyruntime_err)
        })
    }

    /// Retrieves historical book deltas using retained metadata.
    ///
    /// Bounds are UNIX nanoseconds, inclusive on the adapter's ordering clock.
    /// Conversion or retrieval failures raise without partial results.
    #[pyo3(signature = (instrument_id, start, end, limit=None))]
    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[list[nautilus_trader.model.OrderBookDelta]]", imports = ("typing", "nautilus_trader.model")))]
    fn request_book_deltas<'py>(
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
                .request_book_deltas(
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

    /// Retrieves historical quotes using retained metadata.
    ///
    /// Bounds are UNIX nanoseconds, inclusive on the adapter's ordering clock.
    /// Conversion or retrieval failures raise without partial results.
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
    /// Bounds are UNIX nanoseconds, inclusive on the adapter's ordering clock.
    /// Conversion or retrieval failures raise without partial results.
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
    /// Bounds are UNIX nanoseconds, inclusive on the adapter's ordering clock.
    /// Conversion or retrieval failures raise without partial results.
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
