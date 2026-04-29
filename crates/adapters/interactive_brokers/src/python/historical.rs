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

//! Python bindings for the Interactive Brokers historical data client.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use ibapi::contracts::Contract;
use nautilus_common::live::get_runtime;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::{Bar, Data},
    identifiers::InstrumentId,
    instruments::any::InstrumentAny,
    python::{data::data_to_pycapsule, instruments::instrument_any_to_pyobject},
};
use pyo3::{prelude::*, types::PyList};

use crate::{
    historical::HistoricalInteractiveBrokersClient, python::conversion::py_list_to_contracts,
};

#[pymethods]
impl HistoricalInteractiveBrokersClient {
    #[new]
    #[allow(clippy::needless_pass_by_value)]
    fn py_new(
        instrument_provider: crate::providers::instruments::InteractiveBrokersInstrumentProvider,
        config: crate::config::InteractiveBrokersDataClientConfig,
    ) -> PyResult<Self> {
        let shared_client = get_runtime()
            .block_on(crate::common::shared_client::get_or_connect(
                &config.host,
                config.port,
                config.client_id,
                config.connection_timeout,
            ))
            .map_err(to_pyruntime_err)?;

        Ok(Self::new(
            Arc::clone(shared_client.as_arc()),
            Arc::new(instrument_provider),
        ))
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    /// Request historical bars.
    ///
    /// # Arguments
    ///
    /// * `bar_specifications` - List of bar specifications (e.g., ["1-HOUR-LAST"])
    /// * `end_date_time` - End date for bars
    /// * `start_date_time` - Optional start date
    /// * `duration` - Optional duration string (e.g., "1 D")
    /// * `contracts` - Optional list of IB contracts (dicts with symbol, sec_type, exchange, currency, etc.)
    /// * `instrument_ids` - Optional list of instrument IDs
    /// * `use_rth` - Use regular trading hours only
    /// * `timeout` - Request timeout in seconds
    #[pyo3(signature = (bar_specifications, end_date_time, start_date_time=None, duration=None, contracts=None, instrument_ids=None, use_rth=true, timeout=60))]
    #[pyo3(name = "request_bars")]
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::needless_pass_by_value)]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_specifications: Vec<String>,
        end_date_time: DateTime<Utc>,
        start_date_time: Option<DateTime<Utc>>,
        duration: Option<String>,
        contracts: Option<Py<PyList>>,
        instrument_ids: Option<Vec<InstrumentId>>,
        use_rth: bool,
        timeout: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let bar_specs = bar_specifications;
        let duration_str = duration;

        // Convert Python contracts list to Rust Contracts
        let contracts_vec: Option<Vec<Contract>> = if let Some(py_contracts) = contracts.as_ref() {
            let py_contracts_bound = py_contracts.bind(py);
            match py_list_to_contracts(py_contracts_bound) {
                Ok(contracts) => Some(contracts),
                Err(e) => {
                    return Err(to_pyvalue_err(format!("Failed to convert contracts: {e}")));
                }
            }
        } else {
            None
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Convert Vec<String> to Vec<&str> for the request
            let bar_specs_refs: Vec<&str> = bar_specs.iter().map(|s| s.as_str()).collect();
            let bars: Vec<Bar> = client
                .request_bars(
                    bar_specs_refs,
                    end_date_time,
                    start_date_time,
                    duration_str.as_deref(),
                    contracts_vec,
                    instrument_ids,
                    use_rth,
                    timeout,
                )
                .await
                .map_err(to_pyruntime_err)?;
            // Convert bars to Python objects
            Ok(bars)
        })
    }

    /// Request historical ticks (quotes or trades).
    ///
    /// # Arguments
    ///
    /// * `tick_type` - Type of ticks: "TRADES" or "BID_ASK"
    /// * `start_date_time` - Start date for ticks
    /// * `end_date_time` - End date for ticks
    /// * `contracts` - Optional list of IB contracts (dicts with symbol, sec_type, exchange, currency, etc.)
    /// * `instrument_ids` - Optional list of instrument IDs
    /// * `use_rth` - Use regular trading hours only
    /// * `timeout` - Request timeout in seconds
    #[pyo3(signature = (tick_type, start_date_time, end_date_time, contracts=None, instrument_ids=None, use_rth=true, timeout=60))]
    #[pyo3(name = "request_ticks")]
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::needless_pass_by_value)]
    fn py_request_ticks<'py>(
        &self,
        py: Python<'py>,
        tick_type: String,
        start_date_time: DateTime<Utc>,
        end_date_time: DateTime<Utc>,
        contracts: Option<Py<PyList>>,
        instrument_ids: Option<Vec<InstrumentId>>,
        use_rth: bool,
        timeout: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        // Convert Python contracts list to Rust Contracts
        let contracts_vec: Option<Vec<Contract>> = if let Some(py_contracts) = contracts.as_ref() {
            let py_contracts_bound = py_contracts.bind(py);
            match py_list_to_contracts(py_contracts_bound) {
                Ok(contracts) => Some(contracts),
                Err(e) => {
                    return Err(to_pyvalue_err(format!("Failed to convert contracts: {e}")));
                }
            }
        } else {
            None
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let data_vec: Vec<Data> = client
                .request_ticks(
                    &tick_type,
                    start_date_time,
                    end_date_time,
                    contracts_vec,
                    instrument_ids,
                    use_rth,
                    timeout,
                )
                .await
                .map_err(to_pyruntime_err)?;
            // Convert Data enum to Python objects using pycapsules
            Python::attach(|py| -> PyResult<Py<PyList>> {
                let py_list = PyList::empty(py);
                for data in data_vec {
                    let py_capsule = data_to_pycapsule(py, data);
                    py_list.append(py_capsule)?;
                }
                Ok(py_list.into())
            })
        })
    }

    /// Request instruments.
    ///
    /// # Arguments
    ///
    /// * `instrument_ids` - Optional list of instrument IDs to load
    /// * `contracts` - Optional list of IB contracts (dicts with symbol, sec_type, exchange, currency, etc.)
    #[pyo3(signature = (instrument_ids=None, contracts=None))]
    #[pyo3(name = "request_instruments")]
    #[allow(clippy::needless_pass_by_value)]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Option<Vec<InstrumentId>>,
        contracts: Option<Py<PyList>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        // Convert Python contracts list to Rust Contracts
        let contracts_vec: Option<Vec<Contract>> = if let Some(py_contracts) = contracts.as_ref() {
            let py_contracts_bound = py_contracts.bind(py);
            match py_list_to_contracts(py_contracts_bound) {
                Ok(contracts) => Some(contracts),
                Err(e) => {
                    return Err(to_pyvalue_err(format!("Failed to convert contracts: {e}")));
                }
            }
        } else {
            None
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments: Vec<InstrumentAny> = client
                .request_instruments(instrument_ids, contracts_vec)
                .await
                .map_err(to_pyruntime_err)?;
            // Convert instruments to Python objects
            Python::attach(|py| -> PyResult<Py<PyList>> {
                let py_list = PyList::empty(py);

                for instrument in instruments {
                    let py_obj =
                        instrument_any_to_pyobject(py, instrument).map_err(to_pyruntime_err)?;
                    py_list.append(py_obj)?;
                }
                Ok(py_list.into())
            })
        })
    }
}
