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

//! Factory functions for creating Coinbase clients and components.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
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
    common::consts::COINBASE_VENUE,
    config::{CoinbaseDataClientConfig, CoinbaseExecClientConfig},
    data::CoinbaseDataClient,
    execution::CoinbaseExecutionClient,
};

impl ClientConfig for CoinbaseDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for CoinbaseExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Coinbase data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.coinbase", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.coinbase")
)]
pub struct CoinbaseDataClientFactory;

impl CoinbaseDataClientFactory {
    /// Creates a new [`CoinbaseDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for CoinbaseDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for CoinbaseDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let coinbase_config = config
            .as_any()
            .downcast_ref::<CoinbaseDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for CoinbaseDataClientFactory. Expected CoinbaseDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = CoinbaseDataClient::new(client_id, coinbase_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "COINBASE"
    }

    fn config_type(&self) -> &'static str {
        "CoinbaseDataClientConfig"
    }
}

/// Factory for creating Coinbase execution clients.
///
/// Dispatches the spot vs derivatives (CFM) scope from the config's
/// [`AccountType`]: `Cash` bootstraps spot products and uses the
/// `/accounts` endpoint; `Margin` bootstraps perpetual and dated futures,
/// subscribes to the `futures_balance_summary` WebSocket channel, and
/// produces position reports from the CFM endpoints. Other account types
/// are rejected. Hedge mode is not exposed by the venue, so OMS is always
/// `Netting`.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.coinbase", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.coinbase")
)]
pub struct CoinbaseExecutionClientFactory {
    trader_id: TraderId,
    account_id: AccountId,
}

impl CoinbaseExecutionClientFactory {
    /// Creates a new [`CoinbaseExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
        }
    }
}

impl ExecutionClientFactory for CoinbaseExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let coinbase_config = config
            .as_any()
            .downcast_ref::<CoinbaseExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for CoinbaseExecutionClientFactory. Expected CoinbaseExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        let account_type = coinbase_config.account_type;
        if !matches!(account_type, AccountType::Cash | AccountType::Margin) {
            anyhow::bail!(
                "Unsupported account_type {account_type:?} for Coinbase; expected Cash (spot) or Margin (CFM derivatives)"
            );
        }

        let core = ExecutionClientCore::new(
            self.trader_id,
            ClientId::from(name),
            *COINBASE_VENUE,
            OmsType::Netting,
            self.account_id,
            account_type,
            None,
            cache,
        );

        let client = CoinbaseExecutionClient::new(core, coinbase_config)?;

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "COINBASE"
    }

    fn config_type(&self) -> &'static str {
        "CoinbaseExecClientConfig"
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
    fn test_coinbase_data_client_factory_creation() {
        let factory = CoinbaseDataClientFactory::new();
        assert_eq!(factory.name(), "COINBASE");
        assert_eq!(factory.config_type(), "CoinbaseDataClientConfig");
    }

    #[rstest]
    fn test_coinbase_exec_client_config_implements_client_config() {
        let config = CoinbaseExecClientConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<CoinbaseExecClientConfig>();
        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_coinbase_data_client_config_implements_client_config() {
        let config = CoinbaseDataClientConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<CoinbaseDataClientConfig>();
        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_coinbase_data_client_factory_creates_client() {
        setup_test_env();

        let factory = CoinbaseDataClientFactory::new();
        let config = CoinbaseDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("COINBASE-TEST", &config, cache, clock);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("COINBASE-TEST"));
    }

    #[rstest]
    fn test_coinbase_data_client_factory_rejects_wrong_config_type() {
        #[derive(Debug)]
        struct WrongConfig;

        impl ClientConfig for WrongConfig {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let factory = CoinbaseDataClientFactory::new();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("COINBASE-TEST", &WrongConfig, cache, clock);
        let err = match result {
            Ok(_) => panic!("wrong config type should be rejected"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("CoinbaseDataClientFactory"),
            "error should name the factory, was: {msg}"
        );
        assert!(
            msg.contains("CoinbaseDataClientConfig"),
            "error should name the expected config type, was: {msg}"
        );
    }

    fn make_test_exec_config() -> CoinbaseExecClientConfig {
        CoinbaseExecClientConfig {
            api_key: Some("organizations/test-org/apiKeys/test-key".to_string()),
            api_secret: Some("test-pem-placeholder".to_string()),
            ..CoinbaseExecClientConfig::default()
        }
    }

    fn setup_exec_test_env() {
        use nautilus_common::{live::runner::replace_exec_event_sender, messages::ExecutionEvent};
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        replace_exec_event_sender(sender);
    }

    #[rstest]
    fn test_coinbase_execution_client_factory_creation() {
        let factory = CoinbaseExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("COINBASE-001"),
        );
        assert_eq!(factory.name(), "COINBASE");
        assert_eq!(factory.config_type(), "CoinbaseExecClientConfig");
    }

    #[rstest]
    fn test_coinbase_execution_client_factory_creates_cash_client() {
        setup_exec_test_env();

        let factory = CoinbaseExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("COINBASE-001"),
        );
        let config = make_test_exec_config();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let client = factory
            .create("COINBASE-TEST", &config, cache)
            .expect("factory should create exec client with valid config");

        assert_eq!(client.client_id(), ClientId::from("COINBASE-TEST"));
        assert_eq!(client.account_id(), AccountId::from("COINBASE-001"));
        assert_eq!(client.venue(), *COINBASE_VENUE);
        assert_eq!(client.oms_type(), OmsType::Netting);
    }

    #[rstest]
    fn test_coinbase_execution_client_factory_creates_margin_client() {
        setup_exec_test_env();

        let factory = CoinbaseExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("COINBASE-001"),
        );
        let config = CoinbaseExecClientConfig {
            account_type: AccountType::Margin,
            ..make_test_exec_config()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));

        let client = factory
            .create("COINBASE-DERIV", &config, cache)
            .expect("factory should create margin exec client when configured for derivatives");

        assert_eq!(client.client_id(), ClientId::from("COINBASE-DERIV"));
        assert_eq!(client.account_id(), AccountId::from("COINBASE-001"));
        assert_eq!(client.venue(), *COINBASE_VENUE);
        assert_eq!(client.oms_type(), OmsType::Netting);
    }

    #[rstest]
    fn test_coinbase_execution_client_factory_rejects_unsupported_account_type() {
        setup_exec_test_env();

        let factory = CoinbaseExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("COINBASE-001"),
        );
        let config = CoinbaseExecClientConfig {
            account_type: AccountType::Betting,
            ..make_test_exec_config()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));

        let err = factory
            .create("COINBASE-TEST", &config, cache)
            .err()
            .expect("unsupported account type must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("Unsupported account_type"),
            "error should mention unsupported account type, was: {msg}"
        );
    }

    #[rstest]
    fn test_coinbase_execution_client_factory_rejects_wrong_config_type() {
        setup_exec_test_env();

        let factory = CoinbaseExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("COINBASE-001"),
        );
        let wrong_config = CoinbaseDataClientConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("COINBASE-TEST", &wrong_config, cache);
        let err = match result {
            Ok(_) => panic!("wrong config type should be rejected"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("CoinbaseExecutionClientFactory"),
            "error should name the factory, was: {msg}"
        );
        assert!(
            msg.contains("CoinbaseExecClientConfig"),
            "error should name the expected config type, was: {msg}"
        );
    }
}
