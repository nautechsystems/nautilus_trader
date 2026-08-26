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

//! Factory functions for creating Lighter clients and components.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::CacheView,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{ClientId, TraderId},
};

use crate::{
    common::consts::LIGHTER,
    config::{LighterDataClientConfig, LighterExecutionClientConfig},
    data::LighterDataClient,
    execution::LighterExecutionClient,
};

impl ClientConfig for LighterDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for LighterExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Lighter data clients.
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.lighter", from_py_object,)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.lighter")
)]
pub struct LighterDataClientFactory;

impl LighterDataClientFactory {
    /// Creates a new [`LighterDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DataClientFactory for LighterDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let lighter_config = config
            .as_any()
            .downcast_ref::<LighterDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for LighterDataClientFactory. Expected LighterDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = LighterDataClient::new(client_id, lighter_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        LIGHTER
    }

    fn config_type(&self) -> &'static str {
        "LighterDataClientConfig"
    }
}

/// Factory for creating Lighter execution clients.
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.lighter", from_py_object,)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.lighter")
)]
pub struct LighterExecutionClientFactory;

impl LighterExecutionClientFactory {
    /// Creates a new [`LighterExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ExecutionClientFactory for LighterExecutionClientFactory {
    fn create(
        &self,
        trader_id: TraderId,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let lighter_config = config
            .as_any()
            .downcast_ref::<LighterExecutionClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for LighterExecutionClientFactory. Expected LighterExecutionClientConfig, was {config:?}",
                )
            })?
            .clone();

        // Lighter is a perpetual futures DEX with margin accounts and one
        // position per market on the L2.
        let core = ExecutionClientCore::new(
            trader_id,
            ClientId::from(name),
            lighter_config.resolved_venue(),
            OmsType::Netting,
            lighter_config.account_id,
            AccountType::Margin,
            None,
            cache,
        );

        let client = LighterExecutionClient::new(core, lighter_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        LIGHTER
    }

    fn config_type(&self) -> &'static str {
        "LighterExecutionClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
        live::runner::replace_data_event_sender,
    };
    use nautilus_model::identifiers::{AccountId, TraderId, Venue};
    use rstest::rstest;

    use super::*;
    use crate::common::consts::LIGHTER_VENUE;

    const PRIVATE_KEY_HEX: &str =
        "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";

    fn exec_config() -> LighterExecutionClientConfig {
        LighterExecutionClientConfig::builder()
            .account_id(AccountId::from("LIGHTER-001"))
            .account_index(12_345)
            .api_key_index(5)
            .private_key(PRIVATE_KEY_HEX.to_string())
            .build()
    }

    #[rstest]
    fn test_lighter_data_client_factory_creation() {
        let factory = LighterDataClientFactory::new();
        assert_eq!(factory.name(), LIGHTER);
        assert_eq!(factory.config_type(), "LighterDataClientConfig");
    }

    #[rstest]
    fn test_lighter_execution_client_factory_creation() {
        let factory = LighterExecutionClientFactory::new();
        assert_eq!(factory.name(), LIGHTER);
        assert_eq!(factory.config_type(), "LighterExecutionClientConfig");
    }

    #[rstest]
    fn test_lighter_exec_client_config_implements_client_config() {
        let config = exec_config();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<LighterExecutionClientConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_lighter_execution_client_factory_rejects_wrong_config_type() {
        let factory = LighterExecutionClientFactory::new();
        let wrong_config = LighterDataClientConfig::default();

        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create(
            TraderId::from("TRADER-001"),
            "LIGHTER-TEST",
            &wrong_config,
            cache.into(),
        );
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid config type")
        );
    }

    #[rstest]
    fn test_lighter_execution_client_factory_constructs() {
        let factory = LighterExecutionClientFactory::new();
        let config = exec_config();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let client = factory
            .create(
                TraderId::from("TRADER-001"),
                "LIGHTER-TEST",
                &config,
                cache.into(),
            )
            .expect("expected client to construct");

        assert!(!client.is_connected());
        assert_eq!(client.venue(), *LIGHTER_VENUE);
    }

    #[rstest]
    fn test_lighter_execution_client_factory_uses_configured_venue() {
        let factory = LighterExecutionClientFactory::new();
        let venue = Venue::new("LIGHTER_ALT");
        let config = LighterExecutionClientConfig::builder()
            .account_id(AccountId::from("LIGHTER-001"))
            .account_index(12_345)
            .api_key_index(5)
            .private_key(PRIVATE_KEY_HEX.to_string())
            .venue(venue)
            .build();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let client = factory
            .create(
                TraderId::from("TRADER-001"),
                "LIGHTER-ALT",
                &config,
                cache.into(),
            )
            .expect("expected client to construct");

        assert_eq!(client.venue(), venue);
    }

    #[rstest]
    fn test_lighter_data_client_factory_uses_configured_venue() {
        let factory = LighterDataClientFactory::new();
        let venue = Venue::new("LIGHTER_ALT");
        let config = LighterDataClientConfig {
            venue: Some(venue),
            ..Default::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);

        let client = factory
            .create("LIGHTER-ALT", &config, cache.into(), clock)
            .expect("expected data client to construct");

        assert_eq!(client.venue(), Some(venue));
    }

    #[rstest]
    fn test_lighter_data_client_factory_rejects_wrong_config_type() {
        let factory = LighterDataClientFactory::new();
        let wrong_config = exec_config();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("LIGHTER-TEST", &wrong_config, cache.into(), clock);
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid config type")
        );
    }
}
