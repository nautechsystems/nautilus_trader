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

use std::{cmp::max, collections::HashSet, sync::Arc};

use alloy::primitives::{Address, U256};
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
use nautilus_core::UnixNanos;
use nautilus_data::client::DataClient;
use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::{
    defi::{
        Block, Blockchain, DefiData, Pool, PoolLiquidityUpdate, PoolLiquidityUpdateType, PoolSwap,
        SharedChain, SharedDex, SharedPool, Token,
    },
    identifiers::{ClientId, Venue},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    cache::BlockchainCache,
    config::BlockchainDataClientConfig,
    contracts::erc20::{Erc20Contract, TokenInfoError},
    decode::u256_to_quantity,
    events::pool_created::PoolCreatedEvent,
    exchanges::{dex_extended_map, extended::DexExtended},
    hypersync::{client::HyperSyncClient, helpers::extract_block_number},
    reporting::{BlockchainItem, BlockchainSyncReporter},
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

const BLOCKS_PROCESS_IN_SYNC_REPORT: u64 = 50000;

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
    pub async fn initialize_cache_database(&mut self, pg_connect_options: PostgresConnectOptions) {
        tracing::info!(
            "Initializing blockchain cache on database '{}'",
            pg_connect_options.database
        );
        self.cache
            .initialize_database(pg_connect_options.clone().into())
            .await;
    }

    /// Spawns a unified task that handles both commands and data from the same client instances.
    /// This replaces both the command processor and hypersync forwarder with a single unified handler.
    fn spawn_process_task(&mut self) {
        let command_rx = if let Some(r) = self.command_rx.take() {
            r
        } else {
            tracing::error!("Command receiver already taken, not spawning handler");
            return;
        };

        let hypersync_rx = if let Some(r) = self.hypersync_rx.take() {
            r
        } else {
            tracing::error!("HyperSync receiver already taken, not spawning handler");
            return;
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
                        if let Some(cmd) = command {
                            if let Err(e) = Self::process_command(
                                cmd,
                                &mut hypersync_client,
                                rpc_client.as_mut()
                            ).await {
                                tracing::error!("Error processing command: {e}");
                            }
                        } else {
                            tracing::debug!("Command channel closed");
                            break;
                        }
                    }
                    data = hypersync_rx.recv() => {
                        if let Some(msg) = data {
                            let data_event = match msg {
                                BlockchainMessage::Block(block) => {
                                    DataEvent::DeFi(DefiData::Block(block))
                                }
                                BlockchainMessage::Swap(swap) => {
                                    DataEvent::DeFi(DefiData::PoolSwap(swap))
                                }
                                BlockchainMessage::LiquidityUpdate(update) => {
                                    DataEvent::DeFi(DefiData::PoolLiquidityUpdate(update))
                                }
                            };

                            if let Err(e) = data_sender.send(data_event) {
                                tracing::error!("Failed to send data event: {e}");
                                break;
                            }
                        } else {
                            tracing::debug!("HyperSync data channel closed");
                            break;
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
            DefiSubscribeCommand::PoolSwaps(cmd) => {
                tracing::info!(
                    "Processing subscribe pool swaps command for {}",
                    cmd.instrument_id
                );

                if let Some(ref mut _rpc) = rpc_client {
                    tracing::warn!(
                        "RPC pool swaps subscription not yet implemented, using HyperSync"
                    );
                }

                // TODO: Implement pool swaps subscription logic
                tracing::error!(
                    "Implement pool swap subscription logic for {}",
                    cmd.instrument_id
                );

                Ok(())
            }
            DefiSubscribeCommand::PoolLiquidityUpdates(cmd) => {
                tracing::info!(
                    "Processing subscribe pool liquidity updates command for address: {}",
                    cmd.instrument_id
                );

                if let Some(ref mut _rpc) = rpc_client {
                    tracing::warn!(
                        "RPC pool liquidity updates subscription not yet implemented, using HyperSync"
                    );
                }

                // TODO: Implement pool liquidity updates subscription logic
                tracing::error!(
                    "Implement pool liquidity updates subscription logic for {}",
                    cmd.instrument_id
                );

                Ok(())
            }
        }
    }

    /// Handles DeFi unsubscribe commands with access to mutable client instances.
    async fn handle_unsubscribe_command(
        command: DefiUnsubscribeCommand,
        hypersync_client: &mut HyperSyncClient,
        mut rpc_client: Option<&mut BlockchainRpcClientAny>,
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
            DefiUnsubscribeCommand::PoolLiquidityUpdates(cmd) => {
                tracing::info!(
                    "Processing unsubscribe pool liquidity updates command for {}",
                    cmd.instrument_id
                );

                if let Some(ref mut _rpc) = rpc_client {
                    tracing::warn!(
                        "RPC pool liquidity updates unsubscription not yet implemented, using HyperSync"
                    );
                }

                match hypersync_client.get_pool_address(cmd.instrument_id) {
                    Some(address) => {
                        hypersync_client.unsubscribe_pool_liquidity_updates(*address);
                        tracing::info!(
                            "Unsubscribed to pool liquidity updates for {}",
                            cmd.instrument_id
                        );
                    }
                    None => {
                        tracing::error!("Failed to fetch pool address for {}", cmd.instrument_id);
                    }
                }

                Ok(())
            }
        }
    }

    /// Synchronizes blockchain data by fetching and caching all blocks from the starting block to the current chain head.
    ///
    /// # Errors
    ///
    /// Returns an error if block streaming or database operations fail.
    pub async fn sync_blocks(
        &mut self,
        from_block: Option<u64>,
        to_block: Option<u64>,
    ) -> anyhow::Result<()> {
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

        let to_block = if let Some(block) = to_block {
            block
        } else {
            self.hypersync_client.current_block().await
        };
        let total_blocks = to_block.saturating_sub(from_block) + 1;
        tracing::info!(
            "Syncing blocks from {from_block} to {to_block} (total: {total_blocks} blocks)"
        );

        let blocks_stream = self
            .hypersync_client
            .request_blocks_stream(from_block, Some(to_block))
            .await;

        tokio::pin!(blocks_stream);

        let mut metrics = BlockchainSyncReporter::new(
            BlockchainItem::Blocks,
            from_block,
            total_blocks,
            BLOCKS_PROCESS_IN_SYNC_REPORT,
        );

        // Batch configuration
        const BATCH_SIZE: usize = 1000;
        let mut batch: Vec<Block> = Vec::with_capacity(BATCH_SIZE);

        while let Some(block) = blocks_stream.next().await {
            let block_number = block.number;
            if self.cache.get_block_timestamp(block_number).is_some() {
                continue;
            }
            batch.push(block);

            // Process batch when full or last block
            if batch.len() >= BATCH_SIZE || block_number >= to_block {
                let batch_size = batch.len();

                self.cache.add_blocks_batch(batch).await?;
                metrics.update(batch_size);

                // Re-initialize batch vector
                batch = Vec::with_capacity(BATCH_SIZE);
            }

            // Log progress if needed
            if metrics.should_log_progress(block_number, to_block) {
                metrics.log_progress(block_number);
            }
        }

        // Process any remaining blocks
        if !batch.is_empty() {
            let batch_size = batch.len();
            self.cache.add_blocks_batch(batch).await?;
            metrics.update(batch_size);
        }

        metrics.log_final_stats();
        Ok(())
    }

    /// Fetches and caches all swap events for a specific liquidity pool within the given block range.
    ///
    /// # Errors
    ///
    /// Returns an error if DEX lookup, event streaming, or database operations fail.
    ///
    /// # Panics
    ///
    /// Panics if swap event conversion to trade data fails.
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
            pool.instrument_id,
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
                pool.instrument_id,
                pool.address,
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
            pool.instrument_id,
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
                pool.instrument_id,
                pool.address,
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
            pool.instrument_id,
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
                pool.instrument_id,
                pool.address,
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
        let to_block = if let Some(block) = to_block {
            block
        } else {
            self.hypersync_client.current_block().await
        };
        let total_blocks = to_block.saturating_sub(from_block) + 1;
        tracing::info!(
            "Syncing DEX exchange pools from {from_block} to {to_block} (total: {total_blocks} blocks)"
        );

        let mut metrics = BlockchainSyncReporter::new(
            BlockchainItem::PoolCreatedEvents,
            from_block,
            total_blocks,
            BLOCKS_PROCESS_IN_SYNC_REPORT,
        );

        let dex = self.get_dex(dex_id)?.clone();
        let factory_address = dex.factory.as_ref();
        let pair_created_event_signature = dex.pool_created_event.as_ref();
        let pools_stream = self
            .hypersync_client
            .request_contract_events_stream(
                from_block,
                Some(to_block),
                factory_address,
                pair_created_event_signature,
                Vec::new(),
            )
            .await;

        tokio::pin!(pools_stream);

        const TOKEN_BATCH_SIZE: usize = 100;
        let mut token_buffer: HashSet<Address> = HashSet::new();
        let mut pool_buffer: Vec<PoolCreatedEvent> = Vec::new();
        let mut last_block_saved = from_block;
        let mut blocks_processed = 0;

        while let Some(log) = pools_stream.next().await {
            let block_number = extract_block_number(&log)?;
            blocks_processed += block_number - last_block_saved;
            last_block_saved = block_number;

            let pool = dex.parse_pool_created_event(log)?;
            if self.cache.get_pool(&pool.pool_address).is_some() {
                // Pool is already initialized and cached.
                continue;
            }

            // If we have both tokens cached, we can process the pool immediately.
            if self.cache.get_token(&pool.token0).is_some()
                && self.cache.get_token(&pool.token1).is_some()
            {
                self.process_pool(dex.dex.clone(), &pool).await?;
                continue;
            }

            if self.cache.get_token(&pool.token0).is_none() {
                token_buffer.insert(pool.token0);
            }
            if self.cache.get_token(&pool.token1).is_none() {
                token_buffer.insert(pool.token1);
            }
            // Buffer the pool for later processing
            pool_buffer.push(pool);

            if token_buffer.len() >= TOKEN_BATCH_SIZE {
                self.flush_tokens_and_process_pools(
                    &mut token_buffer,
                    &mut pool_buffer,
                    dex.dex.clone(),
                )
                .await?;
                metrics.update(blocks_processed as usize);
                blocks_processed = 0;

                // Log progress if needed
                if metrics.should_log_progress(block_number, to_block) {
                    metrics.log_progress(block_number);
                }
            }
        }

        if !token_buffer.is_empty() || !pool_buffer.is_empty() {
            self.flush_tokens_and_process_pools(
                &mut token_buffer,
                &mut pool_buffer,
                dex.dex.clone(),
            )
            .await?;
            blocks_processed += (to_block) - last_block_saved;
            metrics.update(blocks_processed as usize);
        }

        metrics.log_final_stats();
        Ok(())
    }

    /// Helper method to flush token buffer and process pools.
    async fn flush_tokens_and_process_pools(
        &mut self,
        token_buffer: &mut HashSet<Address>,
        pool_buffer: &mut Vec<PoolCreatedEvent>,
        dex: SharedDex,
    ) -> anyhow::Result<()> {
        if token_buffer.is_empty() {
            return Ok(());
        }

        let batch_addresses: Vec<Address> = token_buffer.drain().collect();
        let token_infos = self.tokens.batch_fetch_token_info(&batch_addresses).await?;

        let mut empty_tokens = HashSet::new();
        for (token_address, token_info) in token_infos {
            match token_info {
                Ok(token) => {
                    let token = Token::new(
                        self.chain.clone(),
                        token_address,
                        token.name,
                        token.symbol,
                        token.decimals,
                    );
                    self.cache.add_token(token).await?;
                }
                Err(token_info_error) => match token_info_error {
                    TokenInfoError::EmptyTokenField { .. } => {
                        // Empty token name/symbol indicates non-standard implementations:
                        // - Non-conforming ERC20 tokens (name/symbol are optional in the standard)
                        // - Minimal proxy contracts without proper metadata forwarding
                        // - Malicious or deprecated tokens
                        // We skip these pools as they're not suitable for trading.
                        empty_tokens.insert(token_address);
                    }
                    _ => {
                        tracing::error!(
                            "Error fetching token info: {}",
                            token_info_error.to_string()
                        );
                    }
                },
            }
        }

        for pool in &mut *pool_buffer {
            // We skip the pool that contains one of the tokens that is flagged as empty
            if empty_tokens.contains(&pool.token0) || empty_tokens.contains(&pool.token1) {
                continue;
            }

            if let Err(e) = self.process_pool(dex.clone(), pool).await {
                tracing::error!("Failed to process {} with error {}", pool.pool_address, e);
            }
        }
        pool_buffer.clear();

        Ok(())
    }

    /// Processes a pool creation event by creating and caching a `Pool` entity.
    async fn process_pool(
        &mut self,
        dex: SharedDex,
        event: &PoolCreatedEvent,
    ) -> anyhow::Result<()> {
        let token0 = match self.cache.get_token(&event.token0) {
            Some(token) => token.clone(),
            None => {
                anyhow::bail!("Token {} should be initialized in the cache", event.token0);
            }
        };
        let token1 = match self.cache.get_token(&event.token1) {
            Some(token) => token.clone(),
            None => {
                anyhow::bail!("Token {} should be initialized in the cache", event.token1);
            }
        };

        let pool = Pool::new(
            self.chain.clone(),
            dex,
            event.pool_address,
            event.block_number,
            token0,
            token1,
            event.fee,
            event.tick_spacing,
            UnixNanos::default(), // TODO: Use default timestamp for now
        );
        self.cache.add_pool(pool.clone()).await?;

        Ok(())
    }

    /// Registers a decentralized exchange with the client.
    pub async fn register_dex_exchange(&mut self, dex_id: &str) -> anyhow::Result<()> {
        if let Some(dex) = dex_extended_map().get(dex_id) {
            tracing::info!("Registering blockchain exchange {dex_id}");
            self.cache
                .add_dex(dex_id.to_string(), dex.to_owned().clone())
                .await?;
            self.cache.load_pools(dex_id).await?;
            Ok(())
        } else {
            anyhow::bail!("Unknown DEX ID: {dex_id}")
        }
    }

    /// Processes incoming messages from the HyperSync client.
    pub async fn process_hypersync_messages(&mut self) {
        tracing::info!("Starting task 'process_hypersync_messages'");

        let mut rx = if let Some(r) = self.hypersync_rx.take() {
            r
        } else {
            tracing::warn!("HyperSync receiver already taken, not spawning forwarder");
            return;
        };

        while let Some(msg) = rx.recv().await {
            match msg {
                BlockchainMessage::Block(block) => {
                    self.send_block(block);
                }
                BlockchainMessage::Swap(swap) => {
                    self.send_swap(swap);
                }
                BlockchainMessage::LiquidityUpdate(update) => {
                    self.send_liquidity_update(update);
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
                BlockchainMessage::LiquidityUpdate(update) => self.send_liquidity_update(update),
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

    /// Determines the starting block for syncing operations.
    ///
    /// # Returns
    /// - The configured `from_block` if provided
    /// - Otherwise, the earliest DEX factory deployment block from the cache
    /// - If no DEXes are registered, defaults to block 0 (genesis)
    fn determine_from_block(&self) -> u64 {
        self.config
            .from_block
            .unwrap_or_else(|| self.cache.min_dex_creation_block().unwrap_or(0))
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
        tracing::info!(
            "Starting blockchain data client for '{chain_name}'",
            chain_name = self.chain.name
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Stopping blockchain data client for '{chain_name}'",
            chain_name = self.chain.name
        );
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Resetting blockchain data client for '{chain_name}'",
            chain_name = self.chain.name
        );
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Disposing blockchain data client for '{chain_name}'",
            chain_name = self.chain.name
        );
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Connecting blockchain data client for '{}'",
            self.chain.name
        );

        if let Some(pg_connect_options) = self.config.postgres_cache_database_config.clone() {
            self.initialize_cache_database(pg_connect_options).await;
        }

        if let Some(ref mut rpc_client) = self.rpc_client {
            rpc_client.connect().await?;
        }

        let from_block = self.determine_from_block();
        // Initialize the chain and register the Dex exchanges in the cache.
        self.cache.initialize_chain().await;
        // Import the cached blockchain data.
        self.cache.connect(from_block).await?;
        // TODO disable block syncing for now as we don't have timestamps yet configured
        // Sync the remaining blocks which are missing.
        // self.sync_blocks(Some(from_block), None).await?;
        for dex_id in self.config.dex_ids.clone() {
            self.register_dex_exchange(&dex_id).await?;
            self.sync_exchange_pools(&dex_id, Some(from_block), None)
                .await?;
        }

        tracing::info!(
            "Connecting to blockchain data source for '{chain_name}' from block {from_block}",
            chain_name = self.chain.name
        );

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
