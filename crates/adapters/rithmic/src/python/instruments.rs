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

//! Python bindings for instrument provider.

#[cfg(feature = "python")]
use pyo3::prelude::*;

#[cfg(feature = "python")]
use pyo3_async_runtimes::tokio::future_into_py;

use nautilus_core::python::to_pyruntime_err;
use std::sync::Arc;

use crate::gateway::RithmicGateway;
use crate::instruments::{RithmicInstrument, RithmicInstrumentProvider};

use super::gateway::PyRithmicGateway;

/// Python wrapper for RithmicInstrument.
#[cfg(feature = "python")]
#[pyclass(name = "RithmicInstrument", skip_from_py_object)]
#[derive(Clone)]
pub struct PyRithmicInstrument {
    inner: RithmicInstrument,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyRithmicInstrument {
    #[getter]
    fn symbol(&self) -> &str {
        &self.inner.symbol
    }

    #[getter]
    fn exchange(&self) -> &str {
        &self.inner.exchange
    }

    #[getter]
    fn product_code(&self) -> &str {
        &self.inner.product_code
    }

    #[getter]
    fn description(&self) -> &str {
        &self.inner.description
    }

    #[getter]
    fn tick_size(&self) -> f64 {
        self.inner.tick_size
    }

    #[getter]
    fn point_value(&self) -> f64 {
        self.inner.point_value
    }

    #[getter]
    fn currency(&self) -> &str {
        &self.inner.currency
    }

    #[getter]
    fn contract_size(&self) -> f64 {
        self.inner.contract_size
    }

    #[getter]
    fn price_precision(&self) -> u8 {
        self.inner.price_precision
    }

    #[getter]
    fn size_precision(&self) -> u8 {
        self.inner.size_precision
    }

    #[getter]
    fn expiration_ts(&self) -> Option<u64> {
        self.inner.expiration_ts
    }

    #[getter]
    fn is_tradeable(&self) -> bool {
        self.inner.is_tradeable
    }

    fn __repr__(&self) -> String {
        format!(
            "RithmicInstrument(symbol={}, exchange={}, tick_size={}, point_value={})",
            self.inner.symbol, self.inner.exchange, self.inner.tick_size, self.inner.point_value
        )
    }
}

impl From<RithmicInstrument> for PyRithmicInstrument {
    fn from(instrument: RithmicInstrument) -> Self {
        Self { inner: instrument }
    }
}

/// Python wrapper for RithmicInstrumentProvider.
#[cfg(feature = "python")]
#[pyclass(name = "RithmicInstrumentProvider")]
pub struct PyRithmicInstrumentProvider {
    gateway: Arc<tokio::sync::RwLock<RithmicGateway>>,
    provider: Arc<RithmicInstrumentProvider>,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyRithmicInstrumentProvider {
    #[new]
    fn new(gateway: &PyRithmicGateway) -> Self {
        let gateway = Arc::clone(&gateway.inner);
        let provider = RithmicInstrumentProvider::new(Arc::clone(&gateway));
        Self {
            gateway,
            provider: Arc::new(provider),
        }
    }

    /// Loads all instruments across known exchanges.
    fn load_all_async<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let provider = Arc::clone(&self.provider);
        let gateway = Arc::clone(&self.gateway);
        future_into_py(py, async move {
            if !gateway.read().await.is_connected() {
                return Err(to_pyruntime_err("Gateway is not connected"));
            }
            provider
                .load_all_async()
                .await
                .map_err(|e| to_pyruntime_err(e.to_string()))
        })
    }

    /// Loads instruments for a specific exchange.
    fn load_exchange_async<'py>(
        &self,
        py: Python<'py>,
        exchange: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = Arc::clone(&self.provider);
        let gateway = Arc::clone(&self.gateway);
        future_into_py(py, async move {
            if !gateway.read().await.is_connected() {
                return Err(to_pyruntime_err("Gateway is not connected"));
            }
            provider
                .load_exchange_async(&exchange)
                .await
                .map(|list| {
                    list.into_iter()
                        .map(PyRithmicInstrument::from)
                        .collect::<Vec<_>>()
                })
                .map_err(|e| to_pyruntime_err(e.to_string()))
        })
    }

    /// Loads a single instrument by symbol and exchange.
    fn load_instrument_async<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = Arc::clone(&self.provider);
        let gateway = Arc::clone(&self.gateway);
        future_into_py(py, async move {
            if !gateway.read().await.is_connected() {
                return Err(to_pyruntime_err("Gateway is not connected"));
            }
            provider
                .load_instrument_async(&symbol, &exchange)
                .await
                .map(PyRithmicInstrument::from)
                .map_err(|e| to_pyruntime_err(e.to_string()))
        })
    }

    /// Loads the current front month contract for a product root and exchange.
    fn load_front_month_async<'py>(
        &self,
        py: Python<'py>,
        product: String,
        exchange: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = Arc::clone(&self.provider);
        let gateway = Arc::clone(&self.gateway);
        future_into_py(py, async move {
            if !gateway.read().await.is_connected() {
                return Err(to_pyruntime_err("Gateway is not connected"));
            }
            provider
                .load_front_month(&product, &exchange)
                .await
                .map(PyRithmicInstrument::from)
                .map_err(|e| to_pyruntime_err(e.to_string()))
        })
    }

    /// Returns all cached instruments.
    fn instruments(&self) -> Vec<PyRithmicInstrument> {
        self.provider
            .instruments()
            .into_iter()
            .map(PyRithmicInstrument::from)
            .collect()
    }

    fn __repr__(&self) -> String {
        format!("RithmicInstrumentProvider(count={})", self.provider.count())
    }
}

/// Registers instrument provider types with the Python module.
#[cfg(feature = "python")]
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRithmicInstrument>()?;
    m.add_class::<PyRithmicInstrumentProvider>()?;
    Ok(())
}
