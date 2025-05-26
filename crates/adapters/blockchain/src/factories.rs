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

//! Factory for creating blockchain data clients.

use std::{any::Any, cell::RefCell, rc::Rc, sync::Arc};

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_data::client::DataClient;
use nautilus_model::defi::chain::Chain;
use nautilus_system::factories::{ClientConfig, DataClientFactory};

use crate::{config::BlockchainAdapterConfig, data::BlockchainDataClient};

/// Configuration for blockchain data clients.
#[derive(Debug, Clone)]
pub struct BlockchainClientConfig {
    /// The blockchain adapter configuration.
    pub adapter_config: BlockchainAdapterConfig,
    /// The blockchain chain configuration.
    pub chain: Arc<Chain>,
}

impl BlockchainClientConfig {
    /// Creates a new [`BlockchainClientConfig`] instance.
    #[must_use]
    pub const fn new(adapter_config: BlockchainAdapterConfig, chain: Arc<Chain>) -> Self {
        Self {
            adapter_config,
            chain,
        }
    }
}

impl ClientConfig for BlockchainClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating blockchain data clients.
///
/// This factory creates `BlockchainDataClient` instances configured for different blockchain networks
/// (Ethereum, Arbitrum, Base, Polygon) with appropriate RPC and HyperSync configurations.
#[derive(Debug)]
pub struct BlockchainDataClientFactory;

impl BlockchainDataClientFactory {
    /// Creates a new [`BlockchainDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BlockchainDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

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
            .downcast_ref::<BlockchainClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BlockchainDataClientFactory. Expected BlockchainClientConfig, got {:?}",
                    config
                )
            })?;

        let client = BlockchainDataClient::new(
            blockchain_config.chain.clone(),
            blockchain_config.adapter_config.clone(),
        );

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "BLOCKCHAIN"
    }

    fn config_type(&self) -> &'static str {
        "BlockchainClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_model::defi::chain::{Blockchain, chains};
    use nautilus_system::factories::DataClientFactory;
    use rstest::rstest;

    use crate::{
        config::BlockchainAdapterConfig,
        factories::{BlockchainClientConfig, BlockchainDataClientFactory},
    };

    #[rstest]
    fn test_blockchain_client_config_creation() {
        let adapter_config = BlockchainAdapterConfig::new(
            "https://eth-mainnet.example.com".to_string(),
            None,
            None,
            false,
        );
        let chain = Arc::new(chains::ETHEREUM.clone());

        let config = BlockchainClientConfig::new(adapter_config, chain);

        assert_eq!(config.chain.name, Blockchain::Ethereum);
        assert_eq!(
            config.adapter_config.http_rpc_url,
            "https://eth-mainnet.example.com"
        );
    }

    #[rstest]
    fn test_factory_creation() {
        let factory = BlockchainDataClientFactory::new();
        assert_eq!(factory.name(), "BlockchainDataClientFactory");
        assert_eq!(factory.config_type(), "BlockchainClientConfig");
    }
}
