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
    // Get pool details from data client
    let pool = data_client.get_pool(&pool_address)?;

    // Create profiler and reporter
    let mut profiler = PoolProfiler::new(pool.clone());
    let initial_sqrt_price_x96 = pool
        .initial_sqrt_price_x96
        .expect("Pool has no initial sqrt price");
    profiler.initialize(initial_sqrt_price_x96);

    // Stream and process events
    if let Some(cache_database) = &data_client.cache.database {
        let mut stream =
            cache_database.stream_pool_events(pool.chain.clone(), pool.dex.clone(), &pool_address);

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    profiler.process(&event)?;
                }
                Err(e) => log::error!("Error processing event: {}", e),
            }
        }
    }

    if dex_type == DexType::UniswapV3 {
        let pool_contract = UniswapV3PoolContract::new(http_rpc_client.clone());

        let on_chain_state = pool_contract.get_global_state(&pool_address).await?;
        let on_chain_ticks = pool_contract
            .batch_get_ticks(&pool_address, &profiler.get_active_tick_values())
            .await?;
        let position_keys: Vec<(Address, i32, i32)> = profiler
            .get_active_positions()
            .iter()
            .map(|position| (position.owner, position.tick_lower, position.tick_upper))
            .collect();
        let on_chain_positions = pool_contract
            .batch_get_positions(&pool_address, &position_keys)
            .await?;
        let result = compare_pool_profiler(
            &profiler,
            on_chain_state.tick,
            on_chain_state.sqrt_price_x96,
            on_chain_state.fee_protocol,
            on_chain_state.liquidity,
            on_chain_ticks,
            on_chain_positions,
        );

        if result {
            log::info!("✅  Pool profiler state matches on-chain smart contract state.");
        } else {
            log::error!("❌  Pool profiler state does NOT match on-chain smart contract state");
        }
    }

    Ok(())
}
