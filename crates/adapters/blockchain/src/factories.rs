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

//! Factory for creating blockchain data clients.

use std::{cell::RefCell, rc::Rc};

#[cfg(feature = "hypersync")]
use std::any::Any;

use nautilus_common::{cache::Cache, clients::ExecutionClient};
#[cfg(feature = "hypersync")]
use nautilus_common::{clients::DataClient, clock::Clock};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::ClientId,
};
#[cfg(feature = "hypersync")]
use nautilus_system::factories::DataClientFactory;
use nautilus_system::{ExecutionClientFactory, factories::ClientConfig};

use crate::{
    config::BlockchainExecutionClientConfig, execution::client::BlockchainExecutionClient,
};

#[cfg(feature = "hypersync")]
use crate::{config::BlockchainDataClientConfig, data::client::BlockchainDataClient};

#[cfg(feature = "hypersync")]
impl ClientConfig for BlockchainDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating blockchain data clients.
///
/// This factory creates `BlockchainDataClient` instances configured for different blockchain networks
/// (Ethereum, Arbitrum, Base, Polygon) with appropriate RPC and HyperSync configurations.
#[derive(Debug, Clone)]
#[cfg(feature = "hypersync")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.blockchain",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainDataClientFactory;

#[cfg(feature = "hypersync")]
impl BlockchainDataClientFactory {
    /// Creates a new [`BlockchainDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[cfg(feature = "hypersync")]
impl Default for BlockchainDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "hypersync")]
impl DataClientFactory for BlockchainDataClientFactory {
    fn create(
        &self,
        _name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let blockchain_config = config
            .as_any()
            .downcast_ref::<BlockchainDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BlockchainDataClientFactory. Expected `BlockchainDataClientConfig`, was {config:?}"
                )
            })?;

        let client = BlockchainDataClient::new(blockchain_config.clone());

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "BLOCKCHAIN"
    }

    fn config_type(&self) -> &'static str {
        "BlockchainDataClientConfig"
    }
}

/// Factory for creating blockchain execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.blockchain",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainExecutionClientFactory;

impl BlockchainExecutionClientFactory {
    /// Creates a new [`BlockchainExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BlockchainExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for BlockchainExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let blockchain_execution_config = config
            .as_any()
            .downcast_ref::<BlockchainExecutionClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BlockchainExecutionClientFactory. Expected `BlockchainExecutionClientConfig`, was {config:?}"
                )
            })?;

        let core_execution_client = ExecutionClientCore::new(
            blockchain_execution_config.trader_id,
            ClientId::from(name),
            blockchain_execution_config.venue,
            OmsType::Netting,
            blockchain_execution_config.client_id,
            AccountType::Cash,
            None,
            cache,
        );

        let client = BlockchainExecutionClient::new(
            core_execution_client,
            blockchain_execution_config.clone(),
        )?;

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "BLOCKCHAIN"
    }

    fn config_type(&self) -> &'static str {
        "BlockchainExecutionClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    #[cfg(feature = "hypersync")]
    use std::sync::Arc;

    use nautilus_common::cache::Cache;
    use nautilus_model::defi::chain::chains;
    use nautilus_model::{
        identifiers::{AccountId, TraderId, Venue},
        stubs::TestDefault,
    };
    use nautilus_system::ExecutionClientFactory;
    use rstest::rstest;

    use crate::{
        config::BlockchainExecutionClientConfig, factories::BlockchainExecutionClientFactory,
    };

    #[rstest]
    #[cfg(feature = "hypersync")]
    fn test_blockchain_data_client_config_creation() {
        use crate::config::BlockchainDataClientConfig;
        use nautilus_model::defi::chain::Blockchain;

        let chain = Arc::new(chains::ETHEREUM.clone());
        let config = BlockchainDataClientConfig::new(
            chain,
            vec![],
            "https://eth-mainnet.example.com".to_string(),
            None,
            None,
            None,
            false,
            None,
            None,
            None,
        );

        assert_eq!(config.chain.name, Blockchain::Ethereum);
        assert_eq!(config.http_rpc_url, "https://eth-mainnet.example.com");
    }

    #[rstest]
    #[cfg(feature = "hypersync")]
    fn test_factory_creation() {
        use crate::factories::BlockchainDataClientFactory;

        let factory = BlockchainDataClientFactory::new();
        assert_eq!(
            nautilus_system::factories::DataClientFactory::name(&factory),
            "BLOCKCHAIN"
        );
        assert_eq!(
            nautilus_system::factories::DataClientFactory::config_type(&factory),
            "BlockchainDataClientConfig"
        );
    }

    #[rstest]
    fn test_execution_factory_propagates_config_venue() {
        let venue = Venue::new("Arbitrum:UniswapV3");
        let config = BlockchainExecutionClientConfig::new(
            TraderId::test_default(),
            AccountId::test_default(),
            venue,
            chains::ARBITRUM.clone(),
            String::from("0x49E96E255bA418d08E66c35b588E2f2F3766E1d0"),
            None,
            String::from("https://arb.example.com"),
            None,
        );
        let factory = BlockchainExecutionClientFactory::new();
        let cache = Rc::new(RefCell::new(Cache::default()));

        let client = factory
            .create("BLOCKCHAIN", &config, cache)
            .expect("Execution client should be created");

        assert_eq!(client.venue(), venue);
    }
}
