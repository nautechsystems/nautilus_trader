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

//! Factory functions for creating Hyperliquid clients and components.

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
    common::consts::HYPERLIQUID_VENUE,
    config::{HyperliquidDataClientConfig, HyperliquidExecClientConfig},
    data::HyperliquidDataClient,
    execution::HyperliquidExecutionClient,
};

impl ClientConfig for HyperliquidDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for HyperliquidExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Hyperliquid data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object
    )
)]
pub struct HyperliquidDataClientFactory;

impl HyperliquidDataClientFactory {
    /// Creates a new [`HyperliquidDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for HyperliquidDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for HyperliquidDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let hyperliquid_config = config
            .as_any()
            .downcast_ref::<HyperliquidDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for HyperliquidDataClientFactory. Expected HyperliquidDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = HyperliquidDataClient::new(client_id, hyperliquid_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "HYPERLIQUID"
    }

    fn config_type(&self) -> &'static str {
        "HyperliquidDataClientConfig"
    }
}

/// Configuration for creating Hyperliquid execution clients via factory.
///
/// This wraps [`HyperliquidExecClientConfig`] with the additional trader and account
/// identifiers required by the [`ExecutionClientCore`].
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object
    )
)]
pub struct HyperliquidExecFactoryConfig {
    /// The trader ID for the execution client.
    pub trader_id: TraderId,
    /// The account ID for the execution client.
    pub account_id: AccountId,
    /// The underlying execution client configuration.
    pub config: HyperliquidExecClientConfig,
}

impl ClientConfig for HyperliquidExecFactoryConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Hyperliquid execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object
    )
)]
pub struct HyperliquidExecutionClientFactory;

impl HyperliquidExecutionClientFactory {
    /// Creates a new [`HyperliquidExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for HyperliquidExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for HyperliquidExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let factory_config = config
            .as_any()
            .downcast_ref::<HyperliquidExecFactoryConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for HyperliquidExecutionClientFactory. Expected HyperliquidExecFactoryConfig, was {config:?}",
                )
            })?
            .clone();

        // Hyperliquid uses netting for perpetual futures
        let oms_type = OmsType::Netting;

        // Hyperliquid is always margin (perpetual futures)
        let account_type = AccountType::Margin;

        let core = ExecutionClientCore::new(
            factory_config.trader_id,
            ClientId::from(name),
            *HYPERLIQUID_VENUE,
            oms_type,
            factory_config.account_id,
            account_type,
            None,
            cache,
        );

        let client = HyperliquidExecutionClient::new(core, factory_config.config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "HYPERLIQUID"
    }

    fn config_type(&self) -> &'static str {
        "HyperliquidExecFactoryConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_model::identifiers::{AccountId, TraderId};
    use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
    use rstest::rstest;

    use super::*;
    use crate::config::{HyperliquidDataClientConfig, HyperliquidExecClientConfig};

    #[rstest]
    fn test_hyperliquid_data_client_factory_creation() {
        let factory = HyperliquidDataClientFactory::new();
        assert_eq!(factory.name(), "HYPERLIQUID");
        assert_eq!(factory.config_type(), "HyperliquidDataClientConfig");
    }

    #[rstest]
    fn test_hyperliquid_data_client_factory_default() {
        let factory = HyperliquidDataClientFactory;
        assert_eq!(factory.name(), "HYPERLIQUID");
    }

    #[rstest]
    fn test_hyperliquid_execution_client_factory_creation() {
        let factory = HyperliquidExecutionClientFactory::new();
        assert_eq!(factory.name(), "HYPERLIQUID");
        assert_eq!(factory.config_type(), "HyperliquidExecFactoryConfig");
    }

    #[rstest]
    fn test_hyperliquid_execution_client_factory_default() {
        let factory = HyperliquidExecutionClientFactory;
        assert_eq!(factory.name(), "HYPERLIQUID");
    }

    #[rstest]
    fn test_hyperliquid_data_client_config_implements_client_config() {
        let config = HyperliquidDataClientConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<HyperliquidDataClientConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_hyperliquid_exec_factory_config_implements_client_config() {
        let config = HyperliquidExecFactoryConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("HYPERLIQUID-001"),
            config: HyperliquidExecClientConfig::new(Some("test_private_key".to_string())),
        };

        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<HyperliquidExecFactoryConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_hyperliquid_data_client_factory_rejects_wrong_config_type() {
        let factory = HyperliquidDataClientFactory::new();
        let wrong_config = HyperliquidExecFactoryConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("HYPERLIQUID-001"),
            config: HyperliquidExecClientConfig::new(Some("test_private_key".to_string())),
        };

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("HYPERLIQUID-TEST", &wrong_config, cache, clock);
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
    fn test_hyperliquid_execution_client_factory_rejects_wrong_config_type() {
        let factory = HyperliquidExecutionClientFactory::new();
        let wrong_config = HyperliquidDataClientConfig::default();

        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("HYPERLIQUID-TEST", &wrong_config, cache);
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
