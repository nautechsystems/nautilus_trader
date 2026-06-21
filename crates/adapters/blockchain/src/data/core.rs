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

use std::{cmp::max, sync::Arc};

use anyhow::Context;
use futures_util::StreamExt;
use nautilus_common::messages::DataEvent;
use nautilus_core::{UnixNanos, hex, string::formatting::Separable};
use nautilus_model::defi::{
    Block, Blockchain, DexType, Pool, PoolIdentifier, PoolLiquidityUpdate, PoolProfiler, PoolSwap,
    SharedChain, SharedDex, SharedPool,
    data::{DefiData, DexPoolData, PoolFeeCollect, PoolFlash, block::BlockPosition},
    pool_analysis::{compare::compare_pool_profiler_detailed, snapshot::PoolSnapshot},
    reporting::{BlockchainSyncReportItems, BlockchainSyncReporter},
};
use nautilus_network::websocket::TransportBackend;

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
        client::{HyperSyncClient, PoolEventStreamItem},
        helpers::{extract_block_number, extract_event_signature_bytes},
    },
    rpc::{
        BlockchainRpcClient, BlockchainRpcClientAny,
        chains::{
            arbitrum::ArbitrumRpcClient, base::BaseRpcClient, bsc::BscRpcClient,
            ethereum::EthereumRpcClient, polygon::PolygonRpcClient,
        },
        http::BlockchainHttpRpcClient,
        types::BlockchainMessage,
    },
    services::PoolDiscoveryService,
};

const BLOCKS_PROCESS_IN_SYNC_REPORT: u64 = 50_000;
const POOL_EVENT_BLOCK_BATCH_SIZE: usize = 20_000;

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

/// Outcome of validating a pool snapshot against on-chain state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotValidation {
    /// Hydrated from chain and matched the profiler state.
    OnChain,
    /// Emitted from deterministic replay and not checked against chain (the RPC could not serve the
    /// block, or validation was skipped). Usable as a replay start point.
    Replay,
    /// Hydrated from chain and did not match the profiler state. Not usable as a replay start point.
    Invalid,
}

impl SnapshotValidation {
    /// Returns `true` if the snapshot is usable as a replay start point.
    #[must_use]
    pub const fn is_usable(self) -> bool {
        !matches!(self, Self::Invalid)
    }

