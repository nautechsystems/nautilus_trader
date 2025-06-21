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

use alloy::primitives::U256;
use futures_util::StreamExt;
use nautilus_common::{
    messages::{
        DataEvent,
        defi::{
            DefiDataCommand, DefiSubscribeCommand, DefiUnsubscribeCommand, SubscribeBlocks,
            SubscribePool, SubscribePoolLiquidityUpdates, SubscribePoolSwaps, UnsubscribeBlocks,
            UnsubscribePool, UnsubscribePoolLiquidityUpdates, UnsubscribePoolSwaps,
        },
    },
    runner::get_data_event_sender,
};
use nautilus_data::client::DataClient;
use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::{
    defi::{
        Block, Blockchain, DefiData, Dex, Pool, PoolLiquidityUpdate, PoolLiquidityUpdateType,
        PoolSwap, SharedChain, SharedPool, Token,
    },
    identifiers::{ClientId, Venue},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    cache::BlockchainCache,
    config::BlockchainDataClientConfig,
    contracts::erc20::Erc20Contract,
    decode::u256_to_quantity,
    events::pool_created::PoolCreatedEvent,
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
    /// The configuration for the data client.
    pub config: BlockchainDataClientConfig,
    /// Local cache for blockchain entities.
    cache: BlockchainCache,
    /// Optional WebSocket RPC client for direct blockchain node communication.
    rpc_client: Option<BlockchainRpcClientAny>,
    /// Interface for interacting with ERC20 token contracts.
    tokens: Erc20Contract,
    /// Client for the HyperSync data indexing service.
    hypersync_client: HyperSyncClient,
    /// Channel receiver for messages from the HyperSync client.
    hypersync_rx: Option<tokio::sync::mpsc::UnboundedReceiver<BlockchainMessage>>,
    /// Channel sender for publishing data events to the `AsyncRunner`.
    data_sender: UnboundedSender<DataEvent>,
    /// Channel sender for commands to be processed asynchronously.
    command_tx: UnboundedSender<DefiDataCommand>,
    /// Channel receiver for commands to be processed asynchronously.
    command_rx: Option<UnboundedReceiver<DefiDataCommand>>,
    /// Background task for processing messages.
    process_task: Option<tokio::task::JoinHandle<()>>,
}

