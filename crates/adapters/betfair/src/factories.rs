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

//! Factory functions for creating Betfair clients and components.

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
    types::{Currency, Money},
};
use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};

use crate::{
    common::{consts::BETFAIR_VENUE, credential::BetfairCredential},
    config::{BetfairDataConfig, BetfairExecConfig},
    data::BetfairDataClient,
    execution::BetfairExecutionClient,
    http::client::BetfairHttpClient,
    provider::NavigationFilter,
    stream::config::BetfairStreamConfig,
};

impl ClientConfig for BetfairDataConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for BetfairExecConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Betfair data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.betfair", from_py_object)
)]
pub struct BetfairDataClientFactory {
    credential: BetfairCredential,
    stream_config: BetfairStreamConfig,
    nav_filter: NavigationFilter,
    currency: Currency,
    min_notional: Option<Money>,
}

impl BetfairDataClientFactory {
    /// Creates a new [`BetfairDataClientFactory`] instance.
    #[must_use]
    pub fn new(
        credential: BetfairCredential,
        stream_config: BetfairStreamConfig,
        nav_filter: NavigationFilter,
        currency: Currency,
        min_notional: Option<Money>,
    ) -> Self {
        Self {
            credential,
            stream_config,
            nav_filter,
            currency,
            min_notional,
        }
    }
}

impl DataClientFactory for BetfairDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let betfair_config = config
            .as_any()
            .downcast_ref::<BetfairDataConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BetfairDataClientFactory. Expected BetfairDataConfig, was {config:?}",
                )
            })?
            .clone();

        let http_client = BetfairHttpClient::new(self.credential.clone(), None, None, None, None)?;

        let client_id = ClientId::from(name);
        let client = BetfairDataClient::new(
            client_id,
            http_client,
            self.credential.clone(),
            self.stream_config.clone(),
            betfair_config,
            self.nav_filter.clone(),
            self.currency,
            self.min_notional,
        );

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "BETFAIR"
    }

    fn config_type(&self) -> &'static str {
        "BetfairDataConfig"
    }
}

/// Factory for creating Betfair execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.betfair", from_py_object)
)]
pub struct BetfairExecutionClientFactory {
    trader_id: TraderId,
    account_id: AccountId,
    credential: BetfairCredential,
    stream_config: BetfairStreamConfig,
    currency: Currency,
}

impl BetfairExecutionClientFactory {
    /// Creates a new [`BetfairExecutionClientFactory`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        account_id: AccountId,
        credential: BetfairCredential,
        stream_config: BetfairStreamConfig,
        currency: Currency,
    ) -> Self {
        Self {
            trader_id,
            account_id,
            credential,
            stream_config,
            currency,
        }
    }
}

impl ExecutionClientFactory for BetfairExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let betfair_config = config
            .as_any()
            .downcast_ref::<BetfairExecConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BetfairExecutionClientFactory. Expected BetfairExecConfig, was {config:?}",
                )
            })?
            .clone();

        let http_client = BetfairHttpClient::new(self.credential.clone(), None, None, None, None)?;

        let core = ExecutionClientCore::new(
            self.trader_id,
            ClientId::from(name),
            *BETFAIR_VENUE,
            OmsType::Netting,
            self.account_id,
            AccountType::Betting,
            None,
            cache,
        );

        let client = BetfairExecutionClient::new(
            core,
            http_client,
            self.credential.clone(),
            self.stream_config.clone(),
            betfair_config,
            self.currency,
        );

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "BETFAIR"
    }

    fn config_type(&self) -> &'static str {
        "BetfairExecConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::cache::Cache;
    use nautilus_model::identifiers::{AccountId, TraderId};
    use nautilus_system::factories::{ClientConfig, ExecutionClientFactory};
    use rstest::rstest;

    use super::*;
    use crate::config::{BetfairDataConfig, BetfairExecConfig};

    fn test_credential() -> BetfairCredential {
        BetfairCredential::new(
            "testuser".to_string(),
            "testpass".to_string(),
            "testappkey".to_string(),
        )
    }

    #[rstest]
    fn test_betfair_data_client_factory_creation() {
        let factory = BetfairDataClientFactory::new(
            test_credential(),
            BetfairStreamConfig::default(),
            NavigationFilter::default(),
            Currency::GBP(),
            None,
        );
        assert_eq!(factory.name(), "BETFAIR");
        assert_eq!(factory.config_type(), "BetfairDataConfig");
    }

    #[rstest]
    fn test_betfair_execution_client_factory_creation() {
        let factory = BetfairExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("BETFAIR-001"),
            test_credential(),
            BetfairStreamConfig::default(),
            Currency::GBP(),
        );
        assert_eq!(factory.name(), "BETFAIR");
        assert_eq!(factory.config_type(), "BetfairExecConfig");
    }

    #[rstest]
    fn test_betfair_data_config_implements_client_config() {
        let config = BetfairDataConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config.as_any().downcast_ref::<BetfairDataConfig>();
        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_betfair_exec_config_implements_client_config() {
        let config = BetfairExecConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config.as_any().downcast_ref::<BetfairExecConfig>();
        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_betfair_execution_client_factory_creates_client() {
        let factory = BetfairExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("BETFAIR-001"),
            test_credential(),
            BetfairStreamConfig::default(),
            Currency::GBP(),
        );
        let config = BetfairExecConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("BETFAIR", &config, cache);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("BETFAIR"));
    }

    #[rstest]
    fn test_betfair_execution_client_factory_rejects_wrong_config_type() {
        let factory = BetfairExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("BETFAIR-001"),
            test_credential(),
            BetfairStreamConfig::default(),
            Currency::GBP(),
        );
        let wrong_config = BetfairDataConfig::default();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("BETFAIR", &wrong_config, cache);
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
