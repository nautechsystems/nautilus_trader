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

use pyo3::{exceptions::PyRuntimeError, prelude::*};
use serde_json::to_string;

use crate::http::{
    client::HyperliquidHttpClient,
    parse::{HyperliquidInstrumentDef, parse_perp_instruments, parse_spot_instruments},
};

/// Python binding for HyperliquidHttpClient.
#[pyclass(name = "HyperliquidHttpClient")]
#[derive(Debug)]
pub struct PyHyperliquidHttpClient {
    pub(crate) client: HyperliquidHttpClient,
}

#[pymethods]
impl PyHyperliquidHttpClient {
    #[new]
    #[pyo3(signature = (is_testnet=false, timeout_secs=None))]
    fn py_new(is_testnet: bool, timeout_secs: Option<u64>) -> PyResult<Self> {
        Ok(Self {
            client: HyperliquidHttpClient::new(is_testnet, timeout_secs),
        })
    }

    /// Get perpetuals metadata as a JSON string.
    #[pyo3(name = "get_perp_meta")]
    fn py_get_perp_meta<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client
                .get_perp_meta()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get perp meta: {e}")))?;

            to_string(&meta)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to serialize perp meta: {e}")))
        })
    }

    /// Get spot metadata as a JSON string.
    #[pyo3(name = "get_spot_meta")]
    fn py_get_spot_meta<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client
                .get_spot_meta()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get spot meta: {e}")))?;

            to_string(&meta)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to serialize spot meta: {e}")))
        })
    }

    /// Load all available instrument definitions (perps and/or spot).
    #[pyo3(name = "load_instrument_definitions", signature = (include_perp=true, include_spot=true))]
    fn py_load_instrument_definitions<'a>(
        &self,
        py: Python<'a>,
        include_perp: bool,
        include_spot: bool,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.client.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut defs: Vec<PyHyperliquidInstrumentDef> = Vec::new();

            if include_perp {
                let meta = client.get_perp_meta().await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to get perp meta: {e}"))
                })?;

                let parsed = parse_perp_instruments(&meta).map_err(PyRuntimeError::new_err)?;

                defs.extend(parsed.into_iter().map(PyHyperliquidInstrumentDef::from));
            }

            if include_spot {
                let meta = client.get_spot_meta().await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to get spot meta: {e}"))
                })?;

                let parsed = parse_spot_instruments(&meta).map_err(PyRuntimeError::new_err)?;

                defs.extend(parsed.into_iter().map(PyHyperliquidInstrumentDef::from));
            }

            defs.sort_by(|lhs, rhs| lhs.inner.symbol.cmp(&rhs.inner.symbol));

            Ok(defs)
        })
    }
}

/// Python binding for HyperliquidInstrumentDef.
#[pyclass(name = "HyperliquidInstrumentDef")]
#[derive(Clone, Debug)]
pub struct PyHyperliquidInstrumentDef {
    pub(crate) inner: HyperliquidInstrumentDef,
}

#[pymethods]
impl PyHyperliquidInstrumentDef {
    #[getter]
    fn symbol(&self) -> &str {
        &self.inner.symbol
    }

    #[getter]
    fn base(&self) -> &str {
        &self.inner.base
    }

    #[getter]
    fn quote(&self) -> &str {
        &self.inner.quote
    }

    #[getter]
    fn market_type(&self) -> String {
        match self.inner.market_type {
            crate::http::parse::HyperliquidMarketType::Perp => "perp".to_string(),
            crate::http::parse::HyperliquidMarketType::Spot => "spot".to_string(),
        }
    }

    #[getter]
    fn price_decimals(&self) -> u32 {
        self.inner.price_decimals
    }

    #[getter]
    fn size_decimals(&self) -> u32 {
        self.inner.size_decimals
    }

    #[getter]
    fn tick_size(&self) -> String {
        self.inner.tick_size.to_string()
    }

    #[getter]
    fn lot_size(&self) -> String {
        self.inner.lot_size.to_string()
    }

    #[getter]
    fn max_leverage(&self) -> Option<u32> {
        self.inner.max_leverage
    }

    #[getter]
    fn only_isolated(&self) -> bool {
        self.inner.only_isolated
    }

    #[getter]
    fn active(&self) -> bool {
        self.inner.active
    }

    #[getter]
    fn raw_data(&self) -> &str {
        &self.inner.raw_data
    }

    fn __repr__(&self) -> String {
        format!(
            "HyperliquidInstrumentDef(symbol={}, market_type={:?}, active={})",
            self.inner.symbol, self.inner.market_type, self.inner.active
        )
    }
}

impl From<HyperliquidInstrumentDef> for PyHyperliquidInstrumentDef {
    fn from(inner: HyperliquidInstrumentDef) -> Self {
        Self { inner }
    }
}
