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

//! Factory functions for creating Interactive Brokers clients and components.

use std::{any::Any, cell::RefCell, rc::Rc, sync::Arc};

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
    common::consts::{IB, IB_VENUE},
    config::{InteractiveBrokersDataClientConfig, InteractiveBrokersExecClientConfig},
    data::InteractiveBrokersDataClient,
    execution::InteractiveBrokersExecutionClient,
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

impl ClientConfig for InteractiveBrokersDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for InteractiveBrokersExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Interactive Brokers data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub struct InteractiveBrokersDataClientFactory;

impl InteractiveBrokersDataClientFactory {
    /// Creates a new [`InteractiveBrokersDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for InteractiveBrokersDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for InteractiveBrokersDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let ib_config = config
            .as_any()
            .downcast_ref::<InteractiveBrokersDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for InteractiveBrokersDataClientFactory. Expected InteractiveBrokersDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let instrument_provider = Arc::new(InteractiveBrokersInstrumentProvider::new(
            ib_config.instrument_provider.clone(),
        ));
        let client = InteractiveBrokersDataClient::new(
            ClientId::from(name),
            ib_config,
            instrument_provider,
        )?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        IB
    }

    fn config_type(&self) -> &'static str {
        stringify!(InteractiveBrokersDataClientConfig)
    }
}

/// Factory for creating Interactive Brokers execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub struct InteractiveBrokersExecutionClientFactory {
    trader_id: TraderId,
    account_id: AccountId,
}

impl InteractiveBrokersExecutionClientFactory {
    /// Creates a new [`InteractiveBrokersExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
        }
    }
}

impl ExecutionClientFactory for InteractiveBrokersExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let mut ib_config = config
            .as_any()
            .downcast_ref::<InteractiveBrokersExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for InteractiveBrokersExecutionClientFactory. Expected InteractiveBrokersExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        let account_id = if let Some(account_id) = ib_config.account_id.as_deref() {
            resolve_account_id(name, account_id)?
        } else {
            self.account_id
        };
        ib_config.account_id = Some(account_id.to_string());

        let core = ExecutionClientCore::new(
            self.trader_id,
            ClientId::from(name),
            *IB_VENUE,
            OmsType::Netting,
            account_id,
            AccountType::Margin,
            None, // base_currency: IB accounts can be multi-currency
            cache,
        );

        let instrument_provider = Arc::new(InteractiveBrokersInstrumentProvider::new(
            ib_config.instrument_provider.clone(),
        ));
        let client = InteractiveBrokersExecutionClient::new(core, ib_config, instrument_provider)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        IB
    }

    fn config_type(&self) -> &'static str {
        stringify!(InteractiveBrokersExecClientConfig)
    }
}

fn resolve_account_id(name: &str, account_id: &str) -> anyhow::Result<AccountId> {
    if account_id.contains('-') {
        return AccountId::new_checked(account_id)
            .map_err(|e| anyhow::anyhow!("Invalid Interactive Brokers account_id: {e}"));
    }

    let issuer = if name.is_empty() { IB } else { name };
    AccountId::new_checked(format!("{issuer}-{account_id}"))
        .map_err(|e| anyhow::anyhow!("Invalid Interactive Brokers account_id: {e}"))
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
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_interactive_brokers_data_client_factory_creation() {
        let factory = InteractiveBrokersDataClientFactory::new();
        assert_eq!(factory.name(), IB);
        assert_eq!(factory.config_type(), "InteractiveBrokersDataClientConfig");
    }

    #[rstest]
    fn test_interactive_brokers_data_client_factory_default() {
        let factory = InteractiveBrokersDataClientFactory;
        assert_eq!(factory.name(), IB);
    }

    #[rstest]
    fn test_interactive_brokers_exec_client_factory_creation() {
        let factory = InteractiveBrokersExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("IB-U1234567"),
        );
        assert_eq!(factory.name(), IB);
        assert_eq!(factory.config_type(), "InteractiveBrokersExecClientConfig");
    }

    #[rstest]
    fn test_interactive_brokers_configs_implement_client_config() {
        let data_config = InteractiveBrokersDataClientConfig::default();
        let exec_config = InteractiveBrokersExecClientConfig::default();

        let boxed_data_config: Box<dyn ClientConfig> = Box::new(data_config);
        let boxed_exec_config: Box<dyn ClientConfig> = Box::new(exec_config);

        assert!(
            boxed_data_config
                .as_any()
                .downcast_ref::<InteractiveBrokersDataClientConfig>()
                .is_some()
        );
        assert!(
            boxed_exec_config
                .as_any()
                .downcast_ref::<InteractiveBrokersExecClientConfig>()
                .is_some()
        );
    }

    #[rstest]
    fn test_interactive_brokers_data_client_factory_creates_client() {
        let factory = InteractiveBrokersDataClientFactory::new();
        let config = InteractiveBrokersDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(data_tx);

        let result = factory.create("IB-TEST", &config, cache.into(), clock);

        assert!(result.is_ok());
        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("IB-TEST"));
    }

    #[rstest]
    fn test_interactive_brokers_exec_client_factory_creates_client() {
        let factory = InteractiveBrokersExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("IB-U1234567"),
        );
        let config = InteractiveBrokersExecClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("IB-TEST", &config, cache.into());

        assert!(result.is_ok());
        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("IB-TEST"));
        assert_eq!(client.account_id(), AccountId::from("IB-U1234567"));
    }

    #[rstest]
    fn test_interactive_brokers_exec_client_factory_uses_config_account_id() {
        let factory = InteractiveBrokersExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("IB-U1234567"),
        );
        let config = InteractiveBrokersExecClientConfig {
            account_id: Some(String::from("U7654321")),
            ..Default::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("IB-CUSTOM", &config, cache.into());

        assert!(result.is_ok());
        let client = result.unwrap();
        assert_eq!(client.account_id(), AccountId::from("IB-CUSTOM-U7654321"));
    }
}
