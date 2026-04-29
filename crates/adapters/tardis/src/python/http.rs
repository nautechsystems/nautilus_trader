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

use nautilus_core::{
    UnixNanos,
    python::{IntoPyObjectNautilusExt, enums::parse_enum, to_pyruntime_err},
};
use nautilus_model::python::instruments::instrument_any_to_pyobject;
use pyo3::prelude::*;

use crate::{
    common::enums::TardisExchange,
    http::{TardisHttpClient, query::InstrumentFilterBuilder},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl TardisHttpClient {
    /// A Tardis HTTP API client.
    /// See <https://docs.tardis.dev/api/http>.
    #[new]
    #[pyo3(signature = (api_key=None, base_url=None, timeout_secs=None, normalize_symbols=true, proxy_url=None))]
    fn py_new(
        api_key: Option<&str>,
        base_url: Option<&str>,
        timeout_secs: Option<u64>,
        normalize_symbols: bool,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            api_key,
            base_url,
            timeout_secs,
            normalize_symbols,
            proxy_url,
        )
        .map_err(to_pyruntime_err)
    }

    #[getter]
    #[pyo3(name = "api_key")]
    fn py_api_key(&self) -> Option<&str> {
        self.credential().map(|c| c.api_key())
    }

    #[getter]
    #[pyo3(name = "api_key_masked")]
    fn py_api_key_masked(&self) -> Option<String> {
        self.credential().map(|c| c.api_key_masked())
    }

    /// Returns all Nautilus instrument definitions for the given `exchange`, and filter params.
    #[expect(clippy::too_many_arguments)]
    #[pyo3(name = "instruments")]
    #[pyo3(signature = (exchange, symbol=None, base_currency=None, quote_currency=None, instrument_type=None, contract_type=None, active=None, start=None, end=None, available_offset=None, effective=None, ts_init=None))]
    fn py_instruments<'py>(
        &self,
        exchange: &str,
        symbol: Option<String>,
        base_currency: Option<Vec<String>>,
        quote_currency: Option<Vec<String>>,
        instrument_type: Option<Vec<String>>,
        contract_type: Option<Vec<String>>,
        active: Option<bool>,
        start: Option<u64>,
        end: Option<u64>,
        available_offset: Option<u64>,
        effective: Option<u64>,
        ts_init: Option<u64>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let exchange: TardisExchange = parse_enum(exchange, stringify!(exchange))?;

        let filter = InstrumentFilterBuilder::default()
            .base_currency(base_currency)
            .quote_currency(quote_currency)
            .instrument_type(instrument_type)
            .contract_type(contract_type)
            .active(active)
            // NOTE: The Tardis instruments metadata API does not function correctly when using
            // the `availableSince` and `availableTo` params.
            // .available_since(start.map(|x| DateTime::from_timestamp_nanos(x as i64)))
            // .available_to(end.map(|x| DateTime::from_timestamp_nanos(x as i64)))
            .build()
            .unwrap(); // All fields are Option, so build cannot fail

        let self_clone = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = self_clone
                .instruments(
                    exchange,
                    symbol.as_deref(),
                    Some(&filter),
                    start.map(UnixNanos::from),
                    end.map(UnixNanos::from),
                    available_offset.map(UnixNanos::from),
                    effective.map(UnixNanos::from),
                    ts_init.map(UnixNanos::from),
                )
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| {
                let mut py_instruments = Vec::new();
                for inst in instruments {
                    py_instruments.push(instrument_any_to_pyobject(py, inst)?);
                }
                Ok(py_instruments.into_py_any_unwrap(py))
            })
        })
    }
}
