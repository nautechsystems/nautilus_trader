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

//! Factory functions for creating Derive clients and components.

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
    identifiers::{AccountId, ClientId, TraderId},
};

use crate::{
    common::consts::{DERIVE, DERIVE_VENUE},
    config::{DeriveDataClientConfig, DeriveExecClientConfig},
    data::DeriveDataClient,
    execution::DeriveExecutionClient,
};

impl ClientConfig for DeriveDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for DeriveExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Derive data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.derive", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.derive")
)]
pub struct DeriveDataClientFactory;

impl DeriveDataClientFactory {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for DeriveDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for DeriveDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let derive_config = config
            .as_any()
            .downcast_ref::<DeriveDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for DeriveDataClientFactory. Expected DeriveDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client = DeriveDataClient::new(ClientId::from(name), derive_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        DERIVE
    }

    fn config_type(&self) -> &'static str {
        stringify!(DeriveDataClientConfig)
    }
}

/// Configuration for creating Derive execution clients via factory.
///
/// Bundles the trader and account identifiers required by
/// [`ExecutionClientCore`] alongside the underlying execution client config.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.derive", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.derive")
)]
pub struct DeriveExecFactoryConfig {
    /// The trader ID for the execution client.
    pub trader_id: TraderId,
    /// The account ID for the execution client.
    pub account_id: AccountId,
    /// The underlying execution client configuration.
    pub config: DeriveExecClientConfig,
}

impl ClientConfig for DeriveExecFactoryConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Derive execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.derive", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.derive")
)]
pub struct DeriveExecutionClientFactory;

impl DeriveExecutionClientFactory {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for DeriveExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for DeriveExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let factory_config = config
            .as_any()
            .downcast_ref::<DeriveExecFactoryConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for DeriveExecutionClientFactory. Expected DeriveExecFactoryConfig, was {config:?}",
                )
            })?
            .clone();

        // Derive perpetuals net per-subaccount; cash accounts are spot.
        let oms_type = OmsType::Netting;
        let account_type = AccountType::Margin;

        let core = ExecutionClientCore::new(
            factory_config.trader_id,
            ClientId::from(name),
            *DERIVE_VENUE,
            oms_type,
            factory_config.account_id,
            account_type,
            None,
            cache,
        );

        let client = DeriveExecutionClient::new(core, factory_config.config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        DERIVE
    }

    fn config_type(&self) -> &'static str {
        stringify!(DeriveExecFactoryConfig)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::{
        cache::Cache, clock::TestClock, live::runner::replace_data_event_sender,
        messages::DataEvent,
    };
    use rstest::rstest;

    use super::*;

    #[derive(Debug)]
    struct WrongConfig;

    impl ClientConfig for WrongConfig {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[rstest]
    fn test_data_client_factory_metadata() {
        let factory = DeriveDataClientFactory::new();

        assert_eq!(factory.name(), DERIVE);
        assert_eq!(factory.config_type(), "DeriveDataClientConfig");
    }

    #[rstest]
    fn test_data_client_factory_creates_client() {
        let factory = DeriveDataClientFactory::new();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let config = DeriveDataClientConfig::default();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(tx);

        let client = factory
            .create(DERIVE, &config, cache.into(), clock)
            .expect("factory creates data client");

        assert_eq!(client.client_id(), ClientId::from(DERIVE));
        assert_eq!(client.venue(), Some(*DERIVE_VENUE));
    }

    #[rstest]
    fn test_data_client_factory_rejects_wrong_config_type() {
        let factory = DeriveDataClientFactory::new();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let wrong_config = WrongConfig;

        let result = factory.create(DERIVE, &wrong_config, cache.into(), clock);

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
    fn test_exec_client_factory_metadata() {
        let factory = DeriveExecutionClientFactory::new();

        assert_eq!(factory.name(), DERIVE);
        assert_eq!(factory.config_type(), "DeriveExecFactoryConfig");
    }

    #[rstest]
    fn test_exec_client_factory_rejects_wrong_config_type() {
        let factory = DeriveExecutionClientFactory::new();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let wrong_config = DeriveDataClientConfig::default();

        let result = factory.create(DERIVE, &wrong_config, cache.into());

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
    fn test_exec_factory_config_implements_client_config() {
        let factory_config = DeriveExecFactoryConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("DERIVE-001"),
            config: DeriveExecClientConfig::default(),
        };

        let boxed: Box<dyn ClientConfig> = Box::new(factory_config);
        assert!(
            boxed
                .as_any()
                .downcast_ref::<DeriveExecFactoryConfig>()
                .is_some()
        );
    }
}
