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
use nautilus_common::messages::DataEvent;
use nautilus_core::UnixNanos;
use nautilus_model::defi::{
    Block, Blockchain, DexType, Pool, PoolLiquidityUpdate, PoolSwap, SharedChain, SharedDex,
    SharedPool, Token, data::PoolFeeCollect,
};

use crate::{
    cache::BlockchainCache,
    config::BlockchainDataClientConfig,
    contracts::erc20::{Erc20Contract, TokenInfoError},
    data::subscription::DefiDataSubscriptionManager,
    decode::u256_to_quantity,
    events::{
        burn::BurnEvent, collect::CollectEvent, mint::MintEvent, pool_created::PoolCreatedEvent,
        swap::SwapEvent,
    },
    exchanges::{extended::DexExtended, get_dex_extended},
    hypersync::{
        client::HyperSyncClient,
        helpers::{extract_block_number, extract_event_signature_bytes},
    },
    reporting::{BlockchainSyncReportItems, BlockchainSyncReporter},
    rpc::{
        BlockchainRpcClient, BlockchainRpcClientAny,
        chains::{
            arbitrum::ArbitrumRpcClient, base::BaseRpcClient, ethereum::EthereumRpcClient,
            polygon::PolygonRpcClient,
        },
        http::BlockchainHttpRpcClient,
        types::BlockchainMessage,
    },
};

const BLOCKS_PROCESS_IN_SYNC_REPORT: u64 = 50000;

/// Core blockchain data client responsible for fetching, processing, and caching blockchain data.
///
/// This struct encapsulates the core functionality for interacting with blockchain networks,
/// including syncing historical data, processing real-time events, and managing cached entities.
#[derive(Debug)]
pub struct BlockchainDataClientCore {
    /// The blockchain being targeted by this client instance.
    pub chain: SharedChain,
    /// The configuration for the data client.
    pub config: BlockchainDataClientConfig,
    /// Local cache for blockchain entities.
    pub cache: BlockchainCache,
    /// Interface for interacting with ERC20 token contracts.
    tokens: Erc20Contract,
    /// Client for the HyperSync data indexing service.
    pub hypersync_client: HyperSyncClient,
    /// Optional WebSocket RPC client for direct blockchain node communication.
    pub rpc_client: Option<BlockchainRpcClientAny>,
    /// Manages subscriptions for various DEX events (swaps, mints, burns).
    pub subscription_manager: DefiDataSubscriptionManager,
    /// Channel sender for data events.
    data_tx: Option<tokio::sync::mpsc::UnboundedSender<DataEvent>>,
}

