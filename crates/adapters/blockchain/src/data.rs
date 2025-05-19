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

use std::sync::Arc;

use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::defi::{
    amm::Pool,
    chain::{Blockchain, SharedChain},
    dex::Dex,
    token::Token,
};

use crate::{
    cache::BlockchainCache,
    config::BlockchainAdapterConfig,
    contracts::erc20::Erc20Contract,
    events::pool_created::PoolCreated,
    exchanges::extended::DexExtended,
    hypersync::client::HyperSyncClient,
    rpc::{
        BlockchainRpcClient, BlockchainRpcClientAny,
        chains::{
            arbitrum::ArbitrumRpcClient, base::BaseRpcClient, ethereum::EthereumRpcClient,
            polygon::PolygonRpclient,
        },
        http::BlockchainHttpRpcClient,
        types::BlockchainMessage,
    },
};

/// A comprehensive client for interacting with blockchain data from multiple sources.
///
/// The `BlockchainDataClient` serves as a facade that coordinates between different blockchain
/// data providers, caching mechanisms, and contract interactions. It provides a unified interface
/// for retrieving and processing blockchain data, particularly focused on DeFi protocols.
///
/// This client supports two primary data sources:
/// 1. Direct RPC connections to blockchain nodes (via WebSocket).
/// 2. HyperSync API for efficient historical data queries.
#[derive(Debug)]
pub struct BlockchainDataClient {
    /// The blockchain being targeted by this client instance.
    pub chain: SharedChain,
    /// Local cache for blockchain entities.
    cache: BlockchainCache,
    /// Optional WebSocket RPC client for direct blockchain node communication.
    rpc_client: Option<BlockchainRpcClientAny>,
    /// Interface for interacting with ERC20 token contracts.
    tokens: Erc20Contract,
    /// Client for the HyperSync data indexing service.
    hypersync_client: HyperSyncClient,
    /// Channel receiver for messages from the HyperSync client.
    hypersync_rx: tokio::sync::mpsc::UnboundedReceiver<BlockchainMessage>,
}

