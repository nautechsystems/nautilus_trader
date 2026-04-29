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

//! Factory functions for creating Bybit clients and components.

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
    common::{consts::BYBIT_VENUE, enums::BybitProductType},
    config::{BybitDataClientConfig, BybitExecClientConfig},
    data::BybitDataClient,
    execution::BybitExecutionClient,
};

impl ClientConfig for BybitDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for BybitExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Bybit data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
pub struct BybitDataClientFactory;

impl BybitDataClientFactory {
    /// Creates a new [`BybitDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BybitDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for BybitDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let bybit_config = config
            .as_any()
            .downcast_ref::<BybitDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BybitDataClientFactory. Expected BybitDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = BybitDataClient::new(client_id, bybit_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "BYBIT"
    }

    fn config_type(&self) -> &'static str {
        "BybitDataClientConfig"
    }
}

/// Factory for creating Bybit execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
pub struct BybitExecutionClientFactory {
    trader_id: TraderId,
    account_id: AccountId,
}

impl BybitExecutionClientFactory {
    /// Creates a new [`BybitExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
        }
    }
}

impl ExecutionClientFactory for BybitExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let bybit_config = config
            .as_any()
            .downcast_ref::<BybitExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BybitExecutionClientFactory. Expected BybitExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // Default to Linear if product_types is empty (matches execution client behavior)
        let product_types = if bybit_config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            bybit_config.product_types.clone()
        };

        let has_derivatives = product_types.iter().any(|t| {
            matches!(
                t,
                BybitProductType::Linear | BybitProductType::Inverse | BybitProductType::Option
            )
        });

        let account_type = if has_derivatives {
            AccountType::Margin
        } else {
            AccountType::Cash
        };

        // Bybit uses netting for derivatives, hedging for spot
        let oms_type = if has_derivatives {
            OmsType::Netting
        } else {
            OmsType::Hedging
        };

        let account_id = bybit_config.account_id.unwrap_or(self.account_id);

        let core = ExecutionClientCore::new(
            self.trader_id,
            ClientId::from(name),
            *BYBIT_VENUE,
            oms_type,
            account_id,
            account_type,
            None, // base_currency
            cache,
        );

        let client = BybitExecutionClient::new(core, bybit_config)?;

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "BYBIT"
    }

    fn config_type(&self) -> &'static str {
        "BybitExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        factories::{ClientConfig, ExecutionClientFactory},
    };
    use nautilus_model::identifiers::{AccountId, TraderId};
    use rstest::rstest;

    use super::*;
    use crate::{common::enums::BybitProductType, config::BybitExecClientConfig};

    #[rstest]
    fn test_bybit_execution_client_factory_creation() {
        let factory = BybitExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("BYBIT-001"),
        );
        assert_eq!(factory.name(), "BYBIT");
        assert_eq!(factory.config_type(), "BybitExecClientConfig");
    }

    #[rstest]
    fn test_bybit_exec_client_config_implements_client_config() {
        let config = BybitExecClientConfig::default();

        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<BybitExecClientConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_bybit_execution_client_factory_creates_client_for_spot() {
        let factory = BybitExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("BYBIT-001"),
        );
        let config = BybitExecClientConfig {
            product_types: vec![BybitProductType::Spot],
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            ..Default::default()
        };

        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("BYBIT-TEST", &config, cache);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("BYBIT-TEST"));
    }

    #[rstest]
    fn test_bybit_execution_client_factory_creates_client_for_derivatives() {
        let factory = BybitExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("BYBIT-001"),
        );
        let config = BybitExecClientConfig {
            product_types: vec![BybitProductType::Linear, BybitProductType::Inverse],
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            ..Default::default()
        };

        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("BYBIT-DERIV", &config, cache);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_bybit_execution_client_factory_rejects_wrong_config_type() {
        let factory = BybitExecutionClientFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("BYBIT-001"),
        );
        let wrong_config = BybitDataClientConfig::default();

        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("BYBIT-TEST", &wrong_config, cache);
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
