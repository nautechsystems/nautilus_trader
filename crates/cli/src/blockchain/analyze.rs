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

use std::{fs, sync::Arc};

use nautilus_blockchain::{
    config::BlockchainDataClientConfig,
    data::core::BlockchainDataClientCore,
    exchanges::{find_dex_type_case_insensitive, get_supported_dexes_for_chain},
    rpc::providers::check_infura_rpc_provider,
};
use nautilus_infrastructure::sql::pg::get_postgres_connect_options;
use nautilus_model::defi::{
    DexType, PoolIdentifier, chain::Chain, data::block::BlockPosition, validation::validate_address,
};
use serde_json::json;
use ustr::Ustr;

use crate::opt::DatabaseConfig;

/// Runs pool analysis for the specified chain, DEX, and pool address.
///
/// # Errors
///
/// Returns an error if the chain or DEX parameters are invalid.
#[expect(clippy::too_many_arguments)]
pub(crate) async fn run_analyze_pool(
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
    let (chain, dex_type) = parse_chain_dex(&chain, &dex)?;
    let mut data_client = create_data_client(
        chain,
        dex_type,
        rpc_url,
        database,
        multicall_calls_per_rpc_request,
    )
    .await?;
    let to_block = resolve_to_block(&data_client, to_block).await;

    analyze_pool_with_client(
        &mut data_client,
        dex_type,
        pool_address,
        from_block,
        to_block,
        reset,
    )
    .await?;

    Ok(())
}

/// Runs pool analysis for several pool addresses in one initialized runtime.
///
/// # Errors
///
/// Returns an error if chain, DEX, database, RPC, or address file setup fails. Individual pool
/// failures are emitted as structured output and the command returns an error after all pools run.
#[expect(clippy::too_many_arguments)]
pub(crate) async fn run_analyze_pools(
    chain: String,
    dex: String,
    addresses: Vec<String>,
    addresses_file: Option<String>,
    from_block: Option<u64>,
    to_block: Option<u64>,
    rpc_url: Option<String>,
    database: DatabaseConfig,
    reset: bool,
    multicall_calls_per_rpc_request: Option<u32>,
) -> anyhow::Result<()> {
    let pool_addresses = load_pool_addresses(addresses, addresses_file)?;
    let (chain, dex_type) = parse_chain_dex(&chain, &dex)?;
    let mut data_client = create_data_client(
        chain.clone(),
        dex_type,
        rpc_url,
        database,
        multicall_calls_per_rpc_request,
    )
    .await?;
    let to_block = resolve_to_block(&data_client, to_block).await;
    let chain_name = chain.name.to_string();
    let dex_name = dex_type.to_string();

    let mut failures = 0usize;

    for pool_address in pool_addresses {
        let result = analyze_pool_with_client(
            &mut data_client,
            dex_type,
            pool_address.clone(),
            from_block,
            to_block,
            reset,
        )
        .await;

        match result {
            Ok(outcome) => {
                println!("{}", outcome.to_json(&chain_name, &dex_name));
            }
            Err(e) => {
                failures += 1;
                println!(
                    "{}",
                    json!({
                        "chain": chain_name.as_str(),
                        "dex": dex_name.as_str(),
                        "pool_address": pool_address,
                        "target_block": to_block,
                        "status": "failure",
                        "error": e.to_string(),
                    })
                );
            }
        }
    }

    if failures > 0 {
        anyhow::bail!("Pool analysis failed for {failures} pool(s)");
    }

    Ok(())
}

async fn analyze_pool_with_client(
    data_client: &mut BlockchainDataClientCore,
    dex_type: DexType,
    pool_address: String,
    from_block: Option<u64>,
    to_block: u64,
    reset: bool,
) -> anyhow::Result<PoolAnalysisOutcome> {
    let pool_address = validate_address(&pool_address)?;
    let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));

    data_client
        .sync_pool_events(
            &dex_type,
            pool_identifier,
            from_block,
            Some(to_block),
            reset,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to sync pool events: {e}"))?;

    log::info!("Profiling pool events from database...");
    let pool = data_client
        .cache
        .get_pool(&pool_identifier)
        .ok_or_else(|| anyhow::anyhow!("Pool {pool_identifier} not found in cache"))?
        .clone();
    let (profiler, already_valid) = data_client
        .bootstrap_latest_pool_profiler(&pool, Some(to_block))
        .await?;
    let snapshot = profiler.extract_snapshot();
    let snapshot_block_position = snapshot.block_position.clone();
    let positions = snapshot.positions.len();
    let ticks = snapshot.ticks.len();

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
    let valid = data_client
        .check_snapshot_validity(&profiler, already_valid)
        .await?;
    let liquidity_utilization_rate = profiler.liquidity_utilization_rate();
    log::info!(
        "Pool liquidity utilization rate is {:.4}%",
        liquidity_utilization_rate * 100.0
    );

    Ok(PoolAnalysisOutcome {
        pool_address: pool_address.to_string(),
        target_block: to_block,
        snapshot_block_position,
        positions,
        ticks,
        valid,
        already_valid,
        liquidity_utilization_rate,
    })
}

