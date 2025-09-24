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

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_data::client::DataClient;
use nautilus_system::factories::{ClientConfig, DataClientFactory};

use crate::{config::BlockchainDataClientConfig, data::client::BlockchainDataClient};

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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.blockchain")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_model::defi::chain::{Blockchain, chains};
    use nautilus_system::factories::DataClientFactory;
    use rstest::rstest;

    use crate::{config::BlockchainDataClientConfig, factories::BlockchainDataClientFactory};

    #[rstest]
    fn test_blockchain_data_client_config_creation() {
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
    fn test_factory_creation() {
        let factory = BlockchainDataClientFactory::new();
        assert_eq!(factory.name(), "BLOCKCHAIN");
        assert_eq!(factory.config_type(), "BlockchainDataClientConfig");
    }
}
