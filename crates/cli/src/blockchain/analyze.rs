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

use nautilus_blockchain::{
    config::BlockchainDataClientConfig,
    data::core::BlockchainDataClientCore,
    exchanges::{find_dex_type_case_insensitive, get_supported_dexes_for_chain},
    rpc::providers::check_infura_rpc_provider,
};
use nautilus_infrastructure::sql::pg::get_postgres_connect_options;
use nautilus_model::defi::{PoolIdentifier, chain::Chain, validation::validate_address};
use ustr::Ustr;

use crate::opt::DatabaseConfig;

/// Runs pool analysis for the specified chain, DEX, and pool address.
///
/// # Errors
///
/// Returns an error if the chain or DEX parameters are invalid.
#[allow(clippy::too_many_arguments)]
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
        .ok_or_else(|| anyhow::anyhow!("Invalid chain name: {chain}"))?;
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
    // Get RPC HTTP URL: CLI arg, Infura provider, OR RPC_HTTP_URL env var
    let rpc_http_url = rpc_url
        .or_else(|| check_infura_rpc_provider(&chain.name))
        .or_else(|| std::env::var("RPC_HTTP_URL").ok())
        .unwrap_or_else(|| {
            panic!(
                "No RPC URL provided for {}. Set --rpc-url, INFURA_API_KEY, or RPC_HTTP_URL",
                chain.name
            )
        });

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
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let mut data_client = BlockchainDataClientCore::new(config, None, None, cancellation_token);
    data_client.initialize_cache_database().await;
    data_client.cache.initialize_chain().await;
    data_client
        .register_dex_exchange(dex_type)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to register DEX exchange: {e}"))?;

    let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));
    data_client
        .sync_pool_events(&dex_type, pool_identifier, from_block, to_block, reset)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to sync pool events: {e}"))?;

    // Profile pool events from database
    log::info!("Profiling pool events from database...");
    let pool = data_client
        .cache
        .get_pool(&pool_identifier)
        .expect("Pool not found in cache")
        .clone();
    let (profiler, already_valid) = data_client.bootstrap_latest_pool_profiler(&pool).await?;
    let snapshot = profiler.extract_snapshot();

    // Save complete pool snapshot to database (includes state, positions, and ticks)
    log::info!(
        "Saving pool snapshot with {} positions and {} ticks to database...",
        snapshot.positions.len(),
        snapshot.ticks.len()
    );
    data_client
        .cache
        .add_pool_snapshot(&pool.dex.name, &pool.pool_identifier, &snapshot)
        .await?;
    log::info!("Saved complete pool snapshot to database");
    data_client
        .check_snapshot_validity(&profiler, already_valid)
        .await?;
    log::info!(
        "Pool liquidity utilization rate is {:.4}%",
        profiler.liquidity_utilization_rate() * 100.0
    );
    Ok(())
}
