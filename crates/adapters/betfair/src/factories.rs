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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::ClientId,
};

use crate::{
    common::consts::{BETFAIR, BETFAIR_VENUE},
    config::{BetfairDataConfig, BetfairExecConfig},
    data::BetfairDataClient,
    execution::BetfairExecutionClient,
    http::client::BetfairHttpClient,
};

/// Factory for creating Betfair data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.betfair", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.betfair")
)]
pub struct BetfairDataClientFactory;

impl BetfairDataClientFactory {
    /// Creates a new [`BetfairDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BetfairDataClientFactory {
    fn default() -> Self {
        Self::new()
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

        betfair_config.validate()?;

        let credential = betfair_config.credential()?;
        let stream_config = betfair_config.stream_config();
        let nav_filter = betfair_config.navigation_filter();
        let currency = betfair_config.currency()?;
        let min_notional = betfair_config.min_notional()?;

        let http_client = BetfairHttpClient::new(
            credential.clone(),
            None,
            None,
            None,
            betfair_config.proxy_url.clone(),
            Some(betfair_config.request_rate_per_second),
            None,
        )?;

        let client = BetfairDataClient::new(
            ClientId::from(name),
            http_client,
            credential,
            stream_config,
            betfair_config,
            nav_filter,
            currency,
            min_notional,
        );

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        BETFAIR
    }

    fn config_type(&self) -> &'static str {
        stringify!(BetfairDataConfig)
    }
}

/// Factory for creating Betfair execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.betfair", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.betfair")
)]
pub struct BetfairExecutionClientFactory;

impl BetfairExecutionClientFactory {
    /// Creates a new [`BetfairExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BetfairExecutionClientFactory {
    fn default() -> Self {
        Self::new()
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

        betfair_config.validate()?;

        let credential = betfair_config.credential()?;
        let stream_config = betfair_config.stream_config();
        let currency = betfair_config.currency()?;

        let http_client = BetfairHttpClient::new(
            credential.clone(),
            None,
            None,
            None,
            betfair_config.proxy_url.clone(),
            Some(betfair_config.request_rate_per_second),
            Some(betfair_config.order_request_rate_per_second),
        )?;

        let core = ExecutionClientCore::new(
            betfair_config.trader_id,
            ClientId::from(name),
            *BETFAIR_VENUE,
            OmsType::Netting,
            betfair_config.account_id,
            AccountType::Betting,
            None,
            cache,
        );

        let client = BetfairExecutionClient::new(
            core,
            http_client,
            credential,
            stream_config,
            betfair_config,
            currency,
        );

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        BETFAIR
    }

    fn config_type(&self) -> &'static str {
        stringify!(BetfairExecConfig)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
        live::runner::set_data_event_sender,
    };
    use rstest::rstest;

    use super::*;
    use crate::config::{BetfairDataConfig, BetfairExecConfig};

    fn data_config() -> BetfairDataConfig {
        BetfairDataConfig {
            username: Some("testuser".to_string()),
            password: Some("testpass".to_string()),
            app_key: Some("testappkey".to_string()),
            ..Default::default()
        }
    }

    fn exec_config() -> BetfairExecConfig {
        BetfairExecConfig {
            username: Some("testuser".to_string()),
            password: Some("testpass".to_string()),
            app_key: Some("testappkey".to_string()),
            ..Default::default()
        }
    }

    #[rstest]
    fn test_betfair_data_client_factory_creation() {
        let factory = BetfairDataClientFactory::new();
        assert_eq!(factory.name(), "BETFAIR");
        assert_eq!(factory.config_type(), "BetfairDataConfig");
    }

    #[rstest]
    fn test_betfair_execution_client_factory_creation() {
        let factory = BetfairExecutionClientFactory::new();
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
    fn test_betfair_data_client_factory_creates_client() {
        let factory = BetfairDataClientFactory::new();
        let config = data_config();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        set_data_event_sender(tx);

        let result = factory.create("BETFAIR", &config, cache, clock);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("BETFAIR"));
    }

    #[rstest]
    fn test_betfair_execution_client_factory_creates_client() {
        let factory = BetfairExecutionClientFactory::new();
        let config = exec_config();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("BETFAIR", &config, cache);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("BETFAIR"));
    }

    #[rstest]
    fn test_betfair_execution_client_factory_rejects_wrong_config_type() {
        let factory = BetfairExecutionClientFactory::new();
        let wrong_config = data_config();
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

    #[rstest]
    fn test_betfair_data_client_factory_rejects_missing_credentials() {
        let factory = BetfairDataClientFactory::new();
        let config = BetfairDataConfig {
            username: Some("testuser".to_string()),
            ..Default::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("BETFAIR", &config, cache, clock);
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("password is missing")
        );
    }
}
