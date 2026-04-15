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

//! Python bindings for the Interactive Brokers instrument provider.

use nautilus_core::python::to_pyruntime_err;
use nautilus_model::{identifiers::InstrumentId, python::instruments::instrument_any_to_pyobject};
use pyo3::{prelude::*, types::PyList};

use crate::{
    providers::instruments::InteractiveBrokersInstrumentProvider,
    python::conversion::{contract_details_to_pyobject, py_to_contract},
};

#[cfg(feature = "python")]
#[pymethods]
impl InteractiveBrokersInstrumentProvider {
    #[new]
    fn py_new(config: crate::config::InteractiveBrokersInstrumentProviderConfig) -> Self {
        Self::new(config)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    /// Find an instrument by its ID.
    #[pyo3(name = "find")]
    fn py_find(&self, py: Python, instrument_id: InstrumentId) -> PyResult<Option<Py<PyAny>>> {
        match self.find(&instrument_id) {
            Some(instrument) => Ok(Some(instrument_any_to_pyobject(py, instrument)?)),
            None => Ok(None),
        }
    }

    /// Find an instrument by IB contract ID.
    #[pyo3(name = "find_by_contract_id")]
    fn py_find_by_contract_id(&self, py: Python, contract_id: i32) -> PyResult<Option<Py<PyAny>>> {
        match self.find_by_contract_id(contract_id) {
            Some(instrument) => Ok(Some(instrument_any_to_pyobject(py, instrument)?)),
            None => Ok(None),
        }
    }

    /// Get all cached instruments.
    #[pyo3(name = "get_all")]
    fn py_get_all<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let instruments = self.get_all();
        let py_instruments: PyResult<Vec<_>> = instruments
            .into_iter()
            .map(|inst| instrument_any_to_pyobject(py, inst))
            .collect();
        PyList::new(py, py_instruments?)
    }

    /// Get the number of cached instruments.
    #[pyo3(name = "count")]
    fn py_count(&self) -> usize {
        self.count()
    }

    /// Get price magnifier for an instrument ID.
    #[pyo3(name = "get_price_magnifier")]
    fn py_get_price_magnifier(&self, instrument_id: InstrumentId) -> i32 {
        self.get_price_magnifier(&instrument_id)
    }

    /// Maintain compatibility with the legacy Python provider API.
    ///
    /// Contract details are fetched as part of the data/execution client load flow,
    /// so the standalone provider has nothing to do here.
    #[pyo3(name = "fetch_contract_details")]
    fn py_fetch_contract_details(&self) {}

    /// Determine venue from contract using provider configuration.
    #[pyo3(name = "determine_venue")]
    #[allow(clippy::needless_pass_by_value)]
    fn py_determine_venue(&self, py: Python<'_>, contract: Py<PyAny>) -> PyResult<String> {
        let rust_contract = py_to_contract(contract.bind(py))?;
        Ok(self.determine_venue(&rust_contract, None).to_string())
    }

    /// Convert an instrument ID to cached IB contract details.
    #[pyo3(name = "instrument_id_to_ib_contract_details")]
    fn py_instrument_id_to_ib_contract_details(
        &self,
        py: Python<'_>,
        instrument_id: InstrumentId,
    ) -> PyResult<Option<Py<PyAny>>> {
        self.instrument_id_to_ib_contract_details(&instrument_id)
            .as_ref()
            .map(|details| contract_details_to_pyobject(py, details))
            .transpose()
    }

    /// Batch load multiple instrument IDs.
    ///
    /// Note: This method requires an IB client which is not stored in the provider.
    /// Use `data_client.batch_load()` or `execution_client.batch_load()` instead,
    /// which have access to the IB client.
    #[pyo3(name = "batch_load")]
    #[allow(clippy::needless_pass_by_value)]
    fn py_batch_load<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _ = instrument_ids;
        // NOTE: This method intentionally requires an IB client managed by data/execution clients.
        // The functionality is available via data_client.batch_load() or execution_client.batch_load()
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Err::<usize, _>(to_pyruntime_err(
                "batch_load requires an IB client. Use data_client.batch_load() or execution_client.batch_load() instead.",
            ))
        })
    }

    /// Fetch option chain for an underlying contract with expiry filtering.
    ///
    /// Note: This method requires an IB client which is not stored in the provider.
    /// Use `data_client.fetch_option_chain_by_range()` instead, which has access to the IB client.
    #[pyo3(signature = (underlying_symbol, expiry_min=None, expiry_max=None))]
    fn py_fetch_option_chain_by_range<'py>(
        &self,
        py: Python<'py>,
        underlying_symbol: String,
        expiry_min: Option<String>,
        expiry_max: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _ = (underlying_symbol, expiry_min, expiry_max);
        // NOTE: This method intentionally requires an IB client managed by data/execution clients.
        // The functionality is available via data_client.fetch_option_chain_by_range()
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Err::<usize, _>(to_pyruntime_err(
                "fetch_option_chain_by_range requires an IB client. Use data_client.fetch_option_chain_by_range() instead.",
            ))
        })
    }

    /// Fetch futures chain for a given underlying symbol.
    ///
    /// Note: This method requires an IB client which is not stored in the provider.
    /// Use `data_client.fetch_futures_chain()` instead, which has access to the IB client.
    #[pyo3(signature = (underlying_symbol, expiry_min=None, expiry_max=None))]
    fn py_fetch_futures_chain<'py>(
        &self,
        py: Python<'py>,
        underlying_symbol: String,
        expiry_min: Option<String>,
        expiry_max: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _ = (underlying_symbol, expiry_min, expiry_max);
        // NOTE: This method intentionally requires an IB client managed by data/execution clients.
        // The functionality is available via data_client.fetch_futures_chain()
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Err::<usize, _>(to_pyruntime_err(
                "fetch_futures_chain requires an IB client. Use data_client.fetch_futures_chain() instead.",
            ))
        })
    }

    /// Fetch BAG (spread) contract details.
    ///
    /// Note: This method requires an IB client which is not stored in the provider.
    /// Use `data_client.fetch_bag_contract()` instead, which has access to the IB client.
    #[pyo3(signature = (bag_contract))]
    #[allow(clippy::needless_pass_by_value)]
    fn py_fetch_bag_contract<'py>(
        &self,
        py: Python<'py>,
        bag_contract: String, // Would need proper Contract type
    ) -> PyResult<Bound<'py, PyAny>> {
        let _ = bag_contract;
        // NOTE: This method intentionally requires an IB client managed by data/execution clients.
        // The functionality is available via data_client.fetch_bag_contract()
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Err::<(), _>(to_pyruntime_err(
                "fetch_bag_contract requires an IB client. Use data_client.fetch_bag_contract() instead.",
            ))
        })
    }

    /// Save the current instrument cache to disk.
    ///
    /// # Arguments
    ///
    /// * `cache_path` - Path to the cache file
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or file I/O fails.
    #[pyo3(name = "save_cache")]
    fn py_save_cache<'py>(
        &self,
        py: Python<'py>,
        cache_path: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            provider
                .save_cache(&cache_path)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Load instrument cache from disk if valid.
    ///
    /// # Arguments
    ///
    /// * `cache_path` - Path to the cache file
    ///
    /// # Returns
    ///
    /// Returns `true` if cache was loaded successfully and is valid, `false` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization or file I/O fails (but treats missing file as non-error).
    #[pyo3(name = "load_cache")]
    fn py_load_cache<'py>(
        &self,
        py: Python<'py>,
        cache_path: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            provider
                .load_cache(&cache_path)
                .await
                .map_err(to_pyruntime_err)
        })
    }
}