impl BlockchainDataClientCore {
    /// Creates a new instance of [`BlockchainDataClientCore`].
    ///
    /// # Panics
    ///
    /// Panics if `use_hypersync_for_live_data` is false but `wss_rpc_url` is None.
    #[must_use]
    pub fn new(
        config: BlockchainDataClientConfig,
        hypersync_tx: Option<tokio::sync::mpsc::UnboundedSender<BlockchainMessage>>,
        data_tx: Option<tokio::sync::mpsc::UnboundedSender<DataEvent>>,
    ) -> Self {
        let chain = config.chain.clone();
        let cache = BlockchainCache::new(chain.clone());
        let rpc_client = if !config.use_hypersync_for_live_data && config.wss_rpc_url.is_some() {
            let wss_rpc_url = config.wss_rpc_url.clone().expect("wss_rpc_url is required");
            Some(Self::initialize_rpc_client(chain.name, wss_rpc_url))
        } else {
            None
        };
        let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            config.http_rpc_url.clone(),
            config.rpc_requests_per_second,
        ));
        let erc20_contract = Erc20Contract::new(
            http_rpc_client,
            config.pool_filters.remove_pools_with_empty_erc20fields,
        );

        let hypersync_client = HyperSyncClient::new(chain.clone(), hypersync_tx);
        Self {
            chain,
            config,
            rpc_client,
            tokens: erc20_contract,
            cache,
            hypersync_client,
            subscription_manager: DefiDataSubscriptionManager::new(),
            data_tx,
        }
    }

    /// Initializes the database connection for the blockchain cache.
    pub async fn initialize_cache_database(&mut self) {
        if let Some(pg_connect_options) = &self.config.postgres_cache_database_config {
            tracing::info!(
                "Initializing blockchain cache on database '{}'",
                pg_connect_options.database
            );
            self.cache
                .initialize_database(pg_connect_options.clone().into())
                .await;
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

    /// Establishes connections to all configured data sources and initializes the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if cache initialization or connection setup fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Connecting blockchain data client for '{}'",
            self.chain.name
        );
        self.initialize_cache_database().await;

        if let Some(ref mut rpc_client) = self.rpc_client {
            rpc_client.connect().await?;
        }

        let from_block = self.determine_from_block();

        tracing::info!(
            "Connecting to blockchain data source for '{chain_name}' from block {from_block}",
            chain_name = self.chain.name
        );

        // Initialize the chain and register the Dex exchanges in the cache.
        self.cache.initialize_chain().await;
        // Import the cached blockchain data.
        self.cache.connect(from_block).await?;
        // TODO disable block syncing for now as we don't have timestamps yet configured
        // Sync the remaining blocks which are missing.
        // self.sync_blocks(Some(from_block), None).await?;
        for dex in self.config.dex_ids.clone() {
            self.register_dex_exchange(dex).await?;
            self.sync_exchange_pools(&dex, from_block, None, false)
                .await?;
        }

        Ok(())
    }

    /// Syncs blocks with consistency checks to ensure data integrity.
    ///
    /// # Errors
    ///
    /// Returns an error if block syncing fails or if consistency checks fail.
    pub async fn sync_blocks_checked(
        &mut self,
        from_block: u64,
        to_block: Option<u64>,
    ) -> anyhow::Result<()> {
        if let Some(blocks_status) = self.cache.get_cache_block_consistency_status().await {
            // If blocks are consistent proceed with copy command.
            if blocks_status.is_consistent() {
                tracing::info!(
                    "Cache is consistent: no gaps detected (last continuous block: {})",
                    blocks_status.last_continuous_block
                );
                let target_block = max(blocks_status.max_block + 1, from_block);
                tracing::info!("Starting fast sync with COPY from block {}", target_block);
                self.sync_blocks(target_block, to_block, true).await?;
            } else {
                let gap_size = blocks_status.max_block - blocks_status.last_continuous_block;
                tracing::info!(
                    "Cache inconsistency detected: {} blocks missing between {} and {}",
                    gap_size,
                    blocks_status.last_continuous_block + 1,
                    blocks_status.max_block
                );

                tracing::info!(
                    "Block syncing Phase 1: Filling gaps with INSERT (blocks {} to {})",
                    blocks_status.last_continuous_block + 1,
                    blocks_status.max_block
                );
                self.sync_blocks(
                    blocks_status.last_continuous_block + 1,
                    Some(blocks_status.max_block),
                    false,
                )
                .await?;

                tracing::info!(
                    "Block syncing Phase 2: Continuing with fast COPY from block {}",
                    blocks_status.max_block + 1
                );
                self.sync_blocks(blocks_status.max_block + 1, to_block, true)
                    .await?;
            }
        } else {
            self.sync_blocks(from_block, to_block, true).await?;
        }

        Ok(())
    }

    /// Synchronizes blockchain data by fetching and caching all blocks from the starting block to the current chain head.
    ///
    /// # Errors
    ///
    /// Returns an error if block fetching, caching, or database operations fail.
    pub async fn sync_blocks(
        &mut self,
        from_block: u64,
        to_block: Option<u64>,
        use_copy_command: bool,
    ) -> anyhow::Result<()> {
        let to_block = if let Some(block) = to_block {
            block
        } else {
            self.hypersync_client.current_block().await
        };
        let total_blocks = to_block.saturating_sub(from_block) + 1;
        tracing::info!(
            "Syncing blocks from {from_block} to {to_block} (total: {total_blocks} blocks)"
        );

        // Enable performance settings for sync operations
        if let Err(e) = self.cache.toggle_performance_settings(true).await {
            tracing::warn!("Failed to enable performance settings: {e}");
        }

        let blocks_stream = self
            .hypersync_client
            .request_blocks_stream(from_block, Some(to_block))
            .await;

        tokio::pin!(blocks_stream);

        let mut metrics = BlockchainSyncReporter::new(
            BlockchainSyncReportItems::Blocks,
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

                self.cache.add_blocks_batch(batch, use_copy_command).await?;
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
            self.cache.add_blocks_batch(batch, use_copy_command).await?;
            metrics.update(batch_size);
        }

        metrics.log_final_stats();

        // Restore default safe settings after sync completion
        if let Err(e) = self.cache.toggle_performance_settings(false).await {
            tracing::warn!("Failed to restore default settings: {e}");
        }

        Ok(())
    }

    /// Synchronizes all events for a specific pool within the given block range.
    ///
    /// # Errors
    ///
    /// Returns an error if event syncing, parsing, or database operations fail.
    pub async fn sync_pool_events(
        &mut self,
        dex: &DexType,
        pool_address: Address,
        from_block: Option<u64>,
        to_block: Option<u64>,
        reset: bool,
    ) -> anyhow::Result<()> {
        let pool: SharedPool = self.get_pool(&pool_address)?.clone();
        let pool_display = pool.to_full_spec_string();
        let from_block = from_block.unwrap_or(pool.creation_block);

        let (last_synced_block, effective_from_block) = if reset {
            (None, from_block)
        } else {
            let last_synced_block = self
                .cache
                .get_pool_last_synced_block(dex, &pool_address)
                .await?;
            let effective_from_block = last_synced_block
                .map_or(from_block, |last_synced| max(from_block, last_synced + 1));
            (last_synced_block, effective_from_block)
        };

        let to_block = match to_block {
            Some(block) => block,
            None => self.hypersync_client.current_block().await,
        };

        // Skip sync if we're already up to date
        if effective_from_block > to_block {
            tracing::info!(
                "D {} already synced to block {} (current: {}), skipping sync",
                dex,
                last_synced_block.unwrap_or(0),
                to_block
            );
            return Ok(());
        }

        // Query table max blocks to detect last blocks to use batch insert before that, then COPY command.
        let last_block_across_pool_events_table = self
            .cache
            .get_pool_event_tables_last_block(&pool_address)
            .await?;

        let total_blocks = to_block.saturating_sub(effective_from_block) + 1;
        tracing::info!(
            "Syncing Pool: '{}' events from {} to {} (total: {} blocks){}",
            pool_display,
            effective_from_block,
            to_block,
            total_blocks,
            if let Some(last_synced) = last_synced_block {
                format!(" - resuming from last synced block {}", last_synced)
            } else {
                String::new()
            }
        );

        let mut metrics = BlockchainSyncReporter::new(
            BlockchainSyncReportItems::PoolEvents,
            effective_from_block,
            total_blocks,
            BLOCKS_PROCESS_IN_SYNC_REPORT,
        );
        let dex_extended = self.get_dex_extended(dex)?.clone();
        let swap_event_signature = dex_extended.swap_created_event.as_ref();
        let mint_event_signature = dex_extended.mint_created_event.as_ref();
        let burn_event_signature = dex_extended.burn_created_event.as_ref();
        let collect_event_signature = dex_extended.collect_created_event.as_ref();
        let initialize_event_signature: Option<&str> =
            dex_extended.initialize_event.as_ref().map(|s| s.as_ref());

        // Pre-decode event signatures to bytes for efficient comparison
        let swap_sig_bytes = hex::decode(
            swap_event_signature
                .strip_prefix("0x")
                .unwrap_or(swap_event_signature),
        )?;
        let mint_sig_bytes = hex::decode(
            mint_event_signature
                .strip_prefix("0x")
                .unwrap_or(mint_event_signature),
        )?;
        let burn_sig_bytes = hex::decode(
            burn_event_signature
                .strip_prefix("0x")
                .unwrap_or(burn_event_signature),
        )?;
        let collect_sig_bytes = hex::decode(
            collect_event_signature
                .strip_prefix("0x")
                .unwrap_or(collect_event_signature),
        )?;
        let initialize_sig_bytes = initialize_event_signature
            .map(|s| hex::decode(s.strip_prefix("0x").unwrap_or(s)).unwrap_or_default());

        let mut event_signatures = vec![
            swap_event_signature,
            mint_event_signature,
            burn_event_signature,
            collect_event_signature,
        ];
        if let Some(event) = dex_extended.initialize_event.as_ref() {
            event_signatures.push(event);
        }
        let pool_events_stream = self
            .hypersync_client
            .request_contract_events_stream(
                effective_from_block,
                Some(to_block),
                &pool_address,
                event_signatures,
            )
            .await;
        tokio::pin!(pool_events_stream);

        let mut last_block_saved = effective_from_block;
        let mut blocks_processed = 0;

        // Batch configuration for events
        const EVENT_BATCH_SIZE: usize = 20000;
        let mut swap_batch: Vec<PoolSwap> = Vec::with_capacity(EVENT_BATCH_SIZE);
        let mut liquidity_batch: Vec<PoolLiquidityUpdate> = Vec::with_capacity(EVENT_BATCH_SIZE);
        let mut collect_batch: Vec<PoolFeeCollect> = Vec::with_capacity(EVENT_BATCH_SIZE);

        // Track when we've moved beyond stale data and can use COPY
        let mut beyond_stale_data = last_block_across_pool_events_table
            .map_or(true, |tables_max| effective_from_block > tables_max);
        while let Some(log) = pool_events_stream.next().await {
            let block_number = extract_block_number(&log)?;
            blocks_processed += block_number - last_block_saved;
            last_block_saved = block_number;

            let event_sig_bytes = extract_event_signature_bytes(&log)?;
            if event_sig_bytes == swap_sig_bytes.as_slice() {
                let swap_event = dex_extended.parse_swap_event(log)?;
                match self.process_pool_swap_event(&swap_event, &pool, &dex_extended) {
                    Ok(swap) => swap_batch.push(swap),
                    Err(e) => tracing::error!("Failed to process swap event: {e}"),
                }
            } else if event_sig_bytes == mint_sig_bytes.as_slice() {
                let mint_event = dex_extended.parse_mint_event(log)?;
                match self.process_pool_mint_event(&mint_event, &pool, &dex_extended) {
                    Ok(liquidity_update) => liquidity_batch.push(liquidity_update),
                    Err(e) => tracing::error!("Failed to process mint event: {e}"),
                }
            } else if event_sig_bytes == burn_sig_bytes.as_slice() {
                let burn_event = dex_extended.parse_burn_event(log)?;
                match self.process_pool_burn_event(&burn_event, &pool, &dex_extended) {
                    Ok(liquidity_update) => liquidity_batch.push(liquidity_update),
                    Err(e) => tracing::error!("Failed to process burn event: {e}"),
                }
            } else if event_sig_bytes == collect_sig_bytes.as_slice() {
                let collect_event = dex_extended.parse_collect_event(log)?;
                match self.process_pool_collect_event(&collect_event, &pool, &dex_extended) {
                    Ok(fee_collect) => collect_batch.push(fee_collect),
                    Err(e) => tracing::error!("Failed to process collect event: {e}"),
                }
            } else if let Some(init_sig_bytes) = &initialize_sig_bytes {
                if event_sig_bytes == init_sig_bytes.as_slice() {
                    let initialize_event = dex_extended.parse_initialize_event(log)?;
                    self.cache
                        .update_pool_initialize_price_tick(&initialize_event)
                        .await?;
                }
            } else {
                let event_signature = hex::encode(event_sig_bytes);
                tracing::error!(
                    "Unexpected event signature: {} for log {:?}",
                    event_signature,
                    log
                );
            }

            // Check if we've moved beyond stale data (transition point for strategy change)
            if !beyond_stale_data
                && last_block_across_pool_events_table
                    .map_or(false, |table_max| block_number > table_max)
            {
                tracing::info!(
                    "Crossed beyond stale data at block {} - flushing current batches with ON CONFLICT, then switching to COPY",
                    block_number
                );

                // Flush all batches with ON CONFLICT to handle any remaining duplicates
                self.flush_event_batches(
                    EVENT_BATCH_SIZE,
                    &mut swap_batch,
                    &mut liquidity_batch,
                    &mut collect_batch,
                    false,
                    true,
                )
                .await?;

                beyond_stale_data = true;
                tracing::info!("Switched to COPY mode - future batches will use COPY command");
            } else {
                // Process batches when they reach batch size
                self.flush_event_batches(
                    EVENT_BATCH_SIZE,
                    &mut swap_batch,
                    &mut liquidity_batch,
                    &mut collect_batch,
                    beyond_stale_data,
                    false,
                )
                .await?;
            }

            metrics.update(blocks_processed as usize);
            blocks_processed = 0;

            // Log progress if needed
            if metrics.should_log_progress(block_number, to_block) {
                metrics.log_progress(block_number);
                self.cache
                    .update_pool_last_synced_block(dex, &pool_address, block_number)
                    .await?;
            }
        }

        self.flush_event_batches(
            EVENT_BATCH_SIZE,
            &mut swap_batch,
            &mut liquidity_batch,
            &mut collect_batch,
            beyond_stale_data,
            true,
        )
        .await?;

        metrics.log_final_stats();
        self.cache
            .update_pool_last_synced_block(dex, &pool_address, to_block)
            .await?;

        tracing::info!(
            "Successfully synced Dex '{}' Pool '{}' up to block {}",
            dex,
            pool_display,
            to_block
        );
        Ok(())
    }

    async fn flush_event_batches(
        &mut self,
        event_batch_size: usize,
        swap_batch: &mut Vec<PoolSwap>,
        liquidity_batch: &mut Vec<PoolLiquidityUpdate>,
        collect_batch: &mut Vec<PoolFeeCollect>,
        use_copy_command: bool,
        force_flush_all: bool,
    ) -> anyhow::Result<()> {
        if force_flush_all || swap_batch.len() >= event_batch_size {
            if !swap_batch.is_empty() {
                self.cache
                    .add_pool_swaps_batch(swap_batch, use_copy_command)
                    .await?;
                swap_batch.clear();
            }
        }
        if force_flush_all || liquidity_batch.len() >= event_batch_size {
            if !liquidity_batch.is_empty() {
                self.cache
                    .add_pool_liquidity_updates_batch(liquidity_batch, use_copy_command)
                    .await?;
                liquidity_batch.clear();
            }
        }
        if force_flush_all || collect_batch.len() >= event_batch_size {
            if !collect_batch.is_empty() {
                self.cache
                    .add_pool_fee_collects_batch(collect_batch, use_copy_command)
                    .await?;
                collect_batch.clear();
            }
        }
        Ok(())
    }

    /// Returns an error if swap event processing fails.
    ///
    /// # Panics
    ///
    /// Panics if swap event conversion to trade data fails.
    pub fn process_pool_swap_event(
        &self,
        swap_event: &SwapEvent,
        pool: &SharedPool,
        dex_extended: &DexExtended,
    ) -> anyhow::Result<PoolSwap> {
        let timestamp = self
            .cache
            .get_block_timestamp(swap_event.block_number)
            .copied();

        let (side, size, price) = dex_extended
            .convert_to_trade_data(&pool.token0, &pool.token1, swap_event)
            .expect("Failed to convert swap event to trade data");
        let swap = swap_event.to_pool_swap(
            self.chain.clone(),
            pool.instrument_id,
            pool.address,
            side,
            size,
            price,
            timestamp,
        );

        // TODO add caching and persisting of swaps, resolve block timestamps sync
        // self.cache.add_pool_swap(&swap).await?;

        Ok(swap)
    }

    /// Processes a mint event (liquidity addition) and converts it to a `PoolLiquidityUpdate`.
    ///
    /// # Errors
    ///
    /// Returns an error if mint event processing fails or if the liquidity update creation fails.
    pub fn process_pool_mint_event(
        &self,
        mint_event: &MintEvent,
        pool: &SharedPool,
        dex_extended: &DexExtended,
    ) -> anyhow::Result<PoolLiquidityUpdate> {
        let timestamp = self
            .cache
            .get_block_timestamp(mint_event.block_number)
            .copied();
        let liquidity = u256_to_quantity(
            U256::from(mint_event.amount),
            self.chain.native_currency_decimals,
        )?;
        let amount0 = u256_to_quantity(mint_event.amount0, pool.token0.decimals)?;
        let amount1 = u256_to_quantity(mint_event.amount1, pool.token1.decimals)?;

        let liquidity_update = mint_event.to_pool_liquidity_update(
            self.chain.clone(),
            dex_extended.dex.clone(),
            pool.instrument_id,
            pool.address,
            liquidity,
            amount0,
            amount1,
            timestamp,
        );

        // self.cache.add_liquidity_update(&liquidity_update).await?;

        Ok(liquidity_update)
    }

    /// Processes a burn event (liquidity removal) and converts it to a `PoolLiquidityUpdate`.
    /// Processes a pool burn event and converts it to a liquidity update.
    ///
    /// # Errors
    ///
    /// Returns an error if the burn event processing fails or if the liquidity update creation fails.
    pub fn process_pool_burn_event(
        &self,
        burn_event: &BurnEvent,
        pool: &SharedPool,
        dex_extended: &DexExtended,
    ) -> anyhow::Result<PoolLiquidityUpdate> {
        let timestamp = self
            .cache
            .get_block_timestamp(burn_event.block_number)
            .copied();
        let liquidity = u256_to_quantity(
            U256::from(burn_event.amount),
            self.chain.native_currency_decimals,
        )?;
        let amount0 = u256_to_quantity(burn_event.amount0, pool.token0.decimals)?;
        let amount1 = u256_to_quantity(burn_event.amount1, pool.token1.decimals)?;

        let liquidity_update = burn_event.to_pool_liquidity_update(
            self.chain.clone(),
            dex_extended.dex.clone(),
            pool.instrument_id,
            pool.address,
            liquidity,
            amount0,
            amount1,
            timestamp,
        );

        // self.cache.add_liquidity_update(&liquidity_update).await?;

        Ok(liquidity_update)
    }

    /// Processes a pool collect event and converts it to a fee collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the collect event processing fails or if the fee collection creation fails.
    pub fn process_pool_collect_event(
        &self,
        collect_event: &CollectEvent,
        pool: &SharedPool,
        dex_extended: &DexExtended,
    ) -> anyhow::Result<PoolFeeCollect> {
        let timestamp = self
            .cache
            .get_block_timestamp(collect_event.block_number)
            .copied();
        let fee0 = u256_to_quantity(collect_event.amount0, pool.token0.decimals)?;
        let fee1 = u256_to_quantity(collect_event.amount1, pool.token1.decimals)?;

        let fee_collect = collect_event.to_pool_fee_collect(
            self.chain.clone(),
            dex_extended.dex.clone(),
            pool.instrument_id,
            pool.address,
            fee0,
            fee1,
            timestamp,
        );

        Ok(fee_collect)
    }

    /// Synchronizes all pools and their tokens for a specific DEX within the given block range.
    ///
    /// This method performs a comprehensive sync of:
    /// 1. Pool creation events from the DEX factory
    /// 2. Token metadata for all tokens in discovered pools
    /// 3. Pool entities with proper token associations
    ///
    /// # Errors
    ///
    /// Returns an error if syncing pools, tokens, or DEX operations fail.
    pub async fn sync_exchange_pools(
        &mut self,
        dex: &DexType,
        from_block: u64,
        to_block: Option<u64>,
        reset: bool,
    ) -> anyhow::Result<()> {
        // Check for last synced block and use it as starting point if higher (unless reset is true)
        let (last_synced_block, effective_from_block) = if reset {
            (None, from_block)
        } else {
            let last_synced_block = self.cache.get_dex_last_synced_block(dex).await?;
            let effective_from_block = last_synced_block
                .map_or(from_block, |last_synced| max(from_block, last_synced + 1));
            (last_synced_block, effective_from_block)
        };

        let to_block = match to_block {
            Some(block) => block,
            None => self.hypersync_client.current_block().await,
        };

        // Skip sync if we're already up to date
        if effective_from_block > to_block {
            tracing::info!(
                "DEX {} already synced to block {} (current: {}), skipping sync",
                dex,
                last_synced_block.unwrap_or(0),
                to_block
            );
            return Ok(());
        }

        let total_blocks = to_block.saturating_sub(effective_from_block) + 1;
        tracing::info!(
            "Syncing DEX exchange pools from {} to {} (total: {} blocks){}",
            effective_from_block,
            to_block,
            total_blocks,
            if let Some(last_synced) = last_synced_block {
                format!(" - resuming from last synced block {last_synced}")
            } else {
                String::new()
            }
        );

        let mut metrics = BlockchainSyncReporter::new(
            BlockchainSyncReportItems::PoolCreatedEvents,
            effective_from_block,
            total_blocks,
            BLOCKS_PROCESS_IN_SYNC_REPORT,
        );

        let dex = self.get_dex_extended(dex)?.clone();
        let factory_address = &dex.factory;
        let pair_created_event_signature = dex.pool_created_event.as_ref();
        let pools_stream = self
            .hypersync_client
            .request_contract_events_stream(
                effective_from_block,
                Some(to_block),
                factory_address,
                vec![pair_created_event_signature],
            )
            .await;

        tokio::pin!(pools_stream);

        const TOKEN_BATCH_SIZE: usize = 100;
        const POOL_BATCH_SIZE: usize = 1000;
        let mut token_buffer: HashSet<Address> = HashSet::new();
        let mut pool_buffer: Vec<PoolCreatedEvent> = Vec::new();
        let mut last_block_saved = effective_from_block;

        while let Some(log) = pools_stream.next().await {
            let block_number = extract_block_number(&log)?;
            let blocks_progress = block_number - last_block_saved;
            last_block_saved = block_number;

            let pool = dex.parse_pool_created_event(log)?;
            if self.cache.get_pool(&pool.pool_address).is_some() {
                // Pool is already initialized and cached.
                continue;
            }

            if self.cache.is_invalid_token(&pool.token0)
                || self.cache.is_invalid_token(&pool.token1)
            {
                // Skip pools with invalid tokens as they cannot be properly processed or traded.
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

            if token_buffer.len() >= TOKEN_BATCH_SIZE || pool_buffer.len() >= POOL_BATCH_SIZE {
                self.flush_tokens_and_process_pools(
                    &mut token_buffer,
                    &mut pool_buffer,
                    dex.dex.clone(),
                )
                .await?;
            }

            metrics.update(blocks_progress as usize);
            // Log progress if needed
            if metrics.should_log_progress(block_number, to_block) {
                metrics.log_progress(block_number);
            }
        }

        if !token_buffer.is_empty() || !pool_buffer.is_empty() {
            self.flush_tokens_and_process_pools(
                &mut token_buffer,
                &mut pool_buffer,
                dex.dex.clone(),
            )
            .await?;
        }

        metrics.log_final_stats();

        // Update the last synced block after successful completion.
        self.cache
            .update_dex_last_synced_block(&dex.dex.name, to_block)
            .await?;

        tracing::info!(
            "Successfully synced DEX {} pools up to block {}",
            dex.dex.name,
            to_block
        );

        Ok(())
    }

    /// Processes buffered tokens and their associated pools in batch.
    ///
    /// This helper method:
    /// 1. Fetches token metadata for all buffered token addresses
    /// 2. Caches valid tokens and tracks invalid ones
    /// 3. Processes pools, skipping those with invalid tokens
    async fn flush_tokens_and_process_pools(
        &mut self,
        token_buffer: &mut HashSet<Address>,
        pool_buffer: &mut Vec<PoolCreatedEvent>,
        dex: SharedDex,
    ) -> anyhow::Result<()> {
        let batch_addresses: Vec<Address> = token_buffer.drain().collect();
        let token_infos = self.tokens.batch_fetch_token_info(&batch_addresses).await?;

        let mut empty_tokens = HashSet::new();
        // We cache both the multicall failed and decoding errors here to skip the pools.
        let mut decoding_errors_tokens = HashSet::new();

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
                        empty_tokens.insert(token_address);
                        self.cache
                            .add_invalid_token(token_address, &token_info_error.to_string())
                            .await?;
                    }
                    TokenInfoError::DecodingError { .. } => {
                        decoding_errors_tokens.insert(token_address);
                        self.cache
                            .add_invalid_token(token_address, &token_info_error.to_string())
                            .await?;
                    }
                    TokenInfoError::CallFailed { .. } => {
                        decoding_errors_tokens.insert(token_address);
                        self.cache
                            .add_invalid_token(token_address, &token_info_error.to_string())
                            .await?;
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
        let mut pools = Vec::new();
        for pool_event in &mut *pool_buffer {
            // We skip the pool that contains one of the tokens that is flagged as empty or decoding error.
            if empty_tokens.contains(&pool_event.token0)
                || empty_tokens.contains(&pool_event.token1)
                || decoding_errors_tokens.contains(&pool_event.token0)
                || decoding_errors_tokens.contains(&pool_event.token1)
            {
                continue;
            }

            match self.construct_pool(dex.clone(), pool_event).await {
                Ok(pool) => pools.push(pool),
                Err(e) => tracing::error!(
                    "Failed to process {} with error {}",
                    pool_event.pool_address,
                    e
                ),
            }
        }

        self.cache.add_pools_batch(pools).await?;
        pool_buffer.clear();
        Ok(())
    }

    /// Constructs a new `Pool` entity from a pool creation event with full token validation.
    ///
    /// Validates that both tokens are present in the cache and creates a properly
    /// initialized pool entity with all required metadata including DEX, tokens, fees, and block information.
    ///
    /// # Errors
    ///
    /// Returns an error if either token is not found in the cache, indicating incomplete token synchronization.
    async fn construct_pool(
        &mut self,
        dex: SharedDex,
        event: &PoolCreatedEvent,
    ) -> anyhow::Result<Pool> {
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

        Ok(Pool::new(
            self.chain.clone(),
            dex,
            event.pool_address,
            event.block_number,
            token0,
            token1,
            event.fee,
            event.tick_spacing,
            UnixNanos::default(), // TODO: Use default timestamp for now
        ))
    }

    /// Registers a decentralized exchange for data collection and event monitoring.
    ///
    /// Registration involves:
    /// 1. Adding the DEX to the cache
    /// 2. Loading existing pools for the DEX
    /// 3. Configuring event signatures for subscriptions
    ///
    /// # Errors
    ///
    /// Returns an error if DEX registration, cache operations, or pool loading fails.
    pub async fn register_dex_exchange(&mut self, dex_id: DexType) -> anyhow::Result<()> {
        if let Some(dex_extended) = get_dex_extended(self.chain.name, &dex_id) {
            tracing::info!("Registering DEX {dex_id} on chain {}", self.chain.name);

            self.cache.add_dex(dex_extended.dex.clone()).await?;
            self.cache.load_pools(&dex_id).await?;

            self.subscription_manager.register_dex_for_subscriptions(
                dex_id,
                dex_extended.swap_created_event.as_ref(),
                dex_extended.mint_created_event.as_ref(),
                dex_extended.burn_created_event.as_ref(),
            );
            Ok(())
        } else {
            anyhow::bail!("Unknown DEX {dex_id} on chain {}", self.chain.name)
        }
    }

    /// Determines the starting block for syncing operations.
    fn determine_from_block(&self) -> u64 {
        self.config
            .from_block
            .unwrap_or_else(|| self.cache.min_dex_creation_block().unwrap_or(0))
    }

    /// Retrieves extended DEX information for a registered DEX.
    fn get_dex_extended(&self, dex_id: &DexType) -> anyhow::Result<&DexExtended> {
        if !self.cache.get_registered_dexes().contains(dex_id) {
            anyhow::bail!("DEX {dex_id} is not registered in the data client");
        }

        match get_dex_extended(self.chain.name, dex_id) {
            Some(dex) => Ok(dex),
            None => anyhow::bail!("Dex {dex_id} doesn't exist for chain {}", self.chain.name),
        }
    }

    /// Retrieves a pool from the cache by its address.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool is not registered in the cache.
    pub fn get_pool(&self, pool_address: &Address) -> anyhow::Result<&SharedPool> {
        match self.cache.get_pool(pool_address) {
            Some(pool) => Ok(pool),
            None => anyhow::bail!("Pool {pool_address} is not registered"),
        }
    }

    /// Sends a data event to all subscribers through the data channel.
    pub fn send_data(&self, data: DataEvent) {
        if let Some(data_tx) = &self.data_tx {
            tracing::debug!("Sending {data}");

            if let Err(e) = data_tx.send(data) {
                tracing::error!("Failed to send data: {e}");
            }
        } else {
            tracing::error!("No data event channel for sending data");
        }
    }

    /// Disconnects all active connections and cleanup resources.
    ///
    /// This method should be called when shutting down the client to ensure
    /// proper cleanup of network connections and background tasks.
    pub fn disconnect(&mut self) {
        self.hypersync_client.disconnect();
    }
}