    /// Returns the database/JSON token for this state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OnChain => "on_chain",
            Self::Replay => "replay",
            Self::Invalid => "invalid",
        }
    }

    /// Parses the database/JSON token into a [`SnapshotValidation`].
    ///
    /// Returns `None` for an unrecognized token.
    #[must_use]
    pub fn from_db_token(token: &str) -> Option<Self> {
        match token {
            "on_chain" => Some(Self::OnChain),
            "replay" => Some(Self::Replay),
            "invalid" => Some(Self::Invalid),
            _ => None,
        }
    }
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
        log::info!(
            "Initializing blockchain data client for '{}' with HTTP RPC: {}",
            chain.name,
            config.http_rpc_url
        );

        let rpc_client = if !config.use_hypersync_for_live_data && config.wss_rpc_url.is_some() {
            let wss_rpc_url = config.wss_rpc_url.clone().expect("wss_rpc_url is required");
            log::info!("WebSocket RPC URL: {wss_rpc_url}");
            Some(Self::initialize_rpc_client(
                chain.name,
                wss_rpc_url,
                config.transport_backend,
                config.proxy_url.clone(),
            ))
        } else {
            log::info!("Using HyperSync for live data (no WebSocket RPC)");
            None
        };
        let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            config.http_rpc_url.clone(),
            config.rpc_requests_per_second,
            config.proxy_url.clone(),
        ));
        let multicall_calls_per_rpc_request = config.multicall_calls_per_rpc_request;
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
            univ3_pool: UniswapV3PoolContract::new(
                http_rpc_client,
                multicall_calls_per_rpc_request,
            ),
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
            log::info!(
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
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> BlockchainRpcClientAny {
        let mut client = match blockchain {
            Blockchain::Ethereum => {
                BlockchainRpcClientAny::Ethereum(EthereumRpcClient::new(wss_rpc_url, proxy_url))
            }
            Blockchain::Polygon => {
                BlockchainRpcClientAny::Polygon(PolygonRpcClient::new(wss_rpc_url, proxy_url))
            }
            Blockchain::Base => {
                BlockchainRpcClientAny::Base(BaseRpcClient::new(wss_rpc_url, proxy_url))
            }
            Blockchain::Arbitrum => {
                BlockchainRpcClientAny::Arbitrum(ArbitrumRpcClient::new(wss_rpc_url, proxy_url))
            }
            Blockchain::Bsc => {
                BlockchainRpcClientAny::Bsc(BscRpcClient::new(wss_rpc_url, proxy_url))
            }
            _ => panic!("Unsupported blockchain {blockchain} for RPC connection"),
        };
        client.set_transport_backend(transport_backend);
        client
    }

    /// Establishes connections to all configured data sources and initializes the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if cache initialization or connection setup fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Connecting blockchain data client for '{}'",
            self.chain.name
        );
        self.initialize_cache_database().await;

        if let Some(ref mut rpc_client) = self.rpc_client {
            rpc_client.connect().await?;
        }

        let from_block = self.determine_from_block();

        log::info!(
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
                log::info!(
                    "Cache is consistent: no gaps detected (last continuous block: {})",
                    blocks_status.last_continuous_block
                );
                let target_block = max(blocks_status.max_block + 1, from_block);
                log::info!(
                    "Starting fast sync with COPY from block {}",
                    target_block.separate_with_commas()
                );
                self.sync_blocks(target_block, to_block, true).await?;
            } else {
                let gap_size = blocks_status.max_block - blocks_status.last_continuous_block;
                log::info!(
                    "Cache inconsistency detected: {} blocks missing between {} and {}",
                    gap_size,
                    blocks_status.last_continuous_block + 1,
                    blocks_status.max_block
                );

                log::info!(
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

                log::info!(
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
        const BATCH_SIZE: usize = 1000;

        let to_block = if let Some(block) = to_block {
            block
        } else {
            self.hypersync_client.current_block().await
        };
        let total_blocks = to_block.saturating_sub(from_block) + 1;
        log::info!(
            "Syncing blocks from {} to {} (total: {} blocks)",
            from_block.separate_with_commas(),
            to_block.separate_with_commas(),
            total_blocks.separate_with_commas()
        );

        // Enable performance settings for sync operations
        if let Err(e) = self.cache.toggle_performance_settings(true).await {
            log::warn!("Failed to enable performance settings: {e}");
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

        let mut batch: Vec<Block> = Vec::with_capacity(BATCH_SIZE);

        let cancellation_token = self.cancellation_token.clone();
        let sync_result = tokio::select! {
            () = cancellation_token.cancelled() => {
                log::info!("Block sync cancelled");
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
            log::warn!("Failed to restore default settings: {e}");
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
        const EVENT_BATCH_SIZE: usize = 20000;

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
            log::info!(
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
        log::info!(
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

        let mut swap_batch: Vec<PoolSwap> = Vec::with_capacity(EVENT_BATCH_SIZE);
        let mut liquidity_batch: Vec<PoolLiquidityUpdate> = Vec::with_capacity(EVENT_BATCH_SIZE);
        let mut collect_batch: Vec<PoolFeeCollect> = Vec::with_capacity(EVENT_BATCH_SIZE);
        let mut flash_batch: Vec<PoolFlash> = Vec::with_capacity(EVENT_BATCH_SIZE);
        let mut block_batch: Vec<Block> = Vec::with_capacity(POOL_EVENT_BLOCK_BATCH_SIZE);

        // Track when we've moved beyond stale data and can use COPY
        let mut beyond_stale_data = last_block_across_pool_events_table
            .is_none_or(|tables_max| effective_from_block > tables_max);

        let cancellation_token = self.cancellation_token.clone();
        let sync_result = tokio::select! {
            () = cancellation_token.cancelled() => {
                log::info!("Pool event sync cancelled");
                Err(anyhow::anyhow!("Sync cancelled"))
            }
            result = async {
                while let Some(item) = pool_events_stream.next().await {
                    let log = match item {
                        PoolEventStreamItem::Block(block) => {
                            self.record_pool_event_block(block, &mut block_batch).await?;
                            continue;
                        }
                        PoolEventStreamItem::Log(log) => log,
                    };
                    let block_number = extract_block_number(&log)?;
                    blocks_processed += block_number - last_block_saved;
                    last_block_saved = block_number;

                    let event_sig_bytes = extract_event_signature_bytes(&log)?;
            if event_sig_bytes == swap_sig_bytes.as_slice() {
                let swap_event = dex_extended.parse_swap_event_hypersync(&log)?;
                let swap = self
                    .process_pool_swap_event(&swap_event, &pool)
                    .with_context(|| {
                        format!("failed to process swap event at block {}", swap_event.block_number)
                    })?;
                swap_batch.push(swap);
            } else if event_sig_bytes == mint_sig_bytes.as_slice() {
                let mint_event = dex_extended.parse_mint_event_hypersync(&log)?;
                let liquidity_update = self
                    .process_pool_mint_event(&mint_event, &pool, &dex_extended)
                    .with_context(|| {
                        format!("failed to process mint event at block {}", mint_event.block_number)
                    })?;
                liquidity_batch.push(liquidity_update);
            } else if event_sig_bytes == burn_sig_bytes.as_slice() {
                let burn_event = dex_extended.parse_burn_event_hypersync(&log)?;
                let liquidity_update = self
                    .process_pool_burn_event(&burn_event, &pool, &dex_extended)
                    .with_context(|| {
                        format!("failed to process burn event at block {}", burn_event.block_number)
                    })?;
                liquidity_batch.push(liquidity_update);
            } else if event_sig_bytes == collect_sig_bytes.as_slice() {
                let collect_event = dex_extended.parse_collect_event_hypersync(&log)?;
                let fee_collect = self
                    .process_pool_collect_event(&collect_event, &pool, &dex_extended)
                    .with_context(|| {
                        format!(
                            "failed to process collect event at block {}",
                            collect_event.block_number
                        )
                    })?;
                collect_batch.push(fee_collect);
            } else if initialize_sig_bytes.as_ref().is_some_and(|sig| sig.as_slice() == event_sig_bytes) {
                let initialize_event = dex_extended.parse_initialize_event_hypersync(&log)?;
                self.cache
                    .update_pool_initialize_price_tick(&initialize_event)
                    .await?;
            } else if flash_sig_bytes.as_ref().is_some_and(|sig| sig.as_slice() == event_sig_bytes) {
                let parse_fn = dex_extended
                    .parse_flash_event_hypersync_fn
                    .context("missing flash event parser")?;
                let flash_event = parse_fn(dex_extended.dex.clone(), &log)
                    .context("failed to parse flash event")?;
                let flash = self
                    .process_pool_flash_event(&flash_event, &pool)
                    .with_context(|| {
                        format!("failed to process flash event at block {}", flash_event.block_number)
                    })?;
                flash_batch.push(flash);
            } else {
                let event_signature = hex::encode(event_sig_bytes);
                anyhow::bail!("unexpected event signature {event_signature} for log {log:?}");
            }

            // Check if we've moved beyond stale data (transition point for strategy change)
            if !beyond_stale_data
                && last_block_across_pool_events_table
                    .is_some_and(|table_max| block_number > table_max)
            {
                log::info!(
                    "Crossed beyond stale data at block {block_number} - flushing current batches with ON CONFLICT, then switching to COPY"
                );

                // Flush all batches with ON CONFLICT to handle any remaining duplicates
                self.flush_event_batches(
                    EVENT_BATCH_SIZE,
                    &mut block_batch,
                    &mut swap_batch,
                    &mut liquidity_batch,
                    &mut collect_batch,
                    &mut flash_batch,
                    false,
                    true,
                )
                .await?;

                beyond_stale_data = true;
                log::info!("Switched to COPY mode - future batches will use COPY command");
            } else {
                // Process batches when they reach batch size
                self.flush_event_batches(
                    EVENT_BATCH_SIZE,
                    &mut block_batch,
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
                self.flush_event_batches(
                    EVENT_BATCH_SIZE,
                    &mut block_batch,
                    &mut swap_batch,
                    &mut liquidity_batch,
                    &mut collect_batch,
                    &mut flash_batch,
                    false,
                    true,
                )
                .await?;

                if let Some(checkpoint_block) =
                    Self::completed_pool_event_checkpoint(block_number, effective_from_block)
                {
                    self.cache
                        .update_pool_last_synced_block(dex, &pool_identifier, checkpoint_block)
                        .await?;
                }
            }
        }

        self.flush_event_batches(
            EVENT_BATCH_SIZE,
            &mut block_batch,
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

        log::info!(
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

    #[expect(clippy::too_many_arguments)]
    async fn flush_event_batches(
        &mut self,
        event_batch_size: usize,
        block_batch: &mut Vec<Block>,
        swap_batch: &mut Vec<PoolSwap>,
        liquidity_batch: &mut Vec<PoolLiquidityUpdate>,
        collect_batch: &mut Vec<PoolFeeCollect>,
        flash_batch: &mut Vec<PoolFlash>,
        use_copy_command: bool,
        force_flush_all: bool,
    ) -> anyhow::Result<()> {
        let should_flush_swaps =
            (force_flush_all || swap_batch.len() >= event_batch_size) && !swap_batch.is_empty();
        let should_flush_liquidity = (force_flush_all || liquidity_batch.len() >= event_batch_size)
            && !liquidity_batch.is_empty();
        let should_flush_collects = (force_flush_all || collect_batch.len() >= event_batch_size)
            && !collect_batch.is_empty();
        let should_flush_flash =
            (force_flush_all || flash_batch.len() >= event_batch_size) && !flash_batch.is_empty();

        if force_flush_all
            || should_flush_swaps
            || should_flush_liquidity
            || should_flush_collects
            || should_flush_flash
        {
            self.flush_pool_event_blocks(block_batch).await?;
        }

        if should_flush_swaps {
            self.cache
                .add_pool_swaps_batch(swap_batch, use_copy_command)
                .await?;
            swap_batch.clear();
        }

        if should_flush_liquidity {
            self.cache
                .add_pool_liquidity_updates_batch(liquidity_batch, use_copy_command)
                .await?;
            liquidity_batch.clear();
        }

        if should_flush_collects {
            self.cache
                .add_pool_fee_collects_batch(collect_batch, use_copy_command)
                .await?;
            collect_batch.clear();
        }

        if should_flush_flash {
            self.cache.add_pool_flash_batch(flash_batch).await?;
            flash_batch.clear();
        }
        Ok(())
    }

    async fn record_pool_event_block(
        &mut self,
        block: Block,
        block_batch: &mut Vec<Block>,
    ) -> anyhow::Result<()> {
        self.cache
            .cache_block_timestamp(block.number, block.timestamp);
        block_batch.push(block);
        if block_batch.len() >= POOL_EVENT_BLOCK_BATCH_SIZE {
            self.flush_pool_event_blocks(block_batch).await?;
        }
        Ok(())
    }

    async fn flush_pool_event_blocks(
        &mut self,
        block_batch: &mut Vec<Block>,
    ) -> anyhow::Result<()> {
        if block_batch.is_empty() {
            return Ok(());
        }

        self.cache
            .add_pool_event_blocks_batch(std::mem::take(block_batch))
            .await
    }

    fn completed_pool_event_checkpoint(
        block_number: u64,
        effective_from_block: u64,
    ) -> Option<u64> {
        let checkpoint_block = block_number.checked_sub(1)?;
        (checkpoint_block >= effective_from_block).then_some(checkpoint_block)
    }

    /// Processes a swap event and converts it to a pool swap.
    ///
    /// Trade-info computation can fail for degenerate MIN/MAX-tick swaps on near-zero-liquidity
    /// pools, whose spot price overflows the price representation. Such failures are non-fatal: the
    /// swap is kept with empty trade-info so an otherwise-valid event does not abort the pool sync.
    ///
    /// # Errors
    ///
    /// Returns an error if the swap event's block timestamp is missing from the cache.
    pub fn process_pool_swap_event(
        &self,
        swap_event: &SwapEvent,
        pool: &SharedPool,
    ) -> anyhow::Result<PoolSwap> {
        let timestamp = self
            .cache
            .get_block_timestamp(swap_event.block_number)
            .copied()
            .context("missing block timestamp for swap event")?;
        let mut swap = swap_event.to_pool_swap(
            self.chain.clone(),
            pool.instrument_id,
            pool.pool_identifier,
            timestamp,
        );
        // Keep the swap and leave price metadata empty rather than aborting the pool sync
        if let Err(e) = swap.calculate_trade_info(&pool.token0, &pool.token1, None) {
            log::warn!(
                "Skipping trade info for swap at block {} on pool {}: {e}",
                swap_event.block_number,
                pool.instrument_id,
            );
        }

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
            .copied()
            .context("missing block timestamp for mint event")?;

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
            .copied()
            .context("missing block timestamp for burn event")?;

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
            .copied()
            .context("missing block timestamp for collect event")?;

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
            .copied()
            .context("missing block timestamp for flash event")?;

        let flash = flash_event.to_pool_flash(self.chain.clone(), pool.instrument_id, timestamp);

        Ok(flash)
    }

    /// Synchronizes all pools and their tokens for a specific DEX within the given block range.
    ///
    /// This method performs a full sync of:
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
        self.register_dex(dex_id).await?;
        let _ = self.cache.load_pools(&dex_id).await?;
        Ok(())
    }

    /// Registers a decentralized exchange but loads only a single pool into the cache.
    ///
    /// Like [`Self::register_dex_exchange`], but loads just `pool_identifier` instead of the whole
    /// DEX pool set, so per-pool tools (e.g. `analyze-pool`) avoid the full pool-set load. A pool
    /// absent from the cache database is left for the caller's later lookup to report.
    ///
    /// # Errors
    ///
    /// Returns an error if DEX registration or the pool load fails.
    pub async fn register_dex_exchange_for_pool(
        &mut self,
        dex_id: DexType,
        pool_identifier: &PoolIdentifier,
    ) -> anyhow::Result<()> {
        self.register_dex(dex_id).await?;
        let _ = self.cache.load_pool(&dex_id, pool_identifier).await?;
        Ok(())
    }

    /// Registers a DEX in the cache and its event signatures for subscriptions, without loading pools.
    async fn register_dex(&mut self, dex_id: DexType) -> anyhow::Result<()> {
        let Some(dex_extended) = get_dex_extended(self.chain.name, &dex_id) else {
            anyhow::bail!("Unknown DEX {dex_id} on chain {}", self.chain.name);
        };

        log::info!("Registering DEX {dex_id} on chain {}", self.chain.name);
        self.cache.add_dex(dex_extended.dex.clone()).await?;
        self.subscription_manager.register_dex_for_subscriptions(
            dex_id,
            dex_extended.swap_created_event.as_ref(),
            dex_extended.mint_created_event.as_ref(),
            dex_extended.burn_created_event.as_ref(),
            dex_extended.collect_created_event.as_ref(),
            dex_extended.flash_created_event.as_deref(),
        );
        Ok(())
    }

    /// Bootstraps a [`PoolProfiler`] with the latest state for a given pool.
    ///
    /// Uses two paths depending on whether the pool has been synced to the database:
    /// - **Never synced**: Streams events from HyperSync, restores from on-chain RPC, returns `(profiler, true)`.
    /// - **Previously synced**: Syncs new events to DB, streams from DB, returns `(profiler, false)`.
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
        to_block: Option<u64>,
    ) -> anyhow::Result<(PoolProfiler, bool)> {
        log::info!(
            "Bootstrapping latest pool profiler for pool {}",
            pool.address
        );

        if self.cache.database.is_none() {
            anyhow::bail!(
                "Database is not initialized, so we cannot properly bootstrap the latest pool profiler"
            );
        }

        let to_block = match to_block {
            Some(block) => block,
            None => self.hypersync_client.current_block().await,
        };
        let mut profiler = PoolProfiler::new(pool.clone());

        // Calculate latest valid block position after which we need to start profiling.
        let from_position = match self
            .cache
            .database
            .as_ref()
            .unwrap()
            .load_latest_pool_snapshot(
                pool.chain.chain_id,
                &pool.pool_identifier,
                Some(to_block),
                true,
            )
            .await
        {
            Ok(Some(snapshot)) => {
                // Empty snapshots at the pool's creation block are stubs left behind by an
                // earlier bootstrap that bailed before any liquidity events landed. Restoring
                // marks the profiler as initialized, which then conflicts with the Initialize
                // event that hypersync re-emits at the same block. Fall through to a fresh
                // bootstrap rather than trust the stub.
                if snapshot.positions.is_empty()
                    && snapshot.ticks.is_empty()
                    && snapshot.block_position.number == pool.creation_block
                {
                    log::warn!(
                        "Ignoring empty stub snapshot at pool creation block {} for {}; rebuilding from events",
                        snapshot.block_position.number.separate_with_commas(),
                        pool.instrument_id,
                    );
                    None
                } else {
                    log::info!(
                        "Loaded valid snapshot from block {} which contains {} positions and {} ticks",
                        snapshot.block_position.number.separate_with_commas(),
                        snapshot.positions.len(),
                        snapshot.ticks.len()
                    );
                    let block_position = snapshot.block_position.clone();
                    profiler.restore_from_snapshot(snapshot)?;
                    log::info!("Restored profiler from snapshot");
                    Some(block_position)
                }
            }
            _ => {
                log::info!("No valid snapshot found, processing from beginning");
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
                .construct_pool_profiler_from_hypersync_rpc(profiler, from_position, to_block)
                .await;
        }

        // Sync the pool events before bootstrapping of pool profiler
        self.sync_pool_events(
            &pool.dex.name,
            pool.pool_identifier,
            None,
            Some(to_block),
            false,
        )
        .await
        .context("failed to sync pool events for snapshot request")?;

        if !profiler.is_initialized {
            if let Some(initial_sqrt_price_x96) = pool.initial_sqrt_price_x96 {
                profiler.initialize(initial_sqrt_price_x96)?;
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
        let total_blocks = to_block.saturating_sub(from_block) + 1;

        // Enable embedded profiler reporting
        profiler.enable_reporting(from_block, total_blocks, BLOCKS_PROCESS_IN_SYNC_REPORT);

        let mut stream = self.cache.database.as_ref().unwrap().stream_pool_events(
            pool.chain.clone(),
            pool.dex.clone(),
            pool.instrument_id,
            pool.pool_identifier,
            from_position.clone(),
            Some(to_block),
        );

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    profiler.process(&event)?;
                }
                Err(e) => return Err(e).context("failed to stream pool event from database"),
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
        &mut self,
        mut profiler: PoolProfiler,
        from_position: Option<BlockPosition>,
        to_block: u64,
    ) -> anyhow::Result<(PoolProfiler, bool)> {
        log::info!("Constructing pool profiler from hypersync stream and RPC final state querying");
        let dex_extended = self.get_dex_extended(&profiler.pool.dex.name)?.clone();
        let mint_event_signature = dex_extended.mint_created_event.as_ref();
        let burn_event_signature = dex_extended.burn_created_event.as_ref();
        let initialize_event_signature =
            if let Some(initialize_event) = &dex_extended.initialize_event {
                initialize_event.as_ref()
            } else {
                anyhow::bail!(
                    "DEX {} does not have initialize event set.",
                    profiler.pool.dex.name
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
        let total_blocks = to_block.saturating_sub(from_block) + 1;

        log::info!(
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
                Some(to_block),
                &profiler.pool.address,
                vec![
                    mint_event_signature,
                    burn_event_signature,
                    initialize_event_signature,
                ],
            )
            .await;
        tokio::pin!(pool_events_stream);
        let mut block_batch: Vec<Block> = Vec::with_capacity(POOL_EVENT_BLOCK_BATCH_SIZE);

        while let Some(item) = pool_events_stream.next().await {
            let log = match item {
                PoolEventStreamItem::Block(block) => {
                    self.record_pool_event_block(block, &mut block_batch)
                        .await?;
                    continue;
                }
                PoolEventStreamItem::Log(log) => log,
            };
            let event_sig_bytes = extract_event_signature_bytes(&log)?;

            if event_sig_bytes == initialize_sig_bytes {
                if profiler.is_initialized {
                    // Profiler was restored from a snapshot at or after this block; the
                    // initialize state is already in place. Skip the re-init that would
                    // otherwise trip AlreadyInitialized.
                    log::debug!(
                        "Profiler already initialized; skipping Initialize event at block {}",
                        extract_block_number(&log)?.separate_with_commas(),
                    );
                } else {
                    let initialize_event = dex_extended.parse_initialize_event_hypersync(&log)?;
                    profiler.initialize(initialize_event.sqrt_price_x96)?;
                    self.cache
                        .database
                        .as_ref()
                        .unwrap()
                        .update_pool_initial_price_tick(self.chain.chain_id, &initialize_event)
                        .await?;
                }
            } else if event_sig_bytes == mint_sig_bytes {
                let mint_event = dex_extended.parse_mint_event_hypersync(&log)?;
                let liquidity_update = self
                    .process_pool_mint_event(&mint_event, &profiler.pool, &dex_extended)
                    .with_context(|| {
                        format!(
                            "failed to process mint event at block {}",
                            mint_event.block_number
                        )
                    })?;
                profiler.process(&DexPoolData::LiquidityUpdate(liquidity_update))?;
            } else if event_sig_bytes == burn_sig_bytes {
                let burn_event = dex_extended.parse_burn_event_hypersync(&log)?;
                let liquidity_update = self
                    .process_pool_burn_event(&burn_event, &profiler.pool, &dex_extended)
                    .with_context(|| {
                        format!(
                            "failed to process burn event at block {}",
                            burn_event.block_number
                        )
                    })?;
                profiler.process(&DexPoolData::LiquidityUpdate(liquidity_update))?;
            } else {
                let event_signature = hex::encode(event_sig_bytes);
                anyhow::bail!(
                    "unexpected event signature in bootstrap_latest_pool_profiler: {event_signature} for log {log:?}"
                );
            }
        }

        self.flush_pool_event_blocks(&mut block_batch).await?;
        profiler.finalize_reporting();

        let on_chain_snapshot = self
            .get_on_chain_snapshot(&profiler)
            .await
            .with_context(|| {
                let snapshot_block = profiler
                    .last_processed_event
                    .as_ref()
                    .map_or(profiler.pool.creation_block, |event| event.number);

                format!(
                    "failed to restore pool {} from on-chain snapshot at block {} with {} ticks and {} positions",
                    profiler.pool.address,
                    snapshot_block.separate_with_commas(),
                    profiler.get_active_tick_values().len().separate_with_commas(),
                    profiler.get_all_position_keys().len().separate_with_commas()
                )
            })?;
        profiler.restore_from_snapshot(on_chain_snapshot)?;

        Ok((profiler, true))
    }

    /// Validates a pool profiler's state against on-chain data for accuracy verification.
    ///
    /// This method performs integrity checking by comparing the profiler's internal state
    /// (positions, ticks, liquidity) with the actual on-chain smart contract state. For UniswapV3
    /// pools, it fetches current on-chain data and verifies that the profiler's tracked state matches.
    /// Returns [`SnapshotValidation::OnChain`] when the profiler matches on-chain state,
    /// [`SnapshotValidation::Invalid`] when it does not, and [`SnapshotValidation::Replay`] when the
    /// on-chain state could not be fetched (e.g. a non-archive RPC for a historical block); in the
    /// last case the replay-derived snapshot is kept. The resolved state is persisted for the
    /// `OnChain` and `Invalid` outcomes; `Replay` leaves the snapshot at its inserted default so a
    /// transient RPC failure cannot clobber a prior definitive verdict.
    ///
    /// # Errors
    ///
    /// Returns an error if database operations fail when persisting the validation state.
    ///
    /// # Panics
    ///
    /// Panics if the profiler does not have a last_processed_event when already_validated is true.
    pub async fn check_snapshot_validity(
        &self,
        profiler: &PoolProfiler,
        already_validated: bool,
    ) -> anyhow::Result<SnapshotValidation> {
        let (validation, block_position) = if already_validated {
            // Skip RPC call - profiler was validated during construction from RPC
            log::info!("Snapshot already validated from RPC, skipping on-chain comparison");
            let last_event = profiler
                .last_processed_event
                .clone()
                .expect("Profiler should have last_processed_event");
            (SnapshotValidation::OnChain, Some(last_event))
        } else {
            // Fetch on-chain state and compare
            match self.get_on_chain_snapshot(profiler).await {
                Ok(on_chain_snapshot) => {
                    log::info!("Comparing profiler state with on-chain state...");
                    let comparison = compare_pool_profiler_detailed(profiler, &on_chain_snapshot);
                    let validation = if comparison.is_valid_for_snapshot() {
                        if !comparison.is_exact_match() {
                            log::warn!(
                                "Pool profiler snapshot has a non-structural mismatch (sqrt ratio or fee protocol); accepting snapshot"
                            );
                        }
                        SnapshotValidation::OnChain
                    } else {
                        log::error!(
                            "Pool profiler state does NOT match on-chain smart contract state"
                        );
                        SnapshotValidation::Invalid
                    };
                    (validation, Some(on_chain_snapshot.block_position))
                }
                Err(e) => {
                    log::warn!(
                        "Could not validate snapshot against on-chain state, keeping replay-derived snapshot: {e}"
                    );
                    // RPC could not reach the block. Report any stored verdict so stdout agrees with
                    // a pre-existing on_chain/invalid row; the None block position below skips the
                    // persist step, so a transient failure cannot clobber that verdict.
                    let reported = self
                        .stored_snapshot_validation(profiler)
                        .await?
                        .unwrap_or(SnapshotValidation::Replay);
                    (reported, None)
                }
            }
        };

        if let (Some(block_position), Some(cache_database)) = (block_position, &self.cache.database)
        {
            cache_database
                .set_pool_snapshot_validation_state(
                    profiler.pool.chain.chain_id,
                    &profiler.pool.pool_identifier,
                    block_position.number,
                    block_position.transaction_index,
                    block_position.log_index,
                    validation.as_str(),
                )
                .await?;
            log::info!(
                "Set pool snapshot validation state to {}",
                validation.as_str()
            );
        }

        Ok(validation)
    }

    /// Reads the persisted [`SnapshotValidation`] for the profiler's current snapshot watermark.
    ///
    /// Returns `None` when no database is configured, the profiler has no processed event, or no
    /// snapshot row exists at that watermark.
    async fn stored_snapshot_validation(
        &self,
        profiler: &PoolProfiler,
    ) -> anyhow::Result<Option<SnapshotValidation>> {
        let (Some(block_position), Some(cache_database)) =
            (profiler.last_processed_event.as_ref(), &self.cache.database)
        else {
            return Ok(None);
        };

        let stored = cache_database
            .get_pool_snapshot_validation_state(
                profiler.pool.chain.chain_id,
                &profiler.pool.pool_identifier,
                block_position.number,
                block_position.transaction_index,
                block_position.log_index,
            )
            .await?;

        Ok(stored.and_then(|token| SnapshotValidation::from_db_token(&token)))
    }

    /// Fetches current on-chain pool state at the last processed block.
    ///
    /// Queries the pool smart contract to retrieve active tick liquidity and position data,
    /// using the profiler's active positions and last processed block number.
    /// Used for profiler state restoration after bootstrapping and validation.
    async fn get_on_chain_snapshot(&self, profiler: &PoolProfiler) -> anyhow::Result<PoolSnapshot> {
        // PancakeSwap V3 shares the Uniswap V3 pool read ABI, so it hydrates through the same contract
        if matches!(
            profiler.pool.dex.name,
            DexType::UniswapV3 | DexType::PancakeSwapV3
        ) {
            let last_processed_event = Self::last_processed_event_for_on_chain_snapshot(profiler)?;
            let timestamp = Self::timestamp_for_on_chain_snapshot(
                profiler,
                self.cache
                    .get_block_timestamp(last_processed_event.number)
                    .copied(),
            )?;
            let on_chain_snapshot = self
                .univ3_pool
                .fetch_snapshot(
                    &profiler.pool.address,
                    profiler.pool.instrument_id,
                    profiler.get_active_tick_values().as_slice(),
                    &profiler.get_all_position_keys(),
                    last_processed_event,
                    timestamp, // ts_event
                    timestamp, // ts_init (same block timestamp)
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

    fn timestamp_for_on_chain_snapshot(
        profiler: &PoolProfiler,
        cached_timestamp: Option<UnixNanos>,
    ) -> anyhow::Result<UnixNanos> {
        if let Some(timestamp) = cached_timestamp {
            return Ok(timestamp);
        }

        profiler
            .last_processed_ts
            .context("missing block timestamp for on-chain snapshot")
    }

    fn last_processed_event_for_on_chain_snapshot(
        profiler: &PoolProfiler,
    ) -> anyhow::Result<BlockPosition> {
        let Some(last_processed_event) = profiler.last_processed_event.clone() else {
            anyhow::bail!(
                "cannot fetch on-chain snapshot for pool {} without a processed event",
                profiler.pool.address
            );
        };
        Ok(last_processed_event)
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
            log::info!(
                "Replaying historical events for pool {} to hydrate profiler",
                pool.instrument_id
            );

            let mut event_stream = database.stream_pool_events(
                self.chain.clone(),
                dex.clone(),
                pool.instrument_id,
                pool.pool_identifier,
                None,
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
                        log::error!("Error streaming event for pool {}: {e}", pool.instrument_id);
                    }
                }
            }

            log::info!(
                "Replayed {event_count} historical events for pool {}",
                pool.instrument_id
            );
        } else {
            log::debug!(
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
            log::debug!("Sending {data}");

            if let Err(e) = data_tx.send(data) {
                log::error!("Failed to send data: {e}");
            }
        } else {
            log::error!("No data event channel for sending data");
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

#[cfg(test)]
mod tests {
    use alloy::primitives::{U160, address};
    use nautilus_core::UnixNanos;
    use nautilus_model::defi::{Chain, Token};
    use rstest::rstest;
    use tokio_util::sync::CancellationToken;

    use super::*;

    const WETH_USDT_POOL: &str = "0x4e68ccd3e89f51c3074ca5072bbac773960dfa36";
    const WETH_USDT_CREATION_BLOCK: u64 = 12_375_326;

    #[rstest]
    #[case(SnapshotValidation::OnChain, "on_chain", true)]
    #[case(SnapshotValidation::Replay, "replay", true)]
    #[case(SnapshotValidation::Invalid, "invalid", false)]
    fn snapshot_validation_db_token_and_usability(
        #[case] validation: SnapshotValidation,
        #[case] expected_str: &str,
        #[case] expected_usable: bool,
    ) {
        // as_str must match the pool_snapshot.validation_state CHECK values and the JSON contract;
        // is_usable must match the load filter `validation_state <> 'invalid'`.
        assert_eq!(validation.as_str(), expected_str);
        assert_eq!(validation.is_usable(), expected_usable);
        // from_db_token round-trips a stored token back to the enum, so a read-back verdict
        // reports the same state that was persisted.
        assert_eq!(
            SnapshotValidation::from_db_token(expected_str),
            Some(validation)
        );
    }

    #[rstest]
    fn snapshot_validation_from_db_token_rejects_unknown() {
        assert_eq!(SnapshotValidation::from_db_token("bogus"), None);
    }

    #[rstest]
    fn last_processed_event_for_on_chain_snapshot_rejects_unprocessed_profiler() {
        let mut profiler = PoolProfiler::new(weth_usdt_pool());
        profiler
            .initialize(U160::from_str_radix("3cb0adde486484998be0b", 16).unwrap())
            .expect("Known WETH/USDT initial sqrt price should initialize");

        let error = BlockchainDataClientCore::last_processed_event_for_on_chain_snapshot(&profiler)
            .expect_err("unprocessed profiler should not fetch on-chain state");

        assert_eq!(
            error.to_string(),
            format!(
                "cannot fetch on-chain snapshot for pool {} without a processed event",
                profiler.pool.address
            )
        );
    }

    #[rstest]
    fn timestamp_for_on_chain_snapshot_prefers_cached_block_timestamp() {
        let pool = weth_usdt_pool();
        let mut profiler = PoolProfiler::new(pool);
        let cached_ts = UnixNanos::from(1_700_000_001_000_000_000);
        let profiler_ts = UnixNanos::from(1_700_000_000_000_000_000);
        profiler.last_processed_ts = Some(profiler_ts);

        let timestamp =
            BlockchainDataClientCore::timestamp_for_on_chain_snapshot(&profiler, Some(cached_ts))
                .unwrap();

        assert_eq!(timestamp, cached_ts);
    }

    #[rstest]
    fn timestamp_for_on_chain_snapshot_falls_back_to_profiler_timestamp() {
        let pool = weth_usdt_pool();
        let mut profiler = PoolProfiler::new(pool);
        let profiler_ts = UnixNanos::from(1_700_000_000_000_000_000);
        profiler.last_processed_ts = Some(profiler_ts);

        let timestamp =
            BlockchainDataClientCore::timestamp_for_on_chain_snapshot(&profiler, None).unwrap();

        assert_eq!(timestamp, profiler_ts);
    }

    #[rstest]
    fn timestamp_for_on_chain_snapshot_rejects_missing_timestamp() {
        let pool = weth_usdt_pool();
        let profiler = PoolProfiler::new(pool);

        let error = BlockchainDataClientCore::timestamp_for_on_chain_snapshot(&profiler, None)
            .expect_err("missing timestamps should fail");

        assert_eq!(
            error.to_string(),
            "missing block timestamp for on-chain snapshot"
        );
    }

    #[rstest]
    #[case(100, 50, Some(99))]
    #[case(50, 50, None)]
    #[case(0, 0, None)]
    fn completed_pool_event_checkpoint_excludes_in_flight_block(
        #[case] block_number: u64,
        #[case] effective_from_block: u64,
        #[case] expected: Option<u64>,
    ) {
        let checkpoint = BlockchainDataClientCore::completed_pool_event_checkpoint(
            block_number,
            effective_from_block,
        );

        assert_eq!(checkpoint, expected);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires ENVIO_API_TOKEN and live HyperSync access"]
    async fn live_hypersync_bootstrap_fails_closed_when_rpc_hydration_fails() {
        std::env::var("ENVIO_API_TOKEN").expect("ENVIO_API_TOKEN must be set");

        let pool = weth_usdt_pool();
        let chain = Arc::new(
            Chain::from_chain_id(1)
                .expect("Ethereum chain should exist")
                .clone(),
        );
        let dex = get_dex_extended(chain.name, &DexType::UniswapV3)
            .expect("Ethereum UniswapV3 should be registered")
            .dex
            .clone();
        let (hypersync_tx, _hypersync_rx) = tokio::sync::mpsc::unbounded_channel();
        let config = BlockchainDataClientConfig::builder()
            .chain(chain)
            .dex_ids(vec![DexType::UniswapV3])
            .http_rpc_url("http://127.0.0.1:9".to_string())
            .use_hypersync_for_live_data(true)
            .maybe_from_block(Some(WETH_USDT_CREATION_BLOCK))
            .build();
        let mut core = BlockchainDataClientCore::new(
            config,
            Some(hypersync_tx),
            None,
            CancellationToken::new(),
        );
        core.cache
            .add_dex(dex)
            .await
            .expect("DEX should be added to in-memory cache");

        let block_position = BlockPosition::new(
            WETH_USDT_CREATION_BLOCK,
            "0x2e07c690f149223e4f290986277304ea6a05c6ee47ba303732166bc1b15cbafb".to_string(),
            11,
            27,
        );
        let mut profiler = PoolProfiler::new(pool);
        profiler
            .initialize(U160::from_str_radix("3cb0adde486484998be0b", 16).unwrap())
            .expect("Known WETH/USDT initial sqrt price should initialize");
        profiler.last_processed_event = Some(block_position.clone());

        let result = core
            .construct_pool_profiler_from_hypersync_rpc(
                profiler,
                Some(block_position),
                WETH_USDT_CREATION_BLOCK,
            )
            .await;

        let error = result.expect_err("RPC hydration failure should fail closed");
        let error_message = format!("{error:?}");
        assert!(
            error_message.contains("failed to restore pool"),
            "hydration error should include pool context, was {error_message}"
        );
        assert!(
            error_message.to_lowercase().contains(WETH_USDT_POOL),
            "hydration error should include pool address, was {error_message}"
        );
    }

    fn weth_usdt_pool() -> SharedPool {
        let chain = Arc::new(
            Chain::from_chain_id(1)
                .expect("Ethereum chain should exist")
                .clone(),
        );
        let dex = get_dex_extended(chain.name, &DexType::UniswapV3)
            .expect("Ethereum UniswapV3 should be registered")
            .dex
            .clone();
        let pool_address = address!("4e68ccd3e89f51c3074ca5072bbac773960dfa36");
        let token0 = Token::new(
            chain.clone(),
            address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
            "Wrapped Ether".to_string(),
            "WETH".to_string(),
            18,
        );
        let token1 = Token::new(
            chain.clone(),
            address!("dAC17F958D2ee523a2206206994597C13D831ec7"),
            "Tether USD".to_string(),
            "USDT".to_string(),
            6,
        );

        Arc::new(Pool::new(
            chain,
            dex,
            pool_address,
            PoolIdentifier::from_address(pool_address),
            WETH_USDT_CREATION_BLOCK,
            token0,
            token1,
            Some(3_000),
            Some(60),
            UnixNanos::default(),
        ))
    }
}