impl BlockchainDataClient {
    /// Creates a new [`BlockchainDataClient`] instance for the specified chain and configuration.
    ///
    /// # Panics
    ///
    /// Panics if `use_hypersync_for_live_data` is false and `wss_rpc_url` is `None` in the provided config.
    #[must_use]
    pub fn new(chain: SharedChain, config: BlockchainAdapterConfig) -> Self {
        let rpc_client = if !config.use_hypersync_for_live_data && config.wss_rpc_url.is_some() {
            let wss_rpc_url = config.wss_rpc_url.clone().expect("wss_rpc_url is required");
            Some(Self::initialize_rpc_client(chain.name, wss_rpc_url))
        } else {
            None
        };
        let (hypersync_tx, hypersync_rx) = tokio::sync::mpsc::unbounded_channel();
        let hypersync_client = HyperSyncClient::new(chain.clone(), hypersync_tx);
        let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            config.http_rpc_url,
            config.rpc_requests_per_second,
        ));
        let erc20_contract = Erc20Contract::new(http_rpc_client);
        let cache = BlockchainCache::new(chain.clone());

        Self {
            chain,
            cache,
            rpc_client,
            tokens: erc20_contract,
            hypersync_client,
            hypersync_rx,
        }
    }

    /// Creates an appropriate blockchain RPC client for the specified blockchain.
    fn initialize_rpc_client(
        blockchain: Blockchain,
        wss_rpc_url: String,
    ) -> BlockchainRpcClientAny {
        match blockchain {
            Blockchain::Ethereum => {
                BlockchainRpcClientAny::Ethereum(EthereumRpcClient::new(wss_rpc_url))
            }
            Blockchain::Polygon => {
                BlockchainRpcClientAny::Polygon(PolygonRpclient::new(wss_rpc_url))
            }
            Blockchain::Base => BlockchainRpcClientAny::Base(BaseRpcClient::new(wss_rpc_url)),
            Blockchain::Arbitrum => {
                BlockchainRpcClientAny::Arbitrum(ArbitrumRpcClient::new(wss_rpc_url))
            }
            _ => panic!("Unsupported blockchain {blockchain} for RPC connection"),
        }
    }

    /// Initializes the database connection for the blockchain cache.
    pub async fn initialize_cache_database(
        &mut self,
        pg_connect_options: Option<PostgresConnectOptions>,
    ) {
        let pg_connect_options = pg_connect_options.unwrap_or_default();
        log::info!(
            "Initializing blockchain cache on database '{}'",
            pg_connect_options.database
        );
        self.cache
            .initialize_database(pg_connect_options.into())
            .await;
    }

    /// Establishes connections to the data providers and cache.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        if let Some(ref mut rpc_client) = self.rpc_client {
            rpc_client.connect().await?;
        }
        self.cache.connect().await?;
        Ok(())
    }

    /// Gracefully disconnects from all data providers.
    pub fn disconnect(&mut self) -> anyhow::Result<()> {
        self.hypersync_client.disconnect();
        Ok(())
    }

    /// Synchronizes token and pool data for a specific DEX from the specified block.
    pub async fn sync_exchange_pools(
        &mut self,
        dex_id: &str,
        from_block: Option<u32>,
    ) -> anyhow::Result<()> {
        let from_block = from_block.unwrap_or(0);
        log::info!("Syncing dex exchange pools for {dex_id} from block {from_block}");

        let dex = if let Some(dex) = self.cache.get_dex(dex_id) {
            dex.clone()
        } else {
            return Err(anyhow::anyhow!("Dex {} is not registered", dex_id));
        };

        let pools = self
            .hypersync_client
            .request_pool_created_events(from_block, &dex)
            .await;
        for pool in pools {
            self.process_token(pool.token0.to_string()).await;
            self.process_token(pool.token1.to_string()).await;
            self.process_pool(&dex, pool).await?;
        }
        Ok(())
    }

    /// Processes a token by address, fetching and caching its metadata if not already cached.
    async fn process_token(&mut self, token_address: String) {
        if self.cache.get_token(&token_address).is_none() {
            let token_info = self.tokens.fetch_token_info(&token_address).await.unwrap();
            let token = Token::new(
                self.chain.clone(),
                token_address,
                token_info.name,
                token_info.symbol,
                token_info.decimals,
            );
            log::info!("Saving fetched token {token} in the cache.");
            self.cache.add_token(token).await.unwrap();
        }
    }

    /// Processes a pool creation event by creating and caching a `Pool` entity.
    async fn process_pool(&mut self, dex: &Dex, event: PoolCreated) -> anyhow::Result<()> {
        let pool = Pool::new(
            self.chain.clone(),
            dex.clone(),
            event.pool_address.to_string(),
            event.block_number,
            self.cache
                .get_token(&event.token0.to_string())
                .cloned()
                .unwrap(),
            self.cache
                .get_token(&event.token1.to_string())
                .cloned()
                .unwrap(),
            event.fee,
            event.tick_spacing,
        );
        self.cache.add_pool(pool).await?;
        Ok(())
    }

    /// Registers a decentralized exchange with the client.
    pub async fn register_exchange(&mut self, dex: DexExtended) -> anyhow::Result<()> {
        let dex_id = dex.id();
        log::info!("Registering blockchain exchange {}", &dex_id);
        self.cache.add_dex(dex_id, dex).await?;
        Ok(())
    }

    /// Processes incoming messages from the HyperSync client.
    pub async fn process_hypersync_message(&mut self) {
        while let Some(msg) = self.hypersync_rx.recv().await {
            match msg {
                BlockchainMessage::Block(block) => {
                    log::info!("{block}");
                }
            }
        }
    }

    /// Processes incoming messages from the RPC client.
    pub async fn process_rpc_message(&mut self) {
        if let Some(rpc_client) = self.rpc_client.as_mut() {
            loop {
                match rpc_client.next_rpc_message().await {
                    Ok(msg) => match msg {
                        BlockchainMessage::Block(block) => {
                            log::info!("{block}");
                        }
                    },
                    Err(e) => {
                        log::error!("Error processing rpc message: {e}");
                    }
                }
            }
        }
    }

    /// Subscribes to new blockchain blocks from the available data source.
    ///
    /// # Panics
    ///
    /// Panics if using the RPC client and the block subscription request fails.
    pub async fn subscribe_blocks(&mut self) {
        if let Some(rpc_client) = self.rpc_client.as_mut() {
            rpc_client.subscribe_blocks().await.unwrap();
        } else {
            self.hypersync_client.subscribe_blocks();
        }
    }

    /// Unsubscribes from block events.
    ///
    /// # Panics
    ///
    /// Panics if using the RPC client and the block unsubscription request fails.
    pub async fn unsubscribe_blocks(&mut self) {
        if let Some(rpc_client) = self.rpc_client.as_mut() {
            rpc_client.unsubscribe_blocks().await.unwrap();
        } else {
            self.hypersync_client.unsubscribe_blocks();
        }
    }
}
