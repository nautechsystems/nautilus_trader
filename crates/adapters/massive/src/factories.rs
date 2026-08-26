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

//! Factory functions for creating Massive clients and components.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::CacheView,
    clients::DataClient,
    clock::Clock,
    factories::{ClientConfig, DataClientFactory},
};
use nautilus_model::identifiers::ClientId;

use crate::{common::consts::MASSIVE, config::MassiveDataClientConfig, data::MassiveDataClient};

impl ClientConfig for MassiveDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Massive data clients.
#[derive(Debug, Default, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.massive", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.massive")
)]
pub struct MassiveDataClientFactory;

impl MassiveDataClientFactory {
    /// Creates a new [`MassiveDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DataClientFactory for MassiveDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let massive_config = config
            .as_any()
            .downcast_ref::<MassiveDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for MassiveDataClientFactory. Expected MassiveDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = MassiveDataClient::new(client_id, massive_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        MASSIVE
    }

    fn config_type(&self) -> &'static str {
        "MassiveDataClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        factories::{ClientConfig, DataClientFactory},
        live::runner::set_data_event_sender,
        messages::DataEvent,
    };
    use rstest::rstest;

    use super::*;

    fn setup_test_env() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);
    }

    #[rstest]
    fn test_massive_data_client_factory_creation() {
        let factory = MassiveDataClientFactory::new();
        assert_eq!(factory.name(), MASSIVE);
        assert_eq!(factory.config_type(), "MassiveDataClientConfig");
    }

    #[rstest]
    fn test_massive_data_client_config_implements_client_config() {
        let config = MassiveDataClientConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<MassiveDataClientConfig>();
        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_massive_data_client_factory_creates_client() {
        setup_test_env();

        let factory = MassiveDataClientFactory::new();
        let config = MassiveDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("MASSIVE-TEST", &config, cache.into(), clock);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("MASSIVE-TEST"));
    }

    #[rstest]
    fn test_massive_data_client_factory_rejects_wrong_config_type() {
        #[derive(Debug)]
        struct WrongConfig;

        impl ClientConfig for WrongConfig {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let factory = MassiveDataClientFactory::new();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("MASSIVE-TEST", &WrongConfig, cache.into(), clock);
        let err = match result {
            Ok(_) => panic!("wrong config type should be rejected"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("MassiveDataClientFactory"),
            "error should name the factory, was: {msg}"
        );
        assert!(
            msg.contains("MassiveDataClientConfig"),
            "error should name the expected config type, was: {msg}"
        );
    }
}
