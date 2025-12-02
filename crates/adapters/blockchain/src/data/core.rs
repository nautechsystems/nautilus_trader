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
use nautilus_common::messages::DataEvent;
use nautilus_model::defi::{
    Block, Blockchain, DexType, Pool, PoolIdentifier, PoolLiquidityUpdate, PoolProfiler, PoolSwap,
    SharedChain, SharedDex, SharedPool,
    data::{DefiData, DexPoolData, PoolFeeCollect, PoolFlash, block::BlockPosition},
    pool_analysis::{compare::compare_pool_profiler, snapshot::PoolSnapshot},
    reporting::{BlockchainSyncReportItems, BlockchainSyncReporter},
};
use thousands::Separable;

use crate::{
    cache::BlockchainCache,
    config::BlockchainDataClientConfig,
    contracts::{erc20::Erc20Contract, uniswap_v3_pool::UniswapV3PoolContract},
    data::subscription::DefiDataSubscriptionManager,
    events::{
        burn::BurnEvent, collect::CollectEvent, flash::FlashEvent, mint::MintEvent, swap::SwapEvent,
    },
    exchanges::{extended::DexExtended, get_dex_extended},
    hypersync::{
        client::HyperSyncClient,
        helpers::{extract_block_number, extract_event_signature_bytes},
    },
    rpc::{
        BlockchainRpcClient, BlockchainRpcClientAny,
        chains::{
            arbitrum::ArbitrumRpcClient, base::BaseRpcClient, ethereum::EthereumRpcClient,
            polygon::PolygonRpcClient,
        },
        http::BlockchainHttpRpcClient,
        types::BlockchainMessage,
    },
    services::PoolDiscoveryService,
};

