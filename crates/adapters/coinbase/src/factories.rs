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
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, TraderId},
};
use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};

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
        let mut coinbase_config = config
            .as_any()
            .downcast_ref::<CoinbaseExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for CoinbaseExecutionClientFactory. Expected CoinbaseExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // The Cash flavor always uses a Cash account with Netting OMS and a
        // single spot instrument cache. Callers that want derivatives must
        // use `CoinbaseDerivativesExecutionClientFactory`; the Cash factory
        // ignores any override the user set on `config.account_type`.
        let account_type = AccountType::Cash;
        let oms_type = OmsType::Netting;
        coinbase_config.account_type = account_type;

        let core = ExecutionClientCore::new(
            self.trader_id,
            ClientId::from(name),
            *COINBASE_VENUE,
            oms_type,
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

/// Factory for creating Coinbase derivatives (CFM) execution clients.
///
/// Produces the same [`CoinbaseExecutionClient`] type as
/// [`CoinbaseExecutionClientFactory`] but pins the account type to
/// [`AccountType::Margin`]. The client's bootstrap loads perpetual and
/// dated futures instruments, the `futures_balance_summary` WebSocket
/// channel is subscribed for live balance updates, and position reports
/// come from the CFM endpoints.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.coinbase", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.coinbase")
)]
pub struct CoinbaseDerivativesExecutionClientFactory {
    trader_id: TraderId,
    account_id: AccountId,
}

impl CoinbaseDerivativesExecutionClientFactory {
    /// Creates a new [`CoinbaseDerivativesExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
        }
    }
}

impl ExecutionClientFactory for CoinbaseDerivativesExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let mut coinbase_config = config
            .as_any()
            .downcast_ref::<CoinbaseExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for CoinbaseDerivativesExecutionClientFactory. Expected CoinbaseExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // Derivatives (CFM) positions and balances require a Margin account.
        // Hedge mode is not exposed by the venue; Netting matches the
        // one-way-per-product scope.
        let account_type = AccountType::Margin;
        let oms_type = OmsType::Netting;
        coinbase_config.account_type = account_type;

        let core = ExecutionClientCore::new(
            self.trader_id,
            ClientId::from(name),
            *COINBASE_VENUE,
            oms_type,
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
        cache::Cache, clock::TestClock, live::runner::set_data_event_sender, messages::DataEvent,
    };
    use nautilus_system::factories::{ClientConfig, DataClientFactory};
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
    fn test_coinbase_execution_client_factory_creates_client() {
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
        // Spot / Cash account, Netting OMS per the factory's hardcoded contract.
        assert_eq!(client.oms_type(), OmsType::Netting);
    }

    #[rstest]
    fn test_coinbase_derivatives_factory_creates_margin_client() {
        setup_exec_test_env();

        let factory = CoinbaseDerivativesExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("COINBASE-001"),
        );
        let config = make_test_exec_config();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let client = factory
            .create("COINBASE-DERIV", &config, cache)
            .expect("derivatives factory should create exec client");

        assert_eq!(client.client_id(), ClientId::from("COINBASE-DERIV"));
        assert_eq!(client.account_id(), AccountId::from("COINBASE-001"));
        assert_eq!(client.venue(), *COINBASE_VENUE);
        assert_eq!(client.oms_type(), OmsType::Netting);
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
