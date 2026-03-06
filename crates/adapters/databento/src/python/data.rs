//! Python bindings for the Databento data client.

use std::path::PathBuf;

use nautilus_core::{python::to_pyruntime_err, time::get_atomic_clock_realtime};
use nautilus_common::clients::DataClient;
use nautilus_model::identifiers::ClientId;
use pyo3::prelude::*;

use crate::data::{DatabentoDataClient, DatabentoDataClientConfig};

#[cfg(feature = "python")]
#[pymethods]
impl DatabentoDataClient {
    /// Creates a new [`DatabentoDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if client creation fails.
    #[new]
    #[pyo3(signature = (client_id, api_key, publishers_filepath, use_exchange_as_venue = true, bars_timestamp_on_close = true))]
    pub fn py_new(
        client_id: ClientId,
        api_key: String,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: bool,
    ) -> PyResult<Self> {
        let config = DatabentoDataClientConfig::new(
            api_key,
            publishers_filepath,
            use_exchange_as_venue,
            bars_timestamp_on_close,
        );

        Self::new(client_id, config, get_atomic_clock_realtime())
            .map_err(to_pyruntime_err)
    }

    /// Returns the client ID.
    #[getter]
    pub fn client_id(&self) -> ClientId {
        DataClient::client_id(self)
    }

    /// Returns whether the client is connected.
    #[getter]
    pub fn is_connected(&self) -> bool {
        DataClient::is_connected(self)
    }

    /// Returns whether the client is disconnected.
    #[getter]
    pub fn is_disconnected(&self) -> bool {
        DataClient::is_disconnected(self)
    }

    /// Returns the API key associated with this client.
    #[getter]
    #[pyo3(name = "api_key")]
    pub fn py_api_key(&self) -> &str {
        self.config.api_key()
    }

    /// Returns a masked version of the API key for logging purposes.
    #[getter]
    #[pyo3(name = "api_key_masked")]
    pub fn py_api_key_masked(&self) -> String {
        self.config.api_key_masked()
    }
}
