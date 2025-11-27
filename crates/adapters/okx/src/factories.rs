// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Factory functions for creating OKX clients and components.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_data::client::DataClient;
use nautilus_execution::client::{ExecutionClient, base::ExecutionClientCore};
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::ClientId,
};
use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};

use crate::{
    common::{consts::OKX_VENUE, enums::OKXInstrumentType},
    config::{OKXDataClientConfig, OKXExecClientConfig},
    data::OKXDataClient,
    execution::OKXExecutionClient,
};

impl ClientConfig for OKXDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for OKXExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating OKX data clients.
#[derive(Debug)]
pub struct OKXDataClientFactory;

impl OKXDataClientFactory {
    /// Creates a new [`OKXDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for OKXDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for OKXDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let okx_config = config
            .as_any()
            .downcast_ref::<OKXDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for OKXDataClientFactory. Expected OKXDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = OKXDataClient::new(client_id, okx_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "OKX"
    }

    fn config_type(&self) -> &'static str {
        "OKXDataClientConfig"
    }
}

/// Factory for creating OKX execution clients.
#[derive(Debug)]
pub struct OKXExecutionClientFactory;

impl OKXExecutionClientFactory {
    /// Creates a new [`OKXExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for OKXExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for OKXExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let okx_config = config
            .as_any()
            .downcast_ref::<OKXExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for OKXExecutionClientFactory. Expected OKXExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        let has_derivatives = okx_config.instrument_types.iter().any(|t| {
            matches!(
                t,
                OKXInstrumentType::Swap | OKXInstrumentType::Futures | OKXInstrumentType::Option
            )
        });

        let account_type = if okx_config.use_spot_margin || has_derivatives {
            AccountType::Margin
        } else {
            AccountType::Cash
        };

        // OKX uses netting for derivatives, hedging for spot
        let oms_type = if has_derivatives {
            OmsType::Netting
        } else {
            OmsType::Hedging
        };

        let core = ExecutionClientCore::new(
            okx_config.trader_id,
            ClientId::from(name),
            *OKX_VENUE,
            oms_type,
            okx_config.account_id,
            account_type,
            None, // base_currency
            clock,
            cache,
        );

        let client = OKXExecutionClient::new(core, okx_config)?;

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "OKX"
    }

    fn config_type(&self) -> &'static str {
        "OKXExecClientConfig"
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_model::identifiers::{AccountId, TraderId};
    use nautilus_system::factories::{ClientConfig, ExecutionClientFactory};
    use rstest::rstest;

    use super::*;
    use crate::{common::enums::OKXInstrumentType, config::OKXExecClientConfig};

    #[rstest]
    fn test_okx_execution_client_factory_creation() {
        let factory = OKXExecutionClientFactory::new();
        assert_eq!(factory.name(), "OKX");
        assert_eq!(factory.config_type(), "OKXExecClientConfig");
    }

    #[rstest]
    fn test_okx_execution_client_factory_default() {
        let factory = OKXExecutionClientFactory::new();
        assert_eq!(factory.name(), "OKX");
    }

    #[rstest]
    fn test_okx_exec_client_config_implements_client_config() {
        let config = OKXExecClientConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("OKX-001"),
            instrument_types: vec![OKXInstrumentType::Spot],
            ..Default::default()
        };

        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config.as_any().downcast_ref::<OKXExecClientConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_okx_execution_client_factory_creates_client_for_spot() {
        let factory = OKXExecutionClientFactory::new();
        let config = OKXExecClientConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("OKX-001"),
            instrument_types: vec![OKXInstrumentType::Spot],
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            api_passphrase: Some("test_pass".to_string()),
            ..Default::default()
        };

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("OKX-TEST", &config, cache, clock);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("OKX-TEST"));
    }

    #[rstest]
    fn test_okx_execution_client_factory_creates_client_for_derivatives() {
        let factory = OKXExecutionClientFactory::new();
        let config = OKXExecClientConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("OKX-001"),
            instrument_types: vec![OKXInstrumentType::Swap, OKXInstrumentType::Futures],
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            api_passphrase: Some("test_pass".to_string()),
            ..Default::default()
        };

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("OKX-DERIV", &config, cache, clock);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_okx_execution_client_factory_rejects_wrong_config_type() {
        let factory = OKXExecutionClientFactory::new();
        let wrong_config = OKXDataClientConfig::default();

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("OKX-TEST", &wrong_config, cache, clock);
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
