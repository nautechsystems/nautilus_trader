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

//! Factory for creating CoinAPI data clients.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::CacheView,
    clients::DataClient,
    clock::Clock,
    factories::{ClientConfig, DataClientFactory},
};
use nautilus_model::identifiers::ClientId;

use crate::{common::consts::COINAPI, config::CoinApiDataClientConfig, data::CoinApiDataClient};

impl ClientConfig for CoinApiDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating CoinAPI data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.coinapi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.coinapi")
)]
pub struct CoinApiDataClientFactory;

impl CoinApiDataClientFactory {
    /// Creates a new [`CoinApiDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for CoinApiDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for CoinApiDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let coinapi_config = config
            .as_any()
            .downcast_ref::<CoinApiDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for CoinApiDataClientFactory. Expected CoinApiDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = CoinApiDataClient::new(client_id, coinapi_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        COINAPI
    }

    fn config_type(&self) -> &'static str {
        "CoinApiDataClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache, clock::VirtualClock, live::runner::set_data_event_sender, messages::DataEvent,
    };
    use nautilus_core::string::secret::SecretString;
    use rstest::rstest;

    use super::*;

    fn setup_test_env() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);
    }

    fn create(name: &str, config: &dyn ClientConfig) -> anyhow::Result<Box<dyn DataClient>> {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(VirtualClock::new()));
        CoinApiDataClientFactory::new().create(name, config, cache.into(), clock)
    }

    #[rstest]
    fn factory_reports_name_and_config_type() {
        let factory = CoinApiDataClientFactory::new();

        assert_eq!(factory.name(), "COINAPI");
        assert_eq!(factory.config_type(), "CoinApiDataClientConfig");
    }

    #[rstest]
    fn config_downcasts_from_client_config() {
        let config = CoinApiDataClientConfig::builder().credit_budget(7).build();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);

        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<CoinApiDataClientConfig>()
            .unwrap();

        assert_eq!(downcasted.credit_budget, 7);
    }

    #[rstest]
    fn factory_creates_client_with_name_as_client_id() {
        setup_test_env();
        let config = CoinApiDataClientConfig::builder()
            .api_key(SecretString::from("test-key"))
            .build();

        let client = create("COINAPI-TEST", &config).unwrap();

        assert_eq!(client.client_id(), ClientId::from("COINAPI-TEST"));
        assert_eq!(client.venue(), None);
    }

    #[rstest]
    fn factory_propagates_client_construction_error() {
        setup_test_env();
        let config = CoinApiDataClientConfig::builder()
            .api_key(SecretString::from("has space"))
            .build();

        let err = create("COINAPI-TEST", &config).err().unwrap();

        assert_eq!(
            err.to_string(),
            "CoinAPI API key must contain visible ASCII characters"
        );
    }

    #[rstest]
    fn factory_rejects_wrong_config_type() {
        #[derive(Debug)]
        struct WrongConfig;

        impl ClientConfig for WrongConfig {
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let err = create("COINAPI-TEST", &WrongConfig).err().unwrap();

        assert_eq!(
            err.to_string(),
            "Invalid config type for CoinApiDataClientFactory. Expected CoinApiDataClientConfig, was WrongConfig"
        );
    }
}
