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

use std::str::FromStr;

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::python::instruments::instrument_any_to_pyobject;
use pyo3::prelude::*;

use crate::tardis::{enums::Exchange, http::TardisHttpClient};

#[pymethods]
impl TardisHttpClient {
    #[new]
    #[pyo3(signature = (api_key=None, base_url=None, timeout_secs=None))]
    fn py_new(
        api_key: Option<&str>,
        base_url: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> PyResult<Self> {
        Self::new(api_key, base_url, timeout_secs).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "instrument")]
    fn py_instrument<'py>(
        &self,
        exchange: &str,
        symbol: &str,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let exchange = Exchange::from_str(exchange).map_err(to_pyvalue_err)?;
        let symbol = symbol.to_owned();
        let self_clone = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instrument = self_clone
                .instrument(exchange, &symbol)
                .await
                .map_err(to_pyruntime_err)?;

            Python::with_gil(|py| Ok(instrument_any_to_pyobject(py, instrument)?.into_py(py)))
        })
    }

    #[pyo3(name = "instruments")]
    fn py_instruments<'py>(&self, exchange: &str, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let exchange = Exchange::from_str(exchange).map_err(to_pyvalue_err)?;
        let self_clone = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = self_clone
                .instruments(exchange)
                .await
                .map_err(to_pyruntime_err)?;

            Python::with_gil(|py| {
                let mut py_instruments = Vec::new();
                for inst in instruments {
                    py_instruments.push(instrument_any_to_pyobject(py, inst)?);
                }
                Ok(py_instruments.into_py(py))
            })
        })
    }
}
