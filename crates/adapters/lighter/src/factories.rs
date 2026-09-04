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
        messages::DataEvent,
    };
    use nautilus_model::identifiers::{AccountId, ClientId, TraderId, Venue};
    use rstest::rstest;

    use super::*;
    use crate::common::{
        consts::{
            LIGHTER_CLIENT_ID, LIGHTER_ROBINHOOD_CLIENT_ID, LIGHTER_ROBINHOOD_VENUE, LIGHTER_VENUE,
        },
        enums::LighterDeployment,
    };

    const PRIVATE_KEY_HEX: &str =
        "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";

    fn exec_config() -> LighterExecutionClientConfig {
        LighterExecutionClientConfig::builder()
            .account_id(AccountId::from("LIGHTER-001"))
            .account_index(12_345)
            .api_key_index(5)
            .private_key(PRIVATE_KEY_HEX.into())
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
    }

    #[rstest]
    fn test_factories_preserve_client_ids_and_custom_venue() {
        let venue = Venue::from("LIGHTER_CUSTOM");
        let data_config = LighterDataClientConfig {
            deployment: LighterDeployment::Robinhood,
            venue: Some(venue),
            ..Default::default()
        };

        let exec_config = LighterExecutionClientConfig::builder()
            .account_id(AccountId::from("LIGHTER_CUSTOM-001"))
            .deployment(LighterDeployment::Robinhood)
            .venue(venue)
            .build();

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(data_tx);

        let data_client = LighterDataClientFactory::new()
            .create("RH-DATA", &data_config, cache.clone().into(), clock)
            .expect("expected data client to construct");
        let exec_client = LighterExecutionClientFactory::new()
            .create(
                TraderId::from("TRADER-001"),
                "RH-EXEC",
                &exec_config,
                cache.into(),
            )
            .expect("expected execution client to construct");

        assert_eq!(data_client.client_id(), ClientId::from("RH-DATA"));
        assert_eq!(data_client.venue(), Some(venue));
        assert_eq!(exec_client.client_id(), ClientId::from("RH-EXEC"));
        assert_eq!(exec_client.venue(), venue);
        assert_eq!(
            exec_client.account_id(),
            AccountId::from("LIGHTER_CUSTOM-001")
        );
    }

    #[rstest]
    fn test_factories_preserve_distinct_deployment_identities() {
        let lighter_data_config = LighterDataClientConfig::default();
        let robinhood_data_config = LighterDataClientConfig {
            deployment: LighterDeployment::Robinhood,
            ..Default::default()
        };

        let lighter_exec_config = exec_config();

        let robinhood_exec_config = LighterExecutionClientConfig::builder()
            .account_id(AccountId::from("LIGHTER_ROBINHOOD-001"))
            .account_index(12_345)
            .api_key_index(5)
            .private_key(PRIVATE_KEY_HEX.into())
            .deployment(LighterDeployment::Robinhood)
            .build();

        let cache = Rc::new(RefCell::new(Cache::default()));
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(data_tx);

        let lighter_data = LighterDataClientFactory::new()
            .create(
                LIGHTER_CLIENT_ID.as_str(),
                &lighter_data_config,
                cache.clone().into(),
                Rc::new(RefCell::new(TestClock::new())),
            )
            .expect("expected Lighter data client to construct");

        let robinhood_data = LighterDataClientFactory::new()
            .create(
                LIGHTER_ROBINHOOD_CLIENT_ID.as_str(),
                &robinhood_data_config,
                cache.clone().into(),
                Rc::new(RefCell::new(TestClock::new())),
            )
            .expect("expected Robinhood data client to construct");

        let lighter_exec = LighterExecutionClientFactory::new()
            .create(
                TraderId::from("TRADER-001"),
                LIGHTER_CLIENT_ID.as_str(),
                &lighter_exec_config,
                cache.clone().into(),
            )
            .expect("expected Lighter execution client to construct");

        let robinhood_exec = LighterExecutionClientFactory::new()
            .create(
                TraderId::from("TRADER-001"),
                LIGHTER_ROBINHOOD_CLIENT_ID.as_str(),
                &robinhood_exec_config,
                cache.into(),
            )
            .expect("expected Robinhood execution client to construct");

        assert_eq!(lighter_data.client_id(), *LIGHTER_CLIENT_ID);
        assert_eq!(lighter_data.venue(), Some(*LIGHTER_VENUE));
        assert_eq!(robinhood_data.client_id(), *LIGHTER_ROBINHOOD_CLIENT_ID);
        assert_eq!(robinhood_data.venue(), Some(*LIGHTER_ROBINHOOD_VENUE));
        assert_eq!(lighter_exec.client_id(), *LIGHTER_CLIENT_ID);
        assert_eq!(lighter_exec.account_id(), AccountId::from("LIGHTER-001"));
        assert_eq!(lighter_exec.venue(), *LIGHTER_VENUE);
        assert_eq!(robinhood_exec.client_id(), *LIGHTER_ROBINHOOD_CLIENT_ID);
        assert_eq!(
            robinhood_exec.account_id(),
            AccountId::from("LIGHTER_ROBINHOOD-001")
        );
        assert_eq!(robinhood_exec.venue(), *LIGHTER_ROBINHOOD_VENUE);
    }

    #[rstest]
    fn test_execution_factory_rejects_account_issuer_venue_mismatch() {
        let config = LighterExecutionClientConfig::builder()
            .deployment(LighterDeployment::Robinhood)
            .build();

        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = LighterExecutionClientFactory::new().create(
            TraderId::from("TRADER-001"),
            "RH-EXEC",
            &config,
            cache.into(),
        );

        let error = match result {
            Ok(_) => panic!("mismatched account issuer should fail"),
            Err(e) => e,
        };

        assert!(error.to_string().contains(
            "account ID issuer LIGHTER does not match configured venue LIGHTER_ROBINHOOD"
        ));
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
