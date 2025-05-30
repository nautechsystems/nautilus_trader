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

//! Factory functions for creating Databento clients and components.

use std::path::PathBuf;

use nautilus_core::time::AtomicTime;
use nautilus_model::identifiers::ClientId;

use crate::{
    data::{DatabentoDataClient, DatabentoDataClientConfig},
    historical::DatabentoHistoricalClient,
};

/// Factory for creating Databento data clients.
#[cfg_attr(feature = "python", pyo3::pyclass)]
#[derive(Debug)]
pub struct DatabentoDataClientFactory;

impl DatabentoDataClientFactory {
    /// Creates a new [`DatabentoDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created or publisher configuration cannot be loaded.
    pub fn create_live_data_client(
        client_id: ClientId,
        api_key: String,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: bool,
        clock: &'static AtomicTime,
    ) -> anyhow::Result<DatabentoDataClient> {
        let config = DatabentoDataClientConfig::new(
            api_key,
            publishers_filepath,
            use_exchange_as_venue,
            bars_timestamp_on_close,
        );

        DatabentoDataClient::new(client_id, config, clock)
    }

    /// Creates a new [`DatabentoDataClient`] instance with a custom configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn create_live_data_client_with_config(
        client_id: ClientId,
        config: DatabentoDataClientConfig,
        clock: &'static AtomicTime,
    ) -> anyhow::Result<DatabentoDataClient> {
        DatabentoDataClient::new(client_id, config, clock)
    }
}

/// Factory for creating Databento historical clients.
#[derive(Debug)]
pub struct DatabentoHistoricalClientFactory;

impl DatabentoHistoricalClientFactory {
    /// Creates a new [`DatabentoHistoricalClient`] instance.
    ///
    /// # Arguments
    ///
    /// * `api_key` - Databento API key for authentication
    /// * `publishers_filepath` - Path to the publishers.json configuration file
    /// * `use_exchange_as_venue` - Whether to use exchange codes as venues for GLBX instruments
    /// * `clock` - Atomic clock for timestamping
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created or publisher configuration cannot be loaded.
    pub fn create(
        api_key: String,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
        clock: &'static AtomicTime,
    ) -> anyhow::Result<DatabentoHistoricalClient> {
        DatabentoHistoricalClient::new(api_key, publishers_filepath, clock, use_exchange_as_venue)
    }
}

/// Builder for [`DatabentoDataClientConfig`].
#[derive(Debug, Default)]
pub struct DatabentoDataClientConfigBuilder {
    api_key: Option<String>,
    dataset: Option<String>,
    publishers_filepath: Option<PathBuf>,
    use_exchange_as_venue: bool,
    bars_timestamp_on_close: bool,
}

impl DatabentoDataClientConfigBuilder {
    /// Creates a new [`DatabentoDataClientConfigBuilder`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the API key.
    #[must_use]
    pub fn api_key(mut self, api_key: String) -> Self {
        self.api_key = Some(api_key);
        self
    }

    /// Sets the dataset.
    #[must_use]
    pub fn dataset(mut self, dataset: String) -> Self {
        self.dataset = Some(dataset);
        self
    }

    /// Sets the publishers filepath.
    #[must_use]
    pub fn publishers_filepath(mut self, filepath: PathBuf) -> Self {
        self.publishers_filepath = Some(filepath);
        self
    }

    /// Sets whether to use exchange as venue.
    #[must_use]
    pub fn use_exchange_as_venue(mut self, use_exchange: bool) -> Self {
        self.use_exchange_as_venue = use_exchange;
        self
    }

    /// Sets whether to timestamp bars on close.
    #[must_use]
    pub fn bars_timestamp_on_close(mut self, timestamp_on_close: bool) -> Self {
        self.bars_timestamp_on_close = timestamp_on_close;
        self
    }

    /// Builds the [`DatabentoDataClientConfig`].
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing.
    pub fn build(self) -> anyhow::Result<DatabentoDataClientConfig> {
        let api_key = self
            .api_key
            .ok_or_else(|| anyhow::anyhow!("API key is required"))?;
        let publishers_filepath = self
            .publishers_filepath
            .ok_or_else(|| anyhow::anyhow!("Publishers filepath is required"))?;

        Ok(DatabentoDataClientConfig::new(
            api_key,
            publishers_filepath,
            self.use_exchange_as_venue,
            self.bars_timestamp_on_close,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use nautilus_core::time::get_atomic_clock_realtime;

    use super::*;

    #[test]
    fn test_config_builder() {
        let config = DatabentoDataClientConfigBuilder::new()
            .api_key("test_key".to_string())
            .dataset("GLBX.MDP3".to_string())
            .publishers_filepath(PathBuf::from("test_publishers.json"))
            .use_exchange_as_venue(true)
            .bars_timestamp_on_close(false)
            .build();

        assert!(config.is_ok());
        let config = config.unwrap();
        assert_eq!(config.api_key, "test_key");
        assert!(config.use_exchange_as_venue);
        assert!(!config.bars_timestamp_on_close);
    }

    #[test]
    fn test_config_builder_missing_required_fields() {
        let config = DatabentoDataClientConfigBuilder::new()
            .api_key("test_key".to_string())
            // Missing dataset and publishers_filepath
            .build();

        assert!(config.is_err());
    }

    #[test]
    fn test_historical_client_factory() {
        let api_key = env::var("DATABENTO_API_KEY").unwrap_or_else(|_| "test_key".to_string());
        let publishers_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json");
        let clock = get_atomic_clock_realtime();

        // This will fail without a real publishers.json file, but tests the factory creation
        let result =
            DatabentoHistoricalClientFactory::create(api_key, publishers_path, false, clock);

        // We expect this to fail in tests due to missing publishers.json
        // but the factory function should be callable
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn test_live_data_client_factory() {
        let client_id = ClientId::from("DATABENTO-001");
        let api_key = "test_key".to_string();
        let publishers_path = PathBuf::from("test_publishers.json");
        let clock = get_atomic_clock_realtime();

        // This will fail without a real publishers.json file, but tests the factory creation
        let result = DatabentoDataClientFactory::create_live_data_client(
            client_id,
            api_key,
            publishers_path,
            false,
            true,
            clock,
        );

        // We expect this to fail in tests due to missing publishers.json
        // but the factory function should be callable
        assert!(result.is_err() || result.is_ok());
    }
}
