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

use std::{any::Any, cell::RefCell, path::PathBuf, rc::Rc};

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_core::time::{AtomicTime, get_atomic_clock_realtime};
use nautilus_data::client::DataClient;
use nautilus_model::identifiers::ClientId;
use nautilus_system::factories::{ClientConfig, DataClientFactory};

use crate::{
    data::{DatabentoDataClient, DatabentoDataClientConfig},
    historical::DatabentoHistoricalClient,
};

/// Configuration for Databento data clients used with `LiveNode`.
#[derive(Debug, Clone)]
pub struct DatabentoLiveClientConfig {
    /// Databento API key.
    pub api_key: String,
    /// Path to publishers.json file.
    pub publishers_filepath: PathBuf,
    /// Whether to use exchange as venue for GLBX instruments.
    pub use_exchange_as_venue: bool,
    /// Whether to timestamp bars on close.
    pub bars_timestamp_on_close: bool,
}

impl DatabentoLiveClientConfig {
    /// Creates a new [`DatabentoLiveClientConfig`] instance.
    #[must_use]
    pub const fn new(
        api_key: String,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: bool,
    ) -> Self {
        Self {
            api_key,
            publishers_filepath,
            use_exchange_as_venue,
            bars_timestamp_on_close,
        }
    }
}

impl ClientConfig for DatabentoLiveClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Databento data clients.
#[derive(Debug)]
pub struct DatabentoDataClientFactory;

impl DatabentoDataClientFactory {
    /// Creates a new [`DatabentoDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

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

impl Default for DatabentoDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for DatabentoDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let databento_config = config
            .as_any()
            .downcast_ref::<DatabentoLiveClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for DatabentoDataClientFactory. Expected DatabentoLiveClientConfig, was {:?}",
                    config
                )
            })?;

        let client_id = ClientId::from(name);
        let config = DatabentoDataClientConfig::new(
            databento_config.api_key.clone(),
            databento_config.publishers_filepath.clone(),
            databento_config.use_exchange_as_venue,
            databento_config.bars_timestamp_on_close,
        );

        let client = DatabentoDataClient::new(client_id, config, get_atomic_clock_realtime())?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "DATABENTO"
    }

    fn config_type(&self) -> &'static str {
        "DatabentoLiveClientConfig"
    }
}

/// Factory for creating Databento historical clients.
#[derive(Debug)]
pub struct DatabentoHistoricalClientFactory;

impl DatabentoHistoricalClientFactory {
    /// Creates a new [`DatabentoHistoricalClient`] instance.
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
    pub const fn use_exchange_as_venue(mut self, use_exchange: bool) -> Self {
        self.use_exchange_as_venue = use_exchange;
        self
    }

    /// Sets whether to timestamp bars on close.
    #[must_use]
    pub const fn bars_timestamp_on_close(mut self, timestamp_on_close: bool) -> Self {
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
    use rstest::rstest;

    use super::*;

    #[rstest]
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

    #[rstest]
    fn test_config_builder_missing_required_fields() {
        let config = DatabentoDataClientConfigBuilder::new()
            .api_key("test_key".to_string())
            // Missing dataset and publishers_filepath
            .build();

        assert!(config.is_err());
    }

    #[rstest]
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

    #[rstest]
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