async fn create_data_client(
    chain: Chain,
    dex_type: DexType,
    rpc_url: Option<String>,
    database: DatabaseConfig,
    multicall_calls_per_rpc_request: Option<u32>,
) -> anyhow::Result<BlockchainDataClientCore> {
    let postgres_connect_options = get_postgres_connect_options(
        database.host,
        database.port,
        database.username,
        database.password,
        database.database,
    );
    let rpc_http_url = rpc_http_url(&chain, rpc_url)?;

    let config = BlockchainDataClientConfig::builder()
        .chain(Arc::new(chain.clone()))
        .dex_ids(vec![dex_type])
        .http_rpc_url(rpc_http_url)
        .maybe_multicall_calls_per_rpc_request(multicall_calls_per_rpc_request)
        .use_hypersync_for_live_data(true)
        .postgres_cache_database_config(postgres_connect_options)
        .build();
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let mut data_client = BlockchainDataClientCore::new(config, None, None, cancellation_token);
    data_client.initialize_cache_database().await;
    data_client.cache.initialize_chain().await;
    data_client
        .register_dex_exchange(dex_type)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to register DEX exchange: {e}"))?;

    Ok(data_client)
}

fn parse_chain_dex(chain: &str, dex: &str) -> anyhow::Result<(Chain, DexType)> {
    let chain = Chain::from_chain_name(chain)
        .ok_or_else(|| anyhow::anyhow!("Invalid chain name: {chain}"))?;

    let dex_type = find_dex_type_case_insensitive(dex, chain).ok_or_else(|| {
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

    Ok((chain.to_owned(), dex_type))
}

fn rpc_http_url(chain: &Chain, rpc_url: Option<String>) -> anyhow::Result<String> {
    rpc_url
        .or_else(|| check_infura_rpc_provider(&chain.name))
        .or_else(|| std::env::var("RPC_HTTP_URL").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No RPC URL provided for {}. Set --rpc-url, INFURA_API_KEY, or RPC_HTTP_URL",
                chain.name
            )
        })
}

async fn resolve_to_block(data_client: &BlockchainDataClientCore, to_block: Option<u64>) -> u64 {
    match to_block {
        Some(block) => block,
        None => data_client.hypersync_client.current_block().await,
    }
}

fn load_pool_addresses(
    addresses: Vec<String>,
    addresses_file: Option<String>,
) -> anyhow::Result<Vec<String>> {
    let mut pool_addresses = addresses;

    if let Some(addresses_file) = addresses_file {
        let contents = fs::read_to_string(&addresses_file)
            .map_err(|e| anyhow::anyhow!("Failed to read addresses file {addresses_file}: {e}"))?;

        for line in contents.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                pool_addresses.push(trimmed.to_string());
            }
        }
    }

    if pool_addresses.is_empty() {
        anyhow::bail!("At least one --address or --addresses-file entry is required");
    }

    Ok(pool_addresses)
}

#[derive(Debug)]
struct PoolAnalysisOutcome {
    pool_address: String,
    target_block: u64,
    snapshot_block_position: BlockPosition,
    positions: usize,
    ticks: usize,
    valid: bool,
    already_valid: bool,
    liquidity_utilization_rate: f64,
}

impl PoolAnalysisOutcome {
    fn to_json(&self, chain: &str, dex: &str) -> serde_json::Value {
        json!({
            "chain": chain,
            "dex": dex,
            "pool_address": self.pool_address.as_str(),
            "target_block": self.target_block,
            "status": "success",
            "snapshot_block": self.snapshot_block_position.number,
            "snapshot_transaction_index": self.snapshot_block_position.transaction_index,
            "snapshot_log_index": self.snapshot_block_position.log_index,
            "positions": self.positions,
            "ticks": self.ticks,
            "valid": self.valid,
            "already_valid": self.already_valid,
            "liquidity_utilization_rate": self.liquidity_utilization_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn load_pool_addresses_merges_cli_and_file_entries_in_order() {
        let addresses_file = write_addresses_file(
            "
# ignored
0x2222222222222222222222222222222222222222

  0x3333333333333333333333333333333333333333
            ",
        );
        let result = load_pool_addresses(
            vec!["0x1111111111111111111111111111111111111111".to_string()],
            Some(addresses_file.to_string_lossy().to_string()),
        )
        .unwrap();

        fs::remove_file(addresses_file).unwrap();

        assert_eq!(
            result,
            vec![
                "0x1111111111111111111111111111111111111111".to_string(),
                "0x2222222222222222222222222222222222222222".to_string(),
                "0x3333333333333333333333333333333333333333".to_string(),
            ]
        );
    }

    #[rstest]
    fn load_pool_addresses_rejects_empty_input() {
        let error = load_pool_addresses(Vec::new(), None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("At least one --address or --addresses-file entry is required")
        );
    }

    fn write_addresses_file(contents: &str) -> PathBuf {
        let unique_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "nautilus-analyze-pools-addresses-{}-{unique_id}.txt",
            std::process::id()
        ));
        fs::write(&path, contents).unwrap();
        path
    }
}
