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

//! Python bindings for the Databento data client factory.

use std::path::PathBuf;

use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::identifiers::ClientId;
use pyo3::prelude::*;

use crate::{data::DatabentoDataClient, factories::DatabentoDataClientFactory};

#[cfg(feature = "python")]
#[pymethods]
impl DatabentoDataClientFactory {
    /// Creates a new [`DatabentoDataClientFactory`] instance.
    #[new]
    pub fn py_new() -> Self {
        Self
    }

    /// Creates a live data client.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if client creation fails.
    #[staticmethod]
    #[pyo3(signature = (client_id, api_key, publishers_filepath, use_exchange_as_venue = true, bars_timestamp_on_close = true))]
    pub fn py_create_live_data_client(
        client_id: ClientId,
        api_key: String,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: bool,
    ) -> PyResult<DatabentoDataClient> {
        DatabentoDataClientFactory::create_live_data_client(
            client_id,
            api_key,
            publishers_filepath,
            use_exchange_as_venue,
            bars_timestamp_on_close,
            get_atomic_clock_realtime(),
        )
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))
    }
}
