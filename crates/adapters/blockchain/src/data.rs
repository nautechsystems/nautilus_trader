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

use std::{cmp::max, sync::Arc};

use futures_util::StreamExt;
use nautilus_common::messages::data::{
    RequestBars, RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestQuotes,
    RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10,
    SubscribeBookSnapshots, SubscribeCustomData, SubscribeIndexPrices, SubscribeInstrument,
    SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices,
    SubscribeQuotes, SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas,
    UnsubscribeBookDepth10, UnsubscribeBookSnapshots, UnsubscribeCustomData,
    UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeInstrumentClose,
    UnsubscribeInstrumentStatus, UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes,
    UnsubscribeTrades,
};
use nautilus_data::client::DataClient;
use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::{
    defi::{
        amm::Pool,
        chain::{Blockchain, SharedChain},
        dex::Dex,
        swap::Swap,
        token::Token,
    },
    identifiers::{ClientId, Venue},
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
            polygon::PolygonRpcClient,
        },
        http::BlockchainHttpRpcClient,
        types::BlockchainMessage,
    },
    validation::validate_address,
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
                BlockchainRpcClientAny::Polygon(PolygonRpcClient::new(wss_rpc_url))
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

    /// Establishes connections to the data providers and cache, then starts block syncing.
    pub async fn connect(&mut self, from_block: Option<u64>) -> anyhow::Result<()> {
        let from_block = from_block.unwrap_or(0);
        if let Some(ref mut rpc_client) = self.rpc_client {
            rpc_client.connect().await?;
        }
        self.cache.connect(from_block).await?;
        self.sync_blocks(from_block).await?;
        Ok(())
    }

    /// Gracefully disconnects from all data providers.
    pub fn disconnect(&mut self) -> anyhow::Result<()> {
        self.hypersync_client.disconnect();
        Ok(())
    }

    /// Synchronizes blockchain data by fetching and caching all blocks from the starting block to the current chain head.
    pub async fn sync_blocks(&mut self, from_block: u64) -> anyhow::Result<()> {
        let from_block = match self.cache.last_cached_block_number() {
            None => from_block,
            Some(cached_block_number) => max(from_block, cached_block_number + 1),
        };
        let current_block = self.hypersync_client.current_block().await;
        log::info!("Syncing blocks from {from_block} to {current_block}");
        let blocks_stream = self
            .hypersync_client
            .request_blocks_stream(from_block, Some(current_block))
            .await;
        tokio::pin!(blocks_stream);
        while let Some(block) = blocks_stream.next().await {
            self.cache.add_block(block).await?;
        }
        log::info!("Finished syncing blocks");
        Ok(())
    }

    /// Fetches and caches all swap events for a specific liquidity pool within the given block range.
    pub async fn sync_pool_swaps(
        &mut self,
        dex_id: &str,
        pool_address: String,
        from_block: Option<u64>,
        to_block: Option<u64>,
    ) -> anyhow::Result<()> {
        let dex_extended = self.get_dex(dex_id)?.clone();
        let pool_address = validate_address(&pool_address)?;
        let pool = match self.cache.get_pool(&pool_address) {
            Some(pool) => pool,
            None => return Err(anyhow::anyhow!("Pool {} is not registered", pool_address)),
        };

        if dex_extended.parse_swap_event_fn.is_none() {
            return Err(anyhow::anyhow!(
                "Swap event parsing function is not set for dex {}",
                dex_id
            ));
        }

        if dex_extended.convert_to_trade_data_fn.is_none() {
            return Err(anyhow::anyhow!(
                "Trade data conversion function is not set for dex {}",
                dex_id
            ));
        }

        let from_block = from_block
            .map(|block| max(block, pool.creation_block))
            .unwrap_or(pool.creation_block);
        let swap_event_signature = dex_extended.swap_created_event.as_ref();
        let parse_swap_event_fn = dex_extended.parse_swap_event_fn.as_ref().unwrap();
        let convert_to_trade_data_fn = dex_extended.convert_to_trade_data_fn.as_ref().unwrap();
        let pool_address = pool.address.to_string();
        let swaps_stream = self
            .hypersync_client
            .request_contract_events_stream(
                from_block,
                to_block,
                &pool_address,
                swap_event_signature,
                Vec::new(),
            )
            .await;

        tokio::pin!(swaps_stream);
        while let Some(log) = swaps_stream.next().await {
            match parse_swap_event_fn(log) {
                Ok(swap_event) => {
                    let timestamp = match self.cache.get_block_timestamp(swap_event.block_number) {
                        Some(num) => num,
                        None => {
                            log::error!(
                                "Missing block timestamp for block {} while processing swap event in the cache",
                                swap_event.block_number
                            );
                            continue;
                        }
                    };
                    let (order_side, size, price) =
                        convert_to_trade_data_fn(&pool.token0, &pool.token1, &swap_event)
                            .expect("Failed to convert swap event to trade data");

                    let swap = Swap::new(
                        self.chain.clone(),
                        dex_extended.dex.clone(),
                        pool.clone(),
                        swap_event.block_number,
                        timestamp.clone(),
                        swap_event.sender,
                        order_side,
                        size,
                        price,
                    );
                    self.cache.add_swap(swap).await?;
                }
                Err(e) => log::error!("Error processing swap event: {}", e),
            }
        }
        log::info!("Finished syncing pool swaps");
        Ok(())
    }

    /// Synchronizes token and pool data for a specific DEX from the specified block.
    pub async fn sync_exchange_pools(
        &mut self,
        dex_id: &str,
        from_block: Option<u64>,
        to_block: Option<u64>,
    ) -> anyhow::Result<()> {
        let from_block = from_block.unwrap_or(0);
        log::info!("Syncing dex exchange pools for {dex_id} from block {from_block}");

        let dex = self.get_dex(dex_id)?.clone();

        // Parsing of pool-created events should be defined in the DEX implementation.
        if dex.parse_pool_created_event_fn.is_none() {
            return Err(anyhow::anyhow!(
                "Pool created event parsing function not set for dex {}",
                dex_id
            ));
        }

        let parse_pool_created_event_fn = dex.parse_pool_created_event_fn.as_ref().unwrap();
        let factory_address = dex.factory.as_ref();
        let pair_created_event_signature = dex.pool_created_event.as_ref();
        let pools_stream = self
            .hypersync_client
            .request_contract_events_stream(
                from_block,
                to_block,
                factory_address,
                pair_created_event_signature,
                Vec::new(),
            )
            .await;

        tokio::pin!(pools_stream);
        while let Some(log) = pools_stream.next().await {
            match parse_pool_created_event_fn(log) {
                Ok(pool) => {
                    self.process_token(pool.token0.to_string()).await?;
                    self.process_token(pool.token1.to_string()).await?;
                    self.process_pool(&dex.dex, pool).await?;
                }
                Err(e) => log::error!("Error processing pool created event: {}", e),
            }
        }
        Ok(())
    }

    /// Processes a token by address, fetching and caching its metadata if not already cached.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching token info or adding to cache fails.
    pub async fn process_token(&mut self, token_address: String) -> anyhow::Result<()> {
        let token_address = validate_address(&token_address)?;
        if self.cache.get_token(&token_address).is_none() {
            let token_info = self.tokens.fetch_token_info(&token_address).await?;
            let token = Token::new(
                self.chain.clone(),
                token_address,
                token_info.name,
                token_info.symbol,
                token_info.decimals,
            );
            log::info!("Saving fetched token {token} in the cache.");
            self.cache.add_token(token).await?;
        }
        Ok(())
    }

    /// Processes a pool creation event by creating and caching a `Pool` entity.
    async fn process_pool(&mut self, dex: &Dex, event: PoolCreated) -> anyhow::Result<()> {
        let pool = Pool::new(
            self.chain.clone(),
            dex.clone(),
            event.pool_address,
            event.block_number,
            self.cache.get_token(&event.token0).cloned().unwrap(),
            self.cache.get_token(&event.token1).cloned().unwrap(),
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

    fn get_dex(&self, dex_id: &str) -> anyhow::Result<&DexExtended> {
        if let Some(dex) = self.cache.get_dex(dex_id) {
            Ok(dex)
        } else {
            Err(anyhow::anyhow!("Dex {dex_id} is not registered"))
        }
    }
}

#[async_trait::async_trait]
impl DataClient for BlockchainDataClient {
    fn client_id(&self) -> ClientId {
        ClientId::from(format!("BLOCKCHAIN-{}", self.chain.name).as_str())
    }

    fn venue(&self) -> Option<Venue> {
        // Blockchain data clients don't map to a single venue since they can provide
        // data for multiple DEXs across the blockchain
        None
    }

    fn start(&self) -> anyhow::Result<()> {
        log::info!("Starting blockchain data client for {}", self.chain.name);
        Ok(())
    }

    fn stop(&self) -> anyhow::Result<()> {
        log::info!("Stopping blockchain data client for {}", self.chain.name);
        Ok(())
    }

    fn reset(&self) -> anyhow::Result<()> {
        log::info!("Resetting blockchain data client for {}", self.chain.name);
        Ok(())
    }

    fn dispose(&self) -> anyhow::Result<()> {
        log::info!("Disposing blockchain data client for {}", self.chain.name);
        Ok(())
    }

    async fn connect(&self) -> anyhow::Result<()> {
        // Note: The current implementation has connect() taking &mut self,
        // but the trait requires &self. For now, we'll log the intent.
        log::info!("Connecting blockchain data client for {}", self.chain.name);
        // TODO: This should call self.connect() but requires refactoring the mutable reference
        Ok(())
    }

    async fn disconnect(&self) -> anyhow::Result<()> {
        // Note: Same issue as connect() - the implementation needs &mut self
        log::info!(
            "Disconnecting blockchain data client for {}",
            self.chain.name
        );
        // TODO: This should call self.disconnect() but requires refactoring the mutable reference
        Ok(())
    }

    fn is_connected(&self) -> bool {
        // For now, we'll assume connected if we have either RPC or HyperSync configured
        self.rpc_client.is_some() || true // HyperSync is always available
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    // Subscription methods - blockchain clients don't support traditional market data subscriptions
    // but we implement them as no-ops for trait compliance

    fn subscribe(&mut self, _cmd: &SubscribeCustomData) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support custom data subscriptions");
        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support instrument subscriptions");
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support instrument subscriptions");
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        _cmd: &SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support instrument status subscriptions");
        Ok(())
    }

    fn subscribe_instrument_close(
        &mut self,
        _cmd: &SubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support instrument close subscriptions");
        Ok(())
    }

    fn subscribe_quotes(&mut self, _cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support quote subscriptions");
        Ok(())
    }

    fn subscribe_trades(&mut self, _cmd: &SubscribeTrades) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support trade subscriptions");
        Ok(())
    }

    fn subscribe_bars(&mut self, _cmd: &SubscribeBars) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support bar subscriptions");
        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, _cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support book snapshot subscriptions");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, _cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support book delta subscriptions");
        Ok(())
    }

    fn subscribe_book_depth10(&mut self, _cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support book depth subscriptions");
        Ok(())
    }

    fn subscribe_index_prices(&mut self, _cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support index price subscriptions");
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, _cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support mark price subscriptions");
        Ok(())
    }

    // Unsubscription methods - all no-ops for blockchain client

    fn unsubscribe(&mut self, _cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instruments(&mut self, _cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, _cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        _cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument_close(
        &mut self,
        _cmd: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, _cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_trades(&mut self, _cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_bars(&mut self, _cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_snapshots(
        &mut self,
        _cmd: &UnsubscribeBookSnapshots,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, _cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_depth10(&mut self, _cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, _cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, _cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    // Request methods - also no-ops for blockchain client since it doesn't provide traditional market data

    fn request_instruments(&self, _request: &RequestInstruments) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support instrument requests");
        Ok(())
    }

    fn request_instrument(&self, _request: &RequestInstrument) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support instrument requests");
        Ok(())
    }

    fn request_quotes(&self, _request: &RequestQuotes) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support quote requests");
        Ok(())
    }

    fn request_trades(&self, _request: &RequestTrades) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support trade requests");
        Ok(())
    }

    fn request_bars(&self, _request: &RequestBars) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support bar requests");
        Ok(())
    }

    fn request_book_snapshot(&self, _request: &RequestBookSnapshot) -> anyhow::Result<()> {
        log::debug!("Blockchain client doesn't support book snapshot requests");
        Ok(())
    }
}