impl BlockchainDataClient {
    /// Creates a new [`BlockchainDataClient`] instance for the specified configuration.
    ///
    /// # Panics
    ///
    /// Panics if `use_hypersync_for_live_data` is false and `wss_rpc_url` is `None` in the provided config.
    #[must_use]
    pub fn new(config: BlockchainDataClientConfig) -> Self {
        let chain = config.chain.clone();
        let rpc_client = if !config.use_hypersync_for_live_data && config.wss_rpc_url.is_some() {
            let wss_rpc_url = config.wss_rpc_url.clone().expect("wss_rpc_url is required");
            Some(Self::initialize_rpc_client(chain.name, wss_rpc_url))
        } else {
            None
        };
        let (hypersync_tx, hypersync_rx) = tokio::sync::mpsc::unbounded_channel();
        let hypersync_client = HyperSyncClient::new(chain.clone(), hypersync_tx);
        let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            config.http_rpc_url.clone(),
            config.rpc_requests_per_second,
        ));
        let erc20_contract = Erc20Contract::new(http_rpc_client);
        let cache = BlockchainCache::new(chain.clone());
        let data_sender = get_data_event_sender();
        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();

        Self {
            chain,
            config,
            cache,
            rpc_client,
            tokens: erc20_contract,
            hypersync_client,
            hypersync_rx: Some(hypersync_rx),
            data_sender,
            command_tx,
            command_rx: Some(command_rx),
            process_task: None,
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
        tracing::info!(
            "Initializing blockchain cache on database '{}'",
            pg_connect_options.database
        );
        self.cache
            .initialize_database(pg_connect_options.into())
            .await;
    }

    /// Spawns a unified task that handles both commands and data from the same client instances.
    /// This replaces both the command processor and hypersync forwarder with a single unified handler.
    fn spawn_process_task(&mut self) {
        let command_rx = match self.command_rx.take() {
            Some(r) => r,
            None => {
                tracing::error!("Command receiver already taken, not spawning handler");
                return;
            }
        };

        let hypersync_rx = match self.hypersync_rx.take() {
            Some(r) => r,
            None => {
                tracing::error!("HyperSync receiver already taken, not spawning handler");
                return;
            }
        };

        let mut hypersync_client = std::mem::replace(
            &mut self.hypersync_client,
            HyperSyncClient::new(self.chain.clone(), tokio::sync::mpsc::unbounded_channel().0),
        );
        let mut rpc_client = self.rpc_client.take();
        let data_sender = self.data_sender.clone();

        let handle = tokio::spawn(async move {
            tracing::debug!("Started task 'process'");

            let mut command_rx = command_rx;
            let mut hypersync_rx = hypersync_rx;

            loop {
                tokio::select! {
                    command = command_rx.recv() => {
                        match command {
                            Some(cmd) => {
                                if let Err(e) = Self::process_command(
                                    cmd,
                                    &mut hypersync_client,
                                    rpc_client.as_mut()
                                ).await {
                                    tracing::error!("Error processing command: {e}");
                                }
                            }
                            None => {
                                tracing::debug!("Command channel closed");
                                break;
                            }
                        }
                    }
                    data = hypersync_rx.recv() => {
                        match data {
                            Some(msg) => {
                                let data_event = match msg {
                                    BlockchainMessage::Block(block) => {
                                        DataEvent::DeFi(DefiData::Block(block))
                                    }
                                    BlockchainMessage::Swap(swap) => {
                                        DataEvent::DeFi(DefiData::PoolSwap(swap))
                                    }
                                };

                                if let Err(e) = data_sender.send(data_event) {
                                    tracing::error!("Failed to send data event: {e}");
                                    break;
                                }
                            }
                            None => {
                                tracing::debug!("HyperSync data channel closed");
                                break;
                            }
                        }
                    }
                }
            }

            tracing::debug!("Stopped task 'process'");
        });

        self.process_task = Some(handle);
    }

    async fn process_command(
        command: DefiDataCommand,
        hypersync_client: &mut HyperSyncClient,
        rpc_client: Option<&mut BlockchainRpcClientAny>,
    ) -> anyhow::Result<()> {
        match command {
            DefiDataCommand::Subscribe(cmd) => {
                Self::handle_subscribe_command(cmd, hypersync_client, rpc_client).await
            }
            DefiDataCommand::Unsubscribe(cmd) => {
                Self::handle_unsubscribe_command(cmd, hypersync_client, rpc_client).await
            }
        }
    }

    /// Handles DeFi subscribe commands with access to mutable client instances.
    async fn handle_subscribe_command(
        command: DefiSubscribeCommand,
        hypersync_client: &mut HyperSyncClient,
        mut rpc_client: Option<&mut BlockchainRpcClientAny>,
    ) -> anyhow::Result<()> {
        match command {
            DefiSubscribeCommand::Blocks(_cmd) => {
                tracing::info!("Processing subscribe blocks command");

                // Try RPC client first if available, otherwise use HyperSync
                if let Some(ref mut rpc) = rpc_client {
                    if let Err(e) = rpc.subscribe_blocks().await {
                        tracing::warn!(
                            "RPC blocks subscription failed: {e}, falling back to HyperSync"
                        );
                        hypersync_client.subscribe_blocks();
                    } else {
                        tracing::info!("Successfully subscribed to blocks via RPC");
                    }
                } else {
                    tracing::info!("Subscribing to blocks via HyperSync");
                    hypersync_client.subscribe_blocks();
                }

                Ok(())
            }
            DefiSubscribeCommand::Pool(_cmd) => {
                tracing::info!("Processing subscribe pool command");
                // Pool subscriptions are typically handled at the application level
                // as they involve specific pool addresses and don't require blockchain streaming
                tracing::warn!("Pool subscriptions are handled at application level");
                Ok(())
            }
            DefiSubscribeCommand::PoolSwaps(_cmd) => {
                tracing::info!("Processing subscribe pool swaps command");
                tracing::warn!("Pool swaps subscription not yet implemented");
                // TODO: Implement actual pool swaps subscription logic
                Ok(())
            }
            DefiSubscribeCommand::PoolLiquidityUpdates(_cmd) => {
                tracing::info!("Processing subscribe pool liquidity updates command");
                tracing::warn!("Pool liquidity updates subscription not yet implemented");
                // TODO: Implement actual pool liquidity updates subscription logic
                Ok(())
            }
        }
    }

    /// Handles DeFi unsubscribe commands with access to mutable client instances.
    async fn handle_unsubscribe_command(
        command: DefiUnsubscribeCommand,
        hypersync_client: &mut HyperSyncClient,
        rpc_client: Option<&mut BlockchainRpcClientAny>,
    ) -> anyhow::Result<()> {
        match command {
            DefiUnsubscribeCommand::Blocks(_cmd) => {
                tracing::info!("Processing unsubscribe blocks command");

                // TODO: Implement RPC unsubscription when available
                if rpc_client.is_some() {
                    tracing::warn!("RPC blocks unsubscription not yet implemented");
                }

                // Use HyperSync client for unsubscription
                hypersync_client.unsubscribe_blocks();
                tracing::info!("Unsubscribed from blocks via HyperSync");

                Ok(())
            }
            DefiUnsubscribeCommand::Pool(_cmd) => {
                tracing::info!("Processing unsubscribe pool command");
                // Pool unsubscriptions are typically handled at the application level
                tracing::warn!("Pool unsubscriptions are handled at application level");
                Ok(())
            }
            DefiUnsubscribeCommand::PoolSwaps(_cmd) => {
                tracing::info!("Processing unsubscribe pool swaps command");
                tracing::warn!("Pool swaps unsubscription not yet implemented");
                // TODO: Implement pool swaps unsubscription logic
                Ok(())
            }
            DefiUnsubscribeCommand::PoolLiquidityUpdates(_cmd) => {
                tracing::info!("Processing unsubscribe pool liquidity updates command");
                tracing::warn!("Pool liquidity updates unsubscription not yet implemented");
                // TODO: Implement pool liquidity updates unsubscription logic
                Ok(())
            }
        }
    }

    /// Synchronizes blockchain data by fetching and caching all blocks from the starting block to the current chain head.
    pub async fn sync_blocks(&mut self, from_block: Option<u64>) -> anyhow::Result<()> {
        let from_block = if let Some(b) = from_block {
            b
        } else {
            tracing::warn!("Skipping blocks sync: `from_block` not supplied");
            return Ok(());
        };

        let from_block = match self.cache.last_cached_block_number() {
            None => from_block,
            Some(cached_block_number) => max(from_block, cached_block_number + 1),
        };

        let current_block = self.hypersync_client.current_block().await;
        tracing::info!("Syncing blocks from {from_block} to {current_block}");

        let blocks_stream = self
            .hypersync_client
            .request_blocks_stream(from_block, Some(current_block))
            .await;

        tokio::pin!(blocks_stream);

        while let Some(block) = blocks_stream.next().await {
            self.cache.add_block(block).await?;
        }

        tracing::info!("Finished syncing blocks");
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
        let pool = self.get_pool(&pool_address)?;
        let from_block =
            from_block.map_or(pool.creation_block, |block| max(block, pool.creation_block));
        tracing::info!(
            "Syncing pool swaps for {} on Dex {} from block {}{}",
            pool.ticker(),
            dex_extended.name,
            from_block,
            to_block.map_or(String::new(), |block| format!(" to {block}"))
        );
        let swap_event_signature = dex_extended.swap_created_event.as_ref();
        let stream = self
            .hypersync_client
            .request_contract_events_stream(
                from_block,
                to_block,
                &pool.address.to_string(),
                swap_event_signature,
                Vec::new(),
            )
            .await;

        tokio::pin!(stream);

        while let Some(log) = stream.next().await {
            let swap_event = dex_extended.parse_swap_event(log)?;
            let Some(timestamp) = self.cache.get_block_timestamp(swap_event.block_number) else {
                tracing::error!(
                    "Missing block timestamp in the cache for block {} while processing swap event",
                    swap_event.block_number
                );
                continue;
            };
            let (side, size, price) = dex_extended
                .convert_to_trade_data(&pool.token0, &pool.token1, &swap_event)
                .expect("Failed to convert swap event to trade data");

            let swap = PoolSwap::new(
                self.chain.clone(),
                dex_extended.dex.clone(),
                pool.clone(),
                swap_event.block_number,
                swap_event.transaction_hash,
                swap_event.transaction_index,
                swap_event.log_index,
                *timestamp,
                swap_event.sender,
                side,
                size,
                price,
            );

            self.cache.add_pool_swap(&swap).await?;

            self.send_swap(swap);
        }

        tracing::info!("Finished syncing pool swaps");
        Ok(())
    }

    /// Fetches and caches all mint events for a specific liquidity pool within the given block range.
    pub async fn sync_pool_mints(
        &self,
        dex_id: &str,
        pool_address: String,
        from_block: Option<u64>,
        to_block: Option<u64>,
    ) -> anyhow::Result<()> {
        let dex_extended = self.get_dex(dex_id)?.clone();
        let pool = self.get_pool(&pool_address)?.clone();
        let from_block =
            from_block.map_or(pool.creation_block, |block| max(block, pool.creation_block));
        tracing::info!(
            "Syncing pool mints for {} on Dex {} from block {from_block}{}",
            pool.ticker(),
            dex_extended.name,
            to_block.map_or(String::new(), |block| format!(" to {block}"))
        );
        let mint_event_signature = dex_extended.mint_created_event.as_ref();
        let stream = self
            .hypersync_client
            .request_contract_events_stream(
                from_block,
                to_block,
                &pool.address.to_string(),
                mint_event_signature,
                Vec::new(),
            )
            .await;

        tokio::pin!(stream);

        while let Some(log) = stream.next().await {
            let mint_event = dex_extended.parse_mint_event(log)?;
            let Some(timestamp) = self.cache.get_block_timestamp(mint_event.block_number) else {
                tracing::error!(
                    "Missing block timestamp in the cache for block {} while processing mint event",
                    mint_event.block_number
                );
                continue;
            };
            let liquidity = u256_to_quantity(
                U256::from(mint_event.amount),
                self.chain.native_currency_decimals,
            )?;
            let amount0 = u256_to_quantity(mint_event.amount0, pool.token0.decimals)?;
            let amount1 = u256_to_quantity(mint_event.amount1, pool.token1.decimals)?;

            let liquidity_update = PoolLiquidityUpdate::new(
                self.chain.clone(),
                dex_extended.dex.clone(),
                pool.clone(),
                PoolLiquidityUpdateType::Mint,
                mint_event.block_number,
                mint_event.transaction_hash,
                mint_event.transaction_index,
                mint_event.log_index,
                Some(mint_event.sender),
                mint_event.owner,
                liquidity,
                amount0,
                amount1,
                mint_event.tick_lower,
                mint_event.tick_upper,
                *timestamp,
            );

            self.cache.add_liquidity_update(&liquidity_update).await?;

            self.send_liquidity_update(liquidity_update);
        }

        tracing::info!("Finished syncing pool mints");
        Ok(())
    }

    /// Fetches and caches all burn events for a specific liquidity pool within the given block range.
    pub async fn sync_pool_burns(
        &self,
        dex_id: &str,
        pool_address: String,
        from_block: Option<u64>,
        to_block: Option<u64>,
    ) -> anyhow::Result<()> {
        let dex_extended = self.get_dex(dex_id)?.clone();
        let pool = self.get_pool(&pool_address)?.clone();
        let from_block =
            from_block.map_or(pool.creation_block, |block| max(block, pool.creation_block));
        tracing::info!(
            "Syncing pool burns for {} on Dex {} from block {from_block}{}",
            pool.ticker(),
            dex_extended.name,
            to_block.map_or(String::new(), |block| format!(" to {block}"))
        );
        let burn_event_signature = dex_extended.burn_created_event.as_ref();
        let stream = self
            .hypersync_client
            .request_contract_events_stream(
                from_block,
                to_block,
                &pool.address.to_string(),
                burn_event_signature,
                Vec::new(),
            )
            .await;

        tokio::pin!(stream);

        while let Some(log) = stream.next().await {
            let burn_event = dex_extended.parse_burn_event(log)?;
            let Some(timestamp) = self.cache.get_block_timestamp(burn_event.block_number) else {
                tracing::error!(
                    "Missing block timestamp in the cache for block {} while processing burn event",
                    burn_event.block_number
                );
                continue;
            };
            let liquidity = u256_to_quantity(
                U256::from(burn_event.amount),
                self.chain.native_currency_decimals,
            )?;
            let amount0 = u256_to_quantity(burn_event.amount0, pool.token0.decimals)?;
            let amount1 = u256_to_quantity(burn_event.amount1, pool.token1.decimals)?;

            let liquidity_update = PoolLiquidityUpdate::new(
                self.chain.clone(),
                dex_extended.dex.clone(),
                pool.clone(),
                PoolLiquidityUpdateType::Burn,
                burn_event.block_number,
                burn_event.transaction_hash,
                burn_event.transaction_index,
                burn_event.log_index,
                None,
                burn_event.owner,
                liquidity,
                amount0,
                amount1,
                burn_event.tick_lower,
                burn_event.tick_upper,
                *timestamp,
            );

            self.cache.add_liquidity_update(&liquidity_update).await?;

            self.send_liquidity_update(liquidity_update);
        }

        tracing::info!("Finished syncing pool burns");
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
        tracing::info!(
            "Syncing Dex exchange pools for {dex_id} from block {from_block}{}",
            to_block.map_or(String::new(), |block| format!(" to {block}"))
        );

        let dex = self.get_dex(dex_id)?.clone();
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
            let pool = dex.parse_pool_created_event(log)?;
            self.process_token(pool.token0.to_string()).await?;
            self.process_token(pool.token1.to_string()).await?;
            self.process_pool(&dex.dex, pool).await?;
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
            tracing::info!("Saving fetched token {token} in the cache");
            self.cache.add_token(token).await?;
        }

        Ok(())
    }

    /// Processes a pool creation event by creating and caching a `Pool` entity.
    async fn process_pool(&mut self, dex: &Dex, event: PoolCreatedEvent) -> anyhow::Result<()> {
        let pool = Pool::new(
            self.chain.clone(),
            dex.clone(),
            event.pool_address,
            event.block_number,
            self.cache.get_token(&event.token0).cloned().unwrap(),
            self.cache.get_token(&event.token1).cloned().unwrap(),
            event.fee,
            event.tick_spacing,
            nautilus_core::UnixNanos::default(), // Use default timestamp for now
        );
        self.cache.add_pool(pool.clone()).await?;

        Ok(())
    }

    /// Registers a decentralized exchange with the client.
    pub async fn register_exchange(&mut self, dex: DexExtended) -> anyhow::Result<()> {
        let dex_id = dex.id();
        tracing::info!("Registering blockchain exchange {dex_id}");
        self.cache.add_dex(dex_id, dex).await?;
        Ok(())
    }

    /// Processes incoming messages from the HyperSync client.
    pub async fn process_hypersync_messages(&mut self) {
        tracing::info!("Starting task 'process_hypersync_messages'");

        let mut rx = match self.hypersync_rx.take() {
            Some(r) => r,
            None => {
                tracing::warn!("HyperSync receiver already taken, not spawning forwarder");
                return;
            }
        };

        while let Some(msg) = rx.recv().await {
            match msg {
                BlockchainMessage::Block(block) => {
                    self.send_block(block);
                }
                BlockchainMessage::Swap(swap) => {
                    self.send_swap(swap);
                }
            }
        }
    }

    /// Processes incoming messages from the RPC client.
    pub async fn process_rpc_messages(&mut self) {
        tracing::info!("Starting task 'process_rpc_messages'");

        loop {
            let msg = {
                match self
                    .rpc_client
                    .as_mut()
                    .expect("process_rpc_messages: RPC client not initialised")
                    .next_rpc_message()
                    .await
                {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!("Error processing RPC message: {e}");
                        continue;
                    }
                }
            };

            match msg {
                BlockchainMessage::Block(block) => self.send_block(block),
                BlockchainMessage::Swap(swap) => self.send_swap(swap),
            }
        }
    }

    /// Subscribes to new blockchain blocks from the available data source.
    pub async fn subscribe_blocks_async(&mut self) -> anyhow::Result<()> {
        if let Some(rpc_client) = self.rpc_client.as_mut() {
            rpc_client.subscribe_blocks().await?;
        } else {
            self.hypersync_client.subscribe_blocks();
        }

        Ok(())
    }

    /// Subscribes to new blockchain blocks from the available data source.
    pub async fn subscribe_pool_swaps_async(&mut self) -> anyhow::Result<()> {
        if let Some(rpc_client) = self.rpc_client.as_mut() {
            rpc_client.subscribe_swaps().await?;
        } else {
            todo!("Not implemented")
            // self.hypersync_client.subscribe_swaps();
        }

        Ok(())
    }

    /// Unsubscribes from block events.
    pub async fn unsubscribe_blocks_async(&mut self) -> anyhow::Result<()> {
        if let Some(_rpc_client) = self.rpc_client.as_mut() {
            todo!("Not implemented");
            // rpc_client.unsubscribe_blocks().await?;
        } else {
            self.hypersync_client.unsubscribe_blocks();
        }

        Ok(())
    }

    /// Unsubscribes from swap events.
    pub async fn unsubscribe_pool_swaps_async(&mut self) -> anyhow::Result<()> {
        if let Some(_rpc_client) = self.rpc_client.as_mut() {
            todo!("Not implemented");
            // rpc_client.unsubscribe_blocks().await?;
        } else {
            self.hypersync_client.unsubscribe_blocks();
        }

        Ok(())
    }

    fn get_dex(&self, dex_id: &str) -> anyhow::Result<&DexExtended> {
        match self.cache.get_dex(dex_id) {
            Some(dex) => Ok(dex),
            None => anyhow::bail!("Dex {dex_id} is not registered"),
        }
    }

    fn get_pool(&self, pool_address: &str) -> anyhow::Result<&SharedPool> {
        let pool_address = validate_address(pool_address)?;
        match self.cache.get_pool(&pool_address) {
            Some(pool) => Ok(pool),
            None => anyhow::bail!("Pool {pool_address} is not registered"),
        }
    }

    fn send_block(&self, block: Block) {
        let data = DataEvent::DeFi(DefiData::Block(block));
        self.send_data(data);
    }

    fn send_swap(&self, swap: PoolSwap) {
        let data = DataEvent::DeFi(DefiData::PoolSwap(swap));
        self.send_data(data);
    }

    fn send_liquidity_update(&self, update: PoolLiquidityUpdate) {
        let data = DataEvent::DeFi(DefiData::PoolLiquidityUpdate(update));
        self.send_data(data);
    }

    fn send_data(&self, data: DataEvent) {
        tracing::debug!("Sending {data}");

        if let Err(e) = self.data_sender.send(data) {
            tracing::error!("Failed to send data: {e}");
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

    fn start(&mut self) -> anyhow::Result<()> {
        tracing::info!("Starting blockchain data client for '{}'", self.chain.name);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!("Stopping blockchain data client for '{}'", self.chain.name);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::info!("Resetting blockchain data client for '{}'", self.chain.name);
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        tracing::info!("Disposing blockchain data client for '{}'", self.chain.name);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Connecting blockchain data client for '{}'",
            self.chain.name
        );

        if let Some(ref mut rpc_client) = self.rpc_client {
            rpc_client.connect().await?;
        }

        let from_block = self.config.from_block.unwrap_or(0);
        self.cache.connect(from_block).await?;
        self.sync_blocks(self.config.from_block).await?;
        // self.subscribe_blocks().await?;

        if self.process_task.is_none() {
            self.spawn_process_task();
        }

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Disconnecting blockchain data client for '{}'",
            self.chain.name
        );

        if let Some(handle) = self.process_task.take() {
            tracing::debug!("Aborting task 'process'");
            handle.abort();
        }

        Ok(())
    }

    fn is_connected(&self) -> bool {
        // TODO: Improve connection detection
        // For now, we'll assume connected if we have either RPC or HyperSync configured
        self.rpc_client.is_some() || true // HyperSync is always available
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn subscribe_blocks(&mut self, cmd: &SubscribeBlocks) -> anyhow::Result<()> {
        let command = DefiDataCommand::Subscribe(DefiSubscribeCommand::Blocks(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn subscribe_pool(&mut self, cmd: &SubscribePool) -> anyhow::Result<()> {
        let command = DefiDataCommand::Subscribe(DefiSubscribeCommand::Pool(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn subscribe_pool_swaps(&mut self, cmd: &SubscribePoolSwaps) -> anyhow::Result<()> {
        let command = DefiDataCommand::Subscribe(DefiSubscribeCommand::PoolSwaps(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn subscribe_pool_liquidity_updates(
        &mut self,
        cmd: &SubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        let command =
            DefiDataCommand::Subscribe(DefiSubscribeCommand::PoolLiquidityUpdates(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn unsubscribe_blocks(&mut self, cmd: &UnsubscribeBlocks) -> anyhow::Result<()> {
        let command = DefiDataCommand::Unsubscribe(DefiUnsubscribeCommand::Blocks(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn unsubscribe_pool(&mut self, cmd: &UnsubscribePool) -> anyhow::Result<()> {
        let command = DefiDataCommand::Unsubscribe(DefiUnsubscribeCommand::Pool(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn unsubscribe_pool_swaps(&mut self, cmd: &UnsubscribePoolSwaps) -> anyhow::Result<()> {
        let command = DefiDataCommand::Unsubscribe(DefiUnsubscribeCommand::PoolSwaps(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn unsubscribe_pool_liquidity_updates(
        &mut self,
        cmd: &UnsubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        let command =
            DefiDataCommand::Unsubscribe(DefiUnsubscribeCommand::PoolLiquidityUpdates(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }
}
