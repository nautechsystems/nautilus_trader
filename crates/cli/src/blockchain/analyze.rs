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

use alloy_primitives::Address;
use futures_util::StreamExt;
use nautilus_blockchain::{
    config::BlockchainDataClientConfig,
    contracts::uniswap_v3_pool::UniswapV3PoolContract,
    data::core::BlockchainDataClientCore,
    exchanges::{find_dex_type_case_insensitive, get_supported_dexes_for_chain},
    rpc::http::BlockchainHttpRpcClient,
};
use nautilus_infrastructure::sql::pg::get_postgres_connect_options;
use nautilus_model::defi::{
    DexType,
    chain::Chain,
    pool_analysis::{compare::compare_pool_profiler, profiler::PoolProfiler},
    validation::validate_address,
};

use crate::opt::DatabaseConfig;

/// Runs pool analysis for the specified chain, DEX, and pool address.
///
/// # Errors
///
/// Returns an error if the chain or DEX parameters are invalid.
pub async fn run_analyze_pool(
    chain: String,
    dex: String,
    pool_address: String,
    from_block: Option<u64>,
    to_block: Option<u64>,
    rpc_url: Option<String>,
    database: DatabaseConfig,
    reset: bool,
    multicall_calls_per_rpc_request: Option<u32>,
) -> anyhow::Result<()> {
    let chain = Chain::from_chain_name(&chain)
        .ok_or_else(|| anyhow::anyhow!("Invalid chain name: {}", chain))?;
    let pool_address = validate_address(&pool_address)?;

    let dex_type = find_dex_type_case_insensitive(&dex, chain).ok_or_else(|| {
        let supported_dexes = get_supported_dexes_for_chain(chain.name);
        if supported_dexes.is_empty() {
            anyhow::anyhow!(
                "Invalid DEX name '{}' (case-insensitive). Chain '{}' is not supported for pool analysis.",
                dex, chain.name
            )
        } else {
            anyhow::anyhow!(
                "Invalid DEX name '{}' (case-insensitive). Supported DEXes for chain '{}': {}",
                dex,
                chain.name,
                supported_dexes.join(", ")
            )
        }
    })?;

    let postgres_connect_options = get_postgres_connect_options(
        database.host,
        database.port,
        database.username,
        database.password,
        database.database,
    );
    // Get RPC HTTP URL from CLI argument or environment variable
    let rpc_http_url = rpc_url
        .or_else(|| std::env::var("RPC_HTTP_URL").ok())
        .unwrap_or_default();

    log::info!("Using RPC HTTP URL: '{}'", rpc_http_url);
    if rpc_http_url.is_empty() {
        log::warn!(
            "No RPC HTTP URL provided via --rpc-url or RPC_HTTP_URL environment variable - some operations may fail"
        );
    }

    let config = BlockchainDataClientConfig::new(
        Arc::new(chain.to_owned()),
        vec![dex_type],
        rpc_http_url,
        None,
        multicall_calls_per_rpc_request,
        None,
        true,
        None,
        None,
        Some(postgres_connect_options),
    );
    let mut data_client = BlockchainDataClientCore::new(config, None, None);
    let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
        data_client.config.http_rpc_url.clone(),
        data_client.config.rpc_requests_per_second,
    ));
    data_client.initialize_cache_database().await;
    data_client.cache.initialize_chain().await;
    data_client
        .register_dex_exchange(dex_type)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to register DEX exchange: {}", e))?;
    data_client
        .sync_pool_events(&dex_type, pool_address, from_block, to_block, reset)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to sync pool events: {}", e))?;

    // Profile pool events from database
    log::info!("Profiling pool events from database...");

    let pool = data_client.get_pool(&pool_address)?;
    let mut profiler = PoolProfiler::new(pool.clone());

    // Try to restore from latest valid snapshot
    let from_position = if let Some(cache_database) = &data_client.cache.database {
        match cache_database
            .load_latest_valid_pool_snapshot(pool.chain.chain_id, &pool_address)
            .await
        {
            Ok(Some(snapshot)) => {
                log::info!(
                    "Loaded valid snapshot from block {}",
                    snapshot.block_position.number
                );
                log::info!(
                    "Snapshot contains {} positions and {} ticks",
                    snapshot.positions.len(),
                    snapshot.ticks.len()
                );

                let block_position = snapshot.block_position.clone();
                profiler.restore_from_snapshot(snapshot)?;
                log::info!("Restored profiler from snapshot");
                Some(block_position)
            }
            Ok(None) => {
                log::info!("No valid snapshot found, processing from beginning");
                let initial_sqrt_price_x96 = pool
                    .initial_sqrt_price_x96
                    .expect("Pool has no initial sqrt price");
                profiler.initialize(initial_sqrt_price_x96);
                None
            }
            Err(e) => {
                log::warn!("Failed to load snapshot: {}, processing from beginning", e);
                let initial_sqrt_price_x96 = pool
                    .initial_sqrt_price_x96
                    .expect("Pool has no initial sqrt price");
                profiler.initialize(initial_sqrt_price_x96);
                None
            }
        }
    } else {
        let initial_sqrt_price_x96 = pool
            .initial_sqrt_price_x96
            .expect("Pool has no initial sqrt price");
        profiler.initialize(initial_sqrt_price_x96);
        None
    };

    // Stream and process events
    if let Some(cache_database) = &data_client.cache.database {
        // Log streaming start position
        if let Some(pos) = &from_position {
            log::info!("Streaming pool events from block {}", pos.number);
        } else {
            log::info!("Streaming pool events from genesis");
        }

        let mut stream = cache_database.stream_pool_events(
            pool.chain.clone(),
            pool.dex.clone(),
            pool.instrument_id,
            &pool_address,
            from_position.clone(),
        );

        #[cfg(debug_assertions)]
        let total_start = std::time::Instant::now();
        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    profiler.process(&event)?;
                }
                Err(e) => log::error!("Error processing event: {}", e),
            }
        }

        #[cfg(debug_assertions)]
        {
            let total_time = total_start.elapsed();
            let processing_time = profiler.get_total_processing_time();
            let streaming_time = total_time - processing_time;
            // Log performance report
            profiler.log_performance_report(total_time, streaming_time);
        }
    }

    let snapshot = profiler.extract_snapshot();

    // Save complete pool snapshot to database (includes state, positions, and ticks)
    log::info!(
        "Saving pool snapshot with {} positions and {} ticks to database...",
        snapshot.positions.len(),
        snapshot.ticks.len()
    );
    data_client
        .cache
        .add_pool_snapshot(&pool.address, &snapshot)
        .await?;
    log::info!("Saved complete pool snapshot to database");

    if dex_type == DexType::UniswapV3 {
        log::info!("Comparing profiler state with on-chain state...");
        let pool_contract = UniswapV3PoolContract::new(http_rpc_client.clone());

        // Prepare data for snapshot fetch
        let tick_values = profiler.get_active_tick_values();
        let position_keys: Vec<(Address, i32, i32)> = profiler
            .get_active_positions()
            .iter()
            .map(|position| (position.owner, position.tick_lower, position.tick_upper))
            .collect();

        // Fetch on-chain snapshot at the snapshot's block position for accurate validation
        // This requires an archive node to query historical state
        let snapshot_block = Some(snapshot.block_position.number);
        log::info!(
            "Fetching on-chain state at block {} for validation (requires archive node)",
            snapshot.block_position.number
        );
        let on_chain_snapshot = pool_contract
            .fetch_snapshot(
                &pool_address,
                pool.instrument_id,
                &tick_values,
                &position_keys,
                snapshot_block,
            )
            .await?;
        let result = compare_pool_profiler(&profiler, &on_chain_snapshot);

        if result {
            log::info!("✅  Pool profiler state matches on-chain smart contract state.");

            // Mark the snapshot as valid since verification passed
            if let Some(cache_database) = &data_client.cache.database {
                cache_database
                    .mark_pool_snapshot_valid(
                        pool.chain.chain_id,
                        &pool.address,
                        snapshot.block_position.number,
                        snapshot.block_position.transaction_index,
                        snapshot.block_position.log_index,
                    )
                    .await?;
                log::info!("Marked pool profiler snapshot as valid");
            }
        } else {
            log::error!("❌  Pool profiler state does NOT match on-chain smart contract state");
        }
    }

    Ok(())
}