const BLOCKS_PROCESS_IN_SYNC_REPORT: u64 = 50_000;

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
    /// Interface for interacting with UniswapV3 pool contracts.
    univ3_pool: UniswapV3PoolContract,
    /// Client for the HyperSync data indexing service.
    pub hypersync_client: HyperSyncClient,
    /// Optional WebSocket RPC client for direct blockchain node communication.
    pub rpc_client: Option<BlockchainRpcClientAny>,
    /// Manages subscriptions for various DEX events (swaps, mints, burns).
    pub subscription_manager: DefiDataSubscriptionManager,
    /// Channel sender for data events.
    data_tx: Option<tokio::sync::mpsc::UnboundedSender<DataEvent>>,
    /// Cancellation token for graceful shutdown of long-running operations.
    cancellation_token: tokio_util::sync::CancellationToken,
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
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        let chain = config.chain.clone();
        let cache = BlockchainCache::new(chain.clone());

        // Log RPC endpoints being used
        tracing::info!(
            "Initializing blockchain data client for '{}' with HTTP RPC: {}",
            chain.name,
            config.http_rpc_url
        );

        let rpc_client = if !config.use_hypersync_for_live_data && config.wss_rpc_url.is_some() {
            let wss_rpc_url = config.wss_rpc_url.clone().expect("wss_rpc_url is required");
            tracing::info!("WebSocket RPC URL: {}", wss_rpc_url);
            Some(Self::initialize_rpc_client(chain.name, wss_rpc_url))
        } else {
            tracing::info!("Using HyperSync for live data (no WebSocket RPC)");
            None
        };
        let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            config.http_rpc_url.clone(),
            config.rpc_requests_per_second,
        ));
        let erc20_contract = Erc20Contract::new(
            http_rpc_client.clone(),
            config.pool_filters.remove_pools_with_empty_erc20fields,
        );

        let hypersync_client =
            HyperSyncClient::new(chain.clone(), hypersync_tx, cancellation_token.clone());
        Self {
            chain,
            config,
            rpc_client,
            tokens: erc20_contract,
            univ3_pool: UniswapV3PoolContract::new(http_rpc_client),
            cache,
            hypersync_client,
            subscription_manager: DefiDataSubscriptionManager::new(),
            data_tx,
            cancellation_token,
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
            "Connecting to blockchain data source for '{}' from block {}",
            self.chain.name,
            from_block.separate_with_commas()
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
                tracing::info!(
                    "Starting fast sync with COPY from block {}",
                    target_block.separate_with_commas()
                );
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
                    (blocks_status.max_block + 1).separate_with_commas()
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
            "Syncing blocks from {} to {} (total: {} blocks)",
            from_block.separate_with_commas(),
            to_block.separate_with_commas(),
            total_blocks.separate_with_commas()
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

        let cancellation_token = self.cancellation_token.clone();
        let sync_result = tokio::select! {
            () = cancellation_token.cancelled() => {
                tracing::info!("Block sync cancelled");
                Err(anyhow::anyhow!("Sync cancelled"))
            }
            result = async {
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
                Ok(())
            } => result
        };

        sync_result?;

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
        pool_identifier: PoolIdentifier,
        from_block: Option<u64>,
        to_block: Option<u64>,
        reset: bool,
    ) -> anyhow::Result<()> {
        let pool: SharedPool = self.get_pool(&pool_identifier)?.clone();
        let pool_display = pool.to_full_spec_string();
        let from_block = from_block.unwrap_or(pool.creation_block);
        // Extract address for blockchain queries
        let pool_address = &pool.address;

        let (last_synced_block, effective_from_block) = if reset {
            (None, from_block)
        } else {
            let last_synced_block = self
                .cache
                .get_pool_last_synced_block(dex, &pool_identifier)
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
                last_synced_block.unwrap_or(0).separate_with_commas(),
                to_block.separate_with_commas()
            );
            return Ok(());
        }

        // Query table max blocks to detect last blocks to use batch insert before that, then COPY command.
        let last_block_across_pool_events_table = self
            .cache
            .get_pool_event_tables_last_block(&pool_identifier)
            .await?;

        let total_blocks = to_block.saturating_sub(effective_from_block) + 1;
        tracing::info!(
            "Syncing Pool: '{}' events from {} to {} (total: {} blocks){}",
            pool_display,
            effective_from_block.separate_with_commas(),
            to_block.separate_with_commas(),
            total_blocks.separate_with_commas(),
            if let Some(last_synced) = last_synced_block {
                format!(
                    " - resuming from last synced block {}",
                    last_synced.separate_with_commas()
                )
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
        let flash_event_signature = dex_extended.flash_created_event.as_ref();
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
        let flash_sig_bytes = flash_event_signature
            .map(|s| hex::decode(s.strip_prefix("0x").unwrap_or(s)).unwrap_or_default());
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
        if let Some(event) = dex_extended.flash_created_event.as_ref() {
            event_signatures.push(event);
        }
        let pool_events_stream = self
            .hypersync_client
            .request_contract_events_stream(
                effective_from_block,
                Some(to_block),
                pool_address,
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
        let mut flash_batch: Vec<PoolFlash> = Vec::with_capacity(EVENT_BATCH_SIZE);

        // Track when we've moved beyond stale data and can use COPY
        let mut beyond_stale_data = last_block_across_pool_events_table
            .is_none_or(|tables_max| effective_from_block > tables_max);

        let cancellation_token = self.cancellation_token.clone();
        let sync_result = tokio::select! {
            () = cancellation_token.cancelled() => {
                tracing::info!("Pool event sync cancelled");
                Err(anyhow::anyhow!("Sync cancelled"))
            }
            result = async {
                while let Some(log) = pool_events_stream.next().await {
                    let block_number = extract_block_number(&log)?;
                    blocks_processed += block_number - last_block_saved;
                    last_block_saved = block_number;

                    let event_sig_bytes = extract_event_signature_bytes(&log)?;
            if event_sig_bytes == swap_sig_bytes.as_slice() {
                let swap_event = dex_extended.parse_swap_event_hypersync(log)?;
                match self.process_pool_swap_event(&swap_event, &pool) {
                    Ok(swap) => swap_batch.push(swap),
                    Err(e) => tracing::error!("Failed to process swap event: {e}"),
                }
            } else if event_sig_bytes == mint_sig_bytes.as_slice() {
                let mint_event = dex_extended.parse_mint_event_hypersync(log)?;
                match self.process_pool_mint_event(&mint_event, &pool, &dex_extended) {
                    Ok(liquidity_update) => liquidity_batch.push(liquidity_update),
                    Err(e) => tracing::error!("Failed to process mint event: {e}"),
                }
            } else if event_sig_bytes == burn_sig_bytes.as_slice() {
                let burn_event = dex_extended.parse_burn_event_hypersync(log)?;
                match self.process_pool_burn_event(&burn_event, &pool, &dex_extended) {
                    Ok(liquidity_update) => liquidity_batch.push(liquidity_update),
                    Err(e) => tracing::error!("Failed to process burn event: {e}"),
                }
            } else if event_sig_bytes == collect_sig_bytes.as_slice() {
                let collect_event = dex_extended.parse_collect_event_hypersync(log)?;
                match self.process_pool_collect_event(&collect_event, &pool, &dex_extended) {
                    Ok(fee_collect) => collect_batch.push(fee_collect),
                    Err(e) => tracing::error!("Failed to process collect event: {e}"),
                }
            } else if initialize_sig_bytes.as_ref().is_some_and(|sig| sig.as_slice() == event_sig_bytes) {
                let initialize_event = dex_extended.parse_initialize_event_hypersync(log)?;
                self.cache
                    .update_pool_initialize_price_tick(&initialize_event)
                    .await?;
            } else if flash_sig_bytes.as_ref().is_some_and(|sig| sig.as_slice() == event_sig_bytes) {
                if let Some(parse_fn) = dex_extended.parse_flash_event_hypersync_fn {
                    match parse_fn(dex_extended.dex.clone(), log) {
                        Ok(flash_event) => {
                            match self.process_pool_flash_event(&flash_event, &pool) {
                                Ok(flash) => flash_batch.push(flash),
                                Err(e) => tracing::error!("Failed to process flash event: {e}"),
                            }
                        }
                        Err(e) => tracing::error!("Failed to parse flash event: {e}"),
                    }
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
                    .is_some_and(|table_max| block_number > table_max)
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
                    &mut flash_batch,
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
                    &mut flash_batch,
                    false, // TODO temporary dont use copy command
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
                    .update_pool_last_synced_block(dex, &pool_identifier, block_number)
                    .await?;
            }
        }

        self.flush_event_batches(
            EVENT_BATCH_SIZE,
            &mut swap_batch,
            &mut liquidity_batch,
            &mut collect_batch,
            &mut flash_batch,
            false,
            true,
        )
        .await?;

        metrics.log_final_stats();
        self.cache
            .update_pool_last_synced_block(dex, &pool_identifier, to_block)
            .await?;

        tracing::info!(
            "Successfully synced Dex '{}' Pool '{}' up to block {}",
            dex,
            pool_display,
            to_block.separate_with_commas()
        );
                Ok(())
            } => result
        };

        sync_result
    }

    #[allow(clippy::too_many_arguments)]
    async fn flush_event_batches(
        &mut self,
        event_batch_size: usize,
        swap_batch: &mut Vec<PoolSwap>,
        liquidity_batch: &mut Vec<PoolLiquidityUpdate>,
        collect_batch: &mut Vec<PoolFeeCollect>,
        flash_batch: &mut Vec<PoolFlash>,
        use_copy_command: bool,
        force_flush_all: bool,
    ) -> anyhow::Result<()> {
        if (force_flush_all || swap_batch.len() >= event_batch_size) && !swap_batch.is_empty() {
            self.cache
                .add_pool_swaps_batch(swap_batch, use_copy_command)
                .await?;
            swap_batch.clear();
        }
        if (force_flush_all || liquidity_batch.len() >= event_batch_size)
            && !liquidity_batch.is_empty()
        {
            self.cache
                .add_pool_liquidity_updates_batch(liquidity_batch, use_copy_command)
                .await?;
            liquidity_batch.clear();
        }
        if (force_flush_all || collect_batch.len() >= event_batch_size) && !collect_batch.is_empty()
        {
            self.cache
                .add_pool_fee_collects_batch(collect_batch, use_copy_command)
                .await?;
            collect_batch.clear();
        }
        if (force_flush_all || flash_batch.len() >= event_batch_size) && !flash_batch.is_empty() {
            self.cache.add_pool_flash_batch(flash_batch).await?;
            flash_batch.clear();
        }
        Ok(())
    }

    /// Processes a swap event and converts it to a pool swap.
    ///
    /// # Errors
    ///
    /// Returns an error if swap event processing fails.
    ///
    /// # Panics
    ///
    /// Panics if swap event conversion to trade data fails.
    pub fn process_pool_swap_event(
        &self,
        swap_event: &SwapEvent,
        pool: &SharedPool,
    ) -> anyhow::Result<PoolSwap> {
        let timestamp = self
            .cache
            .get_block_timestamp(swap_event.block_number)
            .copied();
        let mut swap = swap_event.to_pool_swap(
            self.chain.clone(),
            pool.instrument_id,
            pool.pool_identifier,
            timestamp,
        );
        swap.calculate_trade_info(&pool.token0, &pool.token1, None)?;

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

        let liquidity_update = mint_event.to_pool_liquidity_update(
            self.chain.clone(),
            dex_extended.dex.clone(),
            pool.instrument_id,
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

        let liquidity_update = burn_event.to_pool_liquidity_update(
            self.chain.clone(),
            dex_extended.dex.clone(),
            pool.instrument_id,
            pool.pool_identifier,
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

        let fee_collect = collect_event.to_pool_fee_collect(
            self.chain.clone(),
            dex_extended.dex.clone(),
            pool.instrument_id,
            timestamp,
        );

        Ok(fee_collect)
    }

    /// Processes a pool flash event and converts it to a flash loan.
    ///
    /// # Errors
    ///
    /// Returns an error if the flash event processing fails or if the flash loan creation fails.
    pub fn process_pool_flash_event(
        &self,
        flash_event: &FlashEvent,
        pool: &SharedPool,
    ) -> anyhow::Result<PoolFlash> {
        let timestamp = self
            .cache
            .get_block_timestamp(flash_event.block_number)
            .copied();

        let flash = flash_event.to_pool_flash(self.chain.clone(), pool.instrument_id, timestamp);

        Ok(flash)
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
        let dex_extended = self.get_dex_extended(dex)?.clone();

        let mut service = PoolDiscoveryService::new(
            self.chain.clone(),
            &mut self.cache,
            &self.tokens,
            &self.hypersync_client,
            self.cancellation_token.clone(),
            self.config.clone(),
        );

        service
            .sync_pools(&dex_extended, from_block, to_block, reset)
            .await?;

        Ok(())
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
            let _ = self.cache.load_pools(&dex_id).await?;

            self.subscription_manager.register_dex_for_subscriptions(
                dex_id,
                dex_extended.swap_created_event.as_ref(),
                dex_extended.mint_created_event.as_ref(),
                dex_extended.burn_created_event.as_ref(),
                dex_extended.collect_created_event.as_ref(),
                dex_extended.flash_created_event.as_deref(),
            );
            Ok(())
        } else {
            anyhow::bail!("Unknown DEX {dex_id} on chain {}", self.chain.name)
        }
    }

    /// Bootstraps a [`PoolProfiler`] with the latest state for a given pool.
    ///
    /// Uses two paths depending on whether the pool has been synced to the database:
    /// - **Never synced**: Streams events from HyperSync → restores from on-chain RPC → returns `(profiler, true)`
    /// - **Previously synced**: Syncs new events to DB → streams from DB → returns `(profiler, false)`
    ///
    /// Both paths restore from the latest valid snapshot first (if available), otherwise initialize with pool's initial price.
    ///
    /// # Returns
    ///
    /// - `PoolProfiler`: Hydrated profiler with current pool state
    /// - `bool`: `true` if constructed from RPC (already valid), `false` if from DB (needs validation)
    ///
    /// # Errors
    ///
    /// Returns an error if database is not initialized or event processing fails.
    ///
    /// # Panics
    ///
    /// Panics if the database reference is unavailable.
    pub async fn bootstrap_latest_pool_profiler(
        &mut self,
        pool: &SharedPool,
    ) -> anyhow::Result<(PoolProfiler, bool)> {
        tracing::info!(
            "Bootstrapping latest pool profiler for pool {}",
            pool.address
        );

        if self.cache.database.is_none() {
            anyhow::bail!(
                "Database is not initialized, so we cannot properly bootstrap the latest pool profiler"
            );
        }

        let mut profiler = PoolProfiler::new(pool.clone());

        // Calculate latest valid block position after which we need to start profiling.
        let from_position = match self
            .cache
            .database
            .as_ref()
            .unwrap()
            .load_latest_valid_pool_snapshot(pool.chain.chain_id, &pool.pool_identifier)
            .await
        {
            Ok(Some(snapshot)) => {
                tracing::info!(
                    "Loaded valid snapshot from block {} which contains {} positions and {} ticks",
                    snapshot.block_position.number.separate_with_commas(),
                    snapshot.positions.len(),
                    snapshot.ticks.len()
                );
                let block_position = snapshot.block_position.clone();
                profiler.restore_from_snapshot(snapshot)?;
                tracing::info!("Restored profiler from snapshot");
                Some(block_position)
            }
            _ => {
                tracing::info!("No valid snapshot found, processing from beginning");
                None
            }
        };

        // If we don't have never synced pool events, proceed with faster
        // construction of pool profiler from hypersync and RPC, where we
        // dont need syncing of pool events and fetching it from database
        if self
            .cache
            .database
            .as_ref()
            .unwrap()
            .get_pool_last_synced_block(self.chain.chain_id, &pool.dex.name, &pool.pool_identifier)
            .await?
            .is_none()
        {
            return self
                .construct_pool_profiler_from_hypersync_rpc(profiler, from_position)
                .await;
        }

        // Sync the pool events before bootstrapping of pool profiler
        if let Err(e) = self
            .sync_pool_events(&pool.dex.name, pool.pool_identifier, None, None, false)
            .await
        {
            tracing::error!("Failed to sync pool events for snapshot request: {}", e);
        }

        if !profiler.is_initialized {
            if let Some(initial_sqrt_price_x96) = pool.initial_sqrt_price_x96 {
                profiler.initialize(initial_sqrt_price_x96);
            } else {
                anyhow::bail!(
                    "Pool is not initialized and it doesn't contain initial price, cannot bootstrap profiler"
                );
            }
        }

        let from_block = from_position
            .as_ref()
            .map_or(profiler.pool.creation_block, |block_position| {
                block_position.number
            });
        let to_block = self.hypersync_client.current_block().await;
        let total_blocks = to_block.saturating_sub(from_block) + 1;

        // Enable embedded profiler reporting
        profiler.enable_reporting(from_block, total_blocks, BLOCKS_PROCESS_IN_SYNC_REPORT);

        let mut stream = self.cache.database.as_ref().unwrap().stream_pool_events(
            pool.chain.clone(),
            pool.dex.clone(),
            pool.instrument_id,
            pool.pool_identifier,
            from_position.clone(),
        );

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    profiler.process(&event)?;
                }
                Err(e) => log::error!("Error processing event: {e}"),
            }
        }

        profiler.finalize_reporting();

        Ok((profiler, false))
    }

    /// Constructs a pool profiler by fetching events directly from HyperSync RPC.
    ///
    /// This method is used when the pool has never been synced to the database. It streams
    /// liquidity events (mints, burns) directly from HyperSync and processes them
    /// to build up the profiler's state in real-time. After processing all events, it
    /// restores the profiler from the current on-chain state with the provided ticks and positions
    ///
    /// # Returns
    ///
    /// Returns a tuple of:
    /// - `PoolProfiler`: The hydrated profiler with state built from events
    /// - `bool`: Always `true` to indicate the profiler state was valid, and it was constructed from RPC
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Event streaming from HyperSync fails
    /// - Event parsing or processing fails
    /// - DEX configuration is invalid
    async fn construct_pool_profiler_from_hypersync_rpc(
        &self,
        mut profiler: PoolProfiler,
        from_position: Option<BlockPosition>,
    ) -> anyhow::Result<(PoolProfiler, bool)> {
        tracing::info!(
            "Constructing pool profiler from hypersync stream and RPC final state querying"
        );
        let dex_extended = self.get_dex_extended(&profiler.pool.dex.name)?.clone();
        let mint_event_signature = dex_extended.mint_created_event.as_ref();
        let burn_event_signature = dex_extended.burn_created_event.as_ref();
        let initialize_event_signature =
            if let Some(initialize_event) = &dex_extended.initialize_event {
                initialize_event.as_ref()
            } else {
                anyhow::bail!(
                    "DEX {} does not have initialize event set.",
                    &profiler.pool.dex.name
                );
            };
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
        let initialize_sig_bytes = hex::decode(
            initialize_event_signature
                .strip_prefix("0x")
                .unwrap_or(initialize_event_signature),
        )?;

        let from_block = from_position.map_or(profiler.pool.creation_block, |block_position| {
            block_position.number
        });
        let to_block = self.hypersync_client.current_block().await;
        let total_blocks = to_block.saturating_sub(from_block) + 1;

        tracing::info!(
            "Bootstrapping pool profiler for pool {} from block {} to {} (total: {} blocks)",
            profiler.pool.address,
            from_block.separate_with_commas(),
            to_block.separate_with_commas(),
            total_blocks.separate_with_commas()
        );

        // Enable embedded profiler reporting
        profiler.enable_reporting(from_block, total_blocks, BLOCKS_PROCESS_IN_SYNC_REPORT);

        let pool_events_stream = self
            .hypersync_client
            .request_contract_events_stream(
                from_block,
                None,
                &profiler.pool.address,
                vec![
                    mint_event_signature,
                    burn_event_signature,
                    initialize_event_signature,
                ],
            )
            .await;
        tokio::pin!(pool_events_stream);

        while let Some(log) = pool_events_stream.next().await {
            let event_sig_bytes = extract_event_signature_bytes(&log)?;

            if event_sig_bytes == initialize_sig_bytes {
                let initialize_event = dex_extended.parse_initialize_event_hypersync(log)?;
                profiler.initialize(initialize_event.sqrt_price_x96);
                self.cache
                    .database
                    .as_ref()
                    .unwrap()
                    .update_pool_initial_price_tick(self.chain.chain_id, &initialize_event)
                    .await?;
            } else if event_sig_bytes == mint_sig_bytes {
                let mint_event = dex_extended.parse_mint_event_hypersync(log)?;
                match self.process_pool_mint_event(&mint_event, &profiler.pool, &dex_extended) {
                    Ok(liquidity_update) => {
                        profiler.process(&DexPoolData::LiquidityUpdate(liquidity_update))?;
                    }
                    Err(e) => tracing::error!("Failed to process mint event: {e}"),
                }
            } else if event_sig_bytes == burn_sig_bytes {
                let burn_event = dex_extended.parse_burn_event_hypersync(log)?;
                match self.process_pool_burn_event(&burn_event, &profiler.pool, &dex_extended) {
                    Ok(liquidity_update) => {
                        profiler.process(&DexPoolData::LiquidityUpdate(liquidity_update))?;
                    }
                    Err(e) => tracing::error!("Failed to process burn event: {e}"),
                }
            } else {
                let event_signature = hex::encode(event_sig_bytes);
                tracing::error!(
                    "Unexpected event signature in bootstrap_latest_pool_profiler: {} for log {:?}",
                    event_signature,
                    log
                );
            }
        }

        profiler.finalize_reporting();

        // Hydrate from the current RPC state
        match self.get_on_chain_snapshot(&profiler).await {
            Ok(on_chain_snapshot) => profiler.restore_from_snapshot(on_chain_snapshot)?,
            Err(e) => tracing::error!(
                "Failed to restore from on-chain snapshot: {e}. Sending not hydrated state to client."
            ),
        }

        Ok((profiler, true))
    }

    /// Validates a pool profiler's state against on-chain data for accuracy verification.
    ///
    /// This method performs integrity checking by comparing the profiler's internal state
    /// (positions, ticks, liquidity) with the actual on-chain smart contract state. For UniswapV3
    /// pools, it fetches current on-chain data and verifies that the profiler's tracked state matches.
    /// If validation succeeds or is bypassed, the snapshot is marked as valid in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if database operations fail when marking the snapshot as valid.
    ///
    /// # Panics
    ///
    /// Panics if the profiler does not have a last_processed_event when already_validated is true.
    pub async fn check_snapshot_validity(
        &self,
        profiler: &PoolProfiler,
        already_validated: bool,
    ) -> anyhow::Result<bool> {
        // Determine validity and get block position for marking
        let (is_valid, block_position) = if already_validated {
            // Skip RPC call - profiler was validated during construction from RPC
            tracing::info!("Snapshot already validated from RPC, skipping on-chain comparison");
            let last_event = profiler
                .last_processed_event
                .clone()
                .expect("Profiler should have last_processed_event");
            (true, last_event)
        } else {
            // Fetch on-chain state and compare
            match self.get_on_chain_snapshot(profiler).await {
                Ok(on_chain_snapshot) => {
                    tracing::info!("Comparing profiler state with on-chain state...");
                    let valid = compare_pool_profiler(profiler, &on_chain_snapshot);
                    if !valid {
                        tracing::error!(
                            "Pool profiler state does NOT match on-chain smart contract state"
                        );
                    }
                    (valid, on_chain_snapshot.block_position)
                }
                Err(e) => {
                    tracing::error!("Failed to check snapshot validity: {e}");
                    return Ok(false);
                }
            }
        };

        // Mark snapshot as valid in database if validation passed
        if is_valid && let Some(cache_database) = &self.cache.database {
            cache_database
                .mark_pool_snapshot_valid(
                    profiler.pool.chain.chain_id,
                    &profiler.pool.pool_identifier,
                    block_position.number,
                    block_position.transaction_index,
                    block_position.log_index,
                )
                .await?;
            tracing::info!("Marked pool profiler snapshot as valid");
        }

        Ok(is_valid)
    }

    /// Fetches current on-chain pool state at the last processed block.
    ///
    /// Queries the pool smart contract to retrieve active tick liquidity and position data,
    /// using the profiler's active positions and last processed block number.
    /// Used for profiler state restoration after bootstrapping and validation.
    async fn get_on_chain_snapshot(&self, profiler: &PoolProfiler) -> anyhow::Result<PoolSnapshot> {
        if profiler.pool.dex.name == DexType::UniswapV3 {
            let last_processed_event = profiler
                .last_processed_event
                .clone()
                .expect("We expect at least one processed event in the pool");
            let on_chain_snapshot = self
                .univ3_pool
                .fetch_snapshot(
                    &profiler.pool.address,
                    profiler.pool.instrument_id,
                    profiler.get_active_tick_values().as_slice(),
                    &profiler.get_all_position_keys(),
                    last_processed_event,
                )
                .await?;

            Ok(on_chain_snapshot)
        } else {
            anyhow::bail!(
                "Fetching on-chain snapshot for Dex protocol {} is not supported yet.",
                profiler.pool.dex.name
            )
        }
    }

    /// Replays historical events for a pool to hydrate its profiler state.
    ///
    /// Streams all historical swap, liquidity, and fee collect events from the database
    /// and sends them through the normal data event pipeline to build up pool profiler state.
    ///
    /// # Errors
    ///
    /// Returns an error if database streaming fails or event processing fails.
    pub async fn replay_pool_events(&self, pool: &Pool, dex: &SharedDex) -> anyhow::Result<()> {
        if let Some(database) = &self.cache.database {
            tracing::info!(
                "Replaying historical events for pool {} to hydrate profiler",
                pool.instrument_id
            );

            let mut event_stream = database.stream_pool_events(
                self.chain.clone(),
                dex.clone(),
                pool.instrument_id,
                pool.pool_identifier,
                None,
            );
            let mut event_count = 0;

            while let Some(event_result) = event_stream.next().await {
                match event_result {
                    Ok(event) => {
                        let data_event = match event {
                            DexPoolData::Swap(swap) => DataEvent::DeFi(DefiData::PoolSwap(swap)),
                            DexPoolData::LiquidityUpdate(update) => {
                                DataEvent::DeFi(DefiData::PoolLiquidityUpdate(update))
                            }
                            DexPoolData::FeeCollect(collect) => {
                                DataEvent::DeFi(DefiData::PoolFeeCollect(collect))
                            }
                            DexPoolData::Flash(flash) => {
                                DataEvent::DeFi(DefiData::PoolFlash(flash))
                            }
                        };
                        self.send_data(data_event);
                        event_count += 1;
                    }
                    Err(e) => {
                        tracing::error!(
                            "Error streaming event for pool {}: {e}",
                            pool.instrument_id
                        );
                    }
                }
            }

            tracing::info!(
                "Replayed {event_count} historical events for pool {}",
                pool.instrument_id
            );
        } else {
            tracing::debug!(
                "No database available, skipping event replay for pool {}",
                pool.instrument_id
            );
        }

        Ok(())
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
    pub fn get_pool(&self, pool_identifier: &PoolIdentifier) -> anyhow::Result<&SharedPool> {
        match self.cache.get_pool(pool_identifier) {
            Some(pool) => Ok(pool),
            None => anyhow::bail!("Pool {pool_identifier} is not registered"),
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
    pub async fn disconnect(&mut self) {
        self.hypersync_client.disconnect().await;
    }
}
