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
    data::core::{BlockchainDataClientCore, SnapshotValidation},
    exchanges::{find_dex_type_case_insensitive, get_supported_dexes_for_chain},
    rpc::providers::check_infura_rpc_provider,
};
use nautilus_infrastructure::sql::pg::get_postgres_connect_options;
use nautilus_model::defi::{
    DexType, PoolIdentifier, chain::Chain, data::block::BlockPosition,
    pool_analysis::snapshot::PoolSnapshot, validation::validate_address,
};
use serde_json::json;
use ustr::Ustr;

use crate::opt::DatabaseConfig;

/// Runs pool analysis for the specified chain, DEX, and pool address.
///
/// # Errors
///
/// Returns an error if the chain or DEX parameters are invalid.
#[expect(
    clippy::too_many_arguments,
    reason = "CLI command options map directly to clap fields"
)]
pub(crate) async fn run_analyze_pool(
    chain: String,
    dex: String,
    pool_address: String,
    from_block: Option<u64>,
    to_block: Option<u64>,
    rpc_url: Option<String>,
    database: DatabaseConfig,
    reset: bool,
    require_existing_snapshot: bool,
    checkpoint_blocks: Vec<u64>,
    skip_validation: bool,
    multicall_calls_per_rpc_request: Option<u32>,
) -> anyhow::Result<()> {
    let (chain, dex_type) = parse_chain_dex(&chain, &dex)?;
    let chain_name = chain.name.to_string();
    let dex_name = dex_type.to_string();
    let mut data_client = create_data_client(
        chain,
        dex_type,
        rpc_url,
        database,
        multicall_calls_per_rpc_request,
    )
    .await?;
    let to_block = resolve_to_block(&data_client, to_block).await;

    let outcomes = analyze_pool_with_client(
        &mut data_client,
        dex_type,
        pool_address,
        from_block,
        to_block,
        reset,
        require_existing_snapshot,
        &checkpoint_blocks,
        skip_validation,
    )
    .await?;

    for outcome in &outcomes {
        println!("{}", outcome.to_json(&chain_name, &dex_name));
    }

    Ok(())
}

/// Default number of pools analyzed concurrently when `--concurrency` is omitted.
const DEFAULT_ANALYZE_CONCURRENCY: usize = 4;

/// Runs pool analysis for several pool addresses in one initialized runtime.
///
/// Pools are analyzed concurrently up to `concurrency` at a time. Each pool runs with its own data
/// client, so they share no state.
///
/// # Errors
///
/// Returns an error if chain, DEX, database, RPC, or address file setup fails. Individual pool
/// failures are emitted as structured output and the command returns an error after all pools run.
#[expect(
    clippy::too_many_arguments,
    reason = "CLI command options map directly to clap fields"
)]
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
    require_existing_snapshot: bool,
    checkpoint_blocks: Vec<u64>,
    skip_validation: bool,
    concurrency: Option<usize>,
    multicall_calls_per_rpc_request: Option<u32>,
) -> anyhow::Result<()> {
    let pool_addresses = load_pool_addresses(addresses, addresses_file)?;
    let (chain, dex_type) = parse_chain_dex(&chain, &dex)?;
    let chain_name = chain.name.to_string();
    let dex_name = dex_type.to_string();

    // Resolve the target block once so every pool snapshots at the same tip when --to-block is omitted.
    let to_block = if let Some(block) = to_block {
        block
    } else {
        let data_client = create_data_client(
            chain.clone(),
            dex_type,
            rpc_url.clone(),
            database.clone(),
            multicall_calls_per_rpc_request,
        )
        .await?;
        resolve_to_block(&data_client, None).await
    };

    // Pools are independent (own RPC client, profiler state, and snapshot rows), so analyze them
    // concurrently. The semaphore bounds parallelism against RPC rate limits and the Postgres
    // connection count; tune with --concurrency.
    // ponytail: one DB pool per worker; share a single sqlx pool if connection count bites.
    let concurrency = concurrency.unwrap_or(DEFAULT_ANALYZE_CONCURRENCY).max(1);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    // Pair each pool address with its task handle so a task that panics (e.g. the no-liquidity
    // extract_snapshot panic) still maps to a structured per-pool failure line, not a bare log.
    let mut tasks: Vec<(
        String,
        tokio::task::JoinHandle<anyhow::Result<Vec<PoolAnalysisOutcome>>>,
    )> = Vec::with_capacity(pool_addresses.len());

    for pool_address in pool_addresses {
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore is never closed");
        let chain = chain.clone();
        let rpc_url = rpc_url.clone();
        let database = database.clone();
        let checkpoint_blocks = checkpoint_blocks.clone();
        let task_address = pool_address.clone();

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let mut data_client = create_data_client(
                chain,
                dex_type,
                rpc_url,
                database,
                multicall_calls_per_rpc_request,
            )
            .await?;
            analyze_pool_with_client(
                &mut data_client,
                dex_type,
                pool_address,
                from_block,
                to_block,
                reset,
                require_existing_snapshot,
                &checkpoint_blocks,
                skip_validation,
            )
            .await
        });
        tasks.push((task_address, handle));
    }

    let mut failures = 0usize;

    for (pool_address, handle) in tasks {
        let result = match handle.await {
            Ok(result) => result,
            Err(join_error) => Err(anyhow::anyhow!(
                "analysis task did not complete: {join_error}"
            )),
        };

        match result {
            Ok(outcomes) => {
                for outcome in &outcomes {
                    println!("{}", outcome.to_json(&chain_name, &dex_name));
                }
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

#[expect(
    clippy::too_many_arguments,
    reason = "CLI command options map directly to clap fields"
)]
async fn analyze_pool_with_client(
    data_client: &mut BlockchainDataClientCore,
    dex_type: DexType,
    pool_address: String,
    from_block: Option<u64>,
    to_block: u64,
    reset: bool,
    require_existing_snapshot: bool,
    checkpoint_blocks: &[u64],
    skip_validation: bool,
) -> anyhow::Result<Vec<PoolAnalysisOutcome>> {
    let pool_address = validate_address(&pool_address)?;
    let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));

    let checkpoints = if checkpoint_blocks.is_empty() {
        vec![to_block]
    } else {
        let checkpoints = normalize_checkpoints(checkpoint_blocks, to_block);
        if checkpoints.is_empty() {
            anyhow::bail!("All --checkpoint-blocks exceed --to-block {to_block}");
        }
        checkpoints
    };

    // Bounded-replay mode: a usable snapshot must already exist at or before the first checkpoint,
    // otherwise the caller wants needs_bootstrap rather than a full creation-to-target bootstrap.
    let first_checkpoint = checkpoints[0];
    if require_existing_snapshot
        && needs_bootstrap_before_target(data_client, &pool_identifier, first_checkpoint).await?
    {
        return Ok(vec![PoolAnalysisOutcome::NeedsBootstrap(
            PoolNeedsBootstrapOutcome {
                pool_address: pool_address.to_string(),
                target_block: first_checkpoint,
            },
        )]);
    }

    // Sync once up to the final checkpoint, honoring reset/from_block. Each checkpoint then bootstraps
    // incrementally from the previous checkpoint's snapshot, so one pass produces every snapshot.
    let last_checkpoint = *checkpoints.last().expect("checkpoints is non-empty");
    data_client
        .sync_pool_events(
            &dex_type,
            pool_identifier,
            from_block,
            Some(last_checkpoint),
            reset,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to sync pool events: {e}"))?;

    let pool = data_client
        .cache
        .get_pool(&pool_identifier)
        .ok_or_else(|| anyhow::anyhow!("Pool {pool_identifier} not found in cache"))?
        .clone();

    let mut outcomes = Vec::with_capacity(checkpoints.len());
    for checkpoint in checkpoints {
        log::info!("Profiling pool {pool_identifier} to checkpoint block {checkpoint}");
        let (profiler, already_valid) = data_client
            .bootstrap_latest_pool_profiler(&pool, Some(checkpoint))
            .await?;
        let snapshot = profiler.extract_snapshot();
        let snapshot_block_position = snapshot.block_position.clone();
        let positions = snapshot.positions.len();
        let ticks = snapshot.ticks.len();

        log::info!(
            "Saving pool snapshot with {positions} positions and {ticks} ticks to database..."
        );
        data_client
            .cache
            .add_pool_snapshot(&pool.dex.name, &pool.pool_identifier, &snapshot)
            .await?;

        let validation = if skip_validation {
            SnapshotValidation::Replay
        } else {
            data_client
                .check_snapshot_validity(&profiler, already_valid)
                .await?
        };

        let liquidity_utilization_rate = profiler.liquidity_utilization_rate();
        log::info!(
            "Pool liquidity utilization rate is {:.4}%",
            liquidity_utilization_rate * 100.0
        );

        outcomes.push(PoolAnalysisOutcome::Success(PoolAnalysisSuccessOutcome {
            pool_address: pool_address.to_string(),
            target_block: checkpoint,
            snapshot_block_position,
            positions,
            ticks,
            validation,
            already_valid,
            liquidity_utilization_rate,
        }));
    }

    Ok(outcomes)
}

/// Sorts, dedups, and clamps requested checkpoint blocks to `to_block`.
///
/// Checkpoints above `to_block` are dropped; they cannot be snapshotted in this pass.
fn normalize_checkpoints(checkpoint_blocks: &[u64], to_block: u64) -> Vec<u64> {
    let mut checkpoints: Vec<u64> = checkpoint_blocks
        .iter()
        .copied()
        .filter(|&block| block <= to_block)
        .collect();
    checkpoints.sort_unstable();
    checkpoints.dedup();
    checkpoints
}

async fn needs_bootstrap_before_target(
    data_client: &BlockchainDataClientCore,
    pool_identifier: &PoolIdentifier,
    to_block: u64,
) -> anyhow::Result<bool> {
    let database = data_client
        .cache
        .database
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Database is not initialized"))?;
    let snapshot = database
        .load_latest_pool_snapshot(
            data_client.chain.chain_id,
            pool_identifier,
            Some(to_block),
            true,
        )
        .await?;

    let Some(snapshot) = snapshot else {
        return Ok(true);
    };
    let snapshot_needs_bootstrap = data_client
        .cache
        .get_pool(pool_identifier)
        .is_some_and(|pool| is_empty_creation_snapshot(&snapshot, pool.creation_block));

    Ok(snapshot_needs_bootstrap)
}

fn is_empty_creation_snapshot(snapshot: &PoolSnapshot, pool_creation_block: u64) -> bool {
    snapshot.positions.is_empty()
        && snapshot.ticks.is_empty()
        && snapshot.block_position.number == pool_creation_block
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
enum PoolAnalysisOutcome {
    Success(PoolAnalysisSuccessOutcome),
    NeedsBootstrap(PoolNeedsBootstrapOutcome),
}

#[derive(Debug)]
struct PoolAnalysisSuccessOutcome {
    pool_address: String,
    target_block: u64,
    snapshot_block_position: BlockPosition,
    positions: usize,
    ticks: usize,
    validation: SnapshotValidation,
    already_valid: bool,
    liquidity_utilization_rate: f64,
}

#[derive(Debug)]
struct PoolNeedsBootstrapOutcome {
    pool_address: String,
    target_block: u64,
}

impl PoolAnalysisOutcome {
    fn to_json(&self, chain: &str, dex: &str) -> serde_json::Value {
        match self {
            Self::Success(outcome) => json!({
                "chain": chain,
                "dex": dex,
                "pool_address": outcome.pool_address.as_str(),
                "target_block": outcome.target_block,
                "status": "success",
                "snapshot_block": outcome.snapshot_block_position.number,
                "snapshot_transaction_index": outcome.snapshot_block_position.transaction_index,
                "snapshot_log_index": outcome.snapshot_block_position.log_index,
                "positions": outcome.positions,
                "ticks": outcome.ticks,
                "validation_state": outcome.validation.as_str(),
                "already_valid": outcome.already_valid,
                "liquidity_utilization_rate": outcome.liquidity_utilization_rate,
            }),
            Self::NeedsBootstrap(outcome) => json!({
                "chain": chain,
                "dex": dex,
                "pool_address": outcome.pool_address.as_str(),
                "target_block": outcome.target_block,
                "status": "needs_bootstrap",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use clap::Parser;
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        defi::pool_analysis::snapshot::{PoolAnalytics, PoolState},
        identifiers::InstrumentId,
    };
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::opt::NautilusCli;

    #[rstest]
    fn analyze_pool_cli_parses_require_existing_snapshot() {
        let cli = NautilusCli::try_parse_from([
            "nautilus",
            "blockchain",
            "analyze-pool",
            "--chain",
            "ethereum",
            "--dex",
            "UniswapV3",
            "--address",
            "0x1111111111111111111111111111111111111111",
            "--to-block",
            "200",
            "--rpc-url",
            "http://localhost:8545",
            "--require-existing-snapshot",
            "--host",
            "localhost",
            "--port",
            "5433",
            "--username",
            "postgres",
            "--database",
            "nautilus",
            "--password",
            "secret",
        ])
        .unwrap();

        match cli.command {
            crate::opt::Commands::Blockchain(crate::opt::BlockchainOpt {
                command:
                    crate::opt::BlockchainCommand::AnalyzePool {
                        require_existing_snapshot,
                        ..
                    },
            }) => {
                assert!(require_existing_snapshot);
            }
            _ => panic!("Expected analyze-pool blockchain command"),
        }
    }

    #[rstest]
    fn analyze_pools_cli_parses_checkpoint_blocks_and_concurrency() {
        let cli = NautilusCli::try_parse_from([
            "nautilus",
            "blockchain",
            "analyze-pools",
            "--chain",
            "ethereum",
            "--dex",
            "UniswapV3",
            "--address",
            "0x1111111111111111111111111111111111111111",
            "--to-block",
            "500",
            "--checkpoint-blocks",
            "100,200,300",
            "--skip-validation",
            "--concurrency",
            "8",
            "--rpc-url",
            "http://localhost:8545",
            "--host",
            "localhost",
            "--port",
            "5433",
            "--username",
            "postgres",
            "--database",
            "nautilus",
            "--password",
            "secret",
        ])
        .unwrap();

        match cli.command {
            crate::opt::Commands::Blockchain(crate::opt::BlockchainOpt {
                command:
                    crate::opt::BlockchainCommand::AnalyzePools {
                        checkpoint_blocks,
                        skip_validation,
                        concurrency,
                        ..
                    },
            }) => {
                assert_eq!(checkpoint_blocks, vec![100, 200, 300]);
                assert!(skip_validation);
                assert_eq!(concurrency, Some(8));
            }
            _ => panic!("Expected analyze-pools blockchain command"),
        }
    }

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

    #[rstest]
    fn pool_analysis_needs_bootstrap_json_matches_contract() {
        let outcome = PoolAnalysisOutcome::NeedsBootstrap(PoolNeedsBootstrapOutcome {
            pool_address: "0x1111111111111111111111111111111111111111".to_string(),
            target_block: 25_218_797,
        });

        assert_eq!(
            outcome.to_json("Ethereum", "UniswapV3"),
            json!({
                "chain": "Ethereum",
                "dex": "UniswapV3",
                "pool_address": "0x1111111111111111111111111111111111111111",
                "target_block": 25_218_797,
                "status": "needs_bootstrap",
            })
        );
    }

    #[rstest]
    fn pool_analysis_success_json_matches_contract() {
        let outcome = PoolAnalysisOutcome::Success(PoolAnalysisSuccessOutcome {
            pool_address: "0x1111111111111111111111111111111111111111".to_string(),
            target_block: 25_218_807,
            snapshot_block_position: BlockPosition::new(25_218_797, "0xabc".to_string(), 3, 4),
            positions: 2,
            ticks: 7,
            validation: SnapshotValidation::OnChain,
            already_valid: false,
            liquidity_utilization_rate: 0.25,
        });

        assert_eq!(
            outcome.to_json("Ethereum", "UniswapV3"),
            json!({
                "chain": "Ethereum",
                "dex": "UniswapV3",
                "pool_address": "0x1111111111111111111111111111111111111111",
                "target_block": 25_218_807,
                "status": "success",
                "snapshot_block": 25_218_797,
                "snapshot_transaction_index": 3,
                "snapshot_log_index": 4,
                "positions": 2,
                "ticks": 7,
                "validation_state": "on_chain",
                "already_valid": false,
                "liquidity_utilization_rate": 0.25,
            })
        );
    }

    #[rstest]
    #[case(100, 100, true)]
    #[case(101, 100, false)]
    fn empty_creation_snapshot_detection(
        #[case] snapshot_block: u64,
        #[case] creation_block: u64,
        #[case] expected: bool,
    ) {
        let snapshot = PoolSnapshot::new(
            "ETHUSDT.BINANCE".parse::<InstrumentId>().unwrap(),
            PoolState::default(),
            Vec::new(),
            Vec::new(),
            PoolAnalytics::default(),
            BlockPosition::new(snapshot_block, "0xabc".to_string(), 0, 0),
            UnixNanos::from(0),
            UnixNanos::from(0),
        );

        assert_eq!(
            is_empty_creation_snapshot(&snapshot, creation_block),
            expected
        );
    }

    #[rstest]
    #[case(vec![300, 100, 200], 500, vec![100, 200, 300])]
    #[case(vec![100, 100, 200], 500, vec![100, 200])]
    #[case(vec![100, 600, 200], 500, vec![100, 200])]
    #[case(vec![600, 700], 500, Vec::<u64>::new())]
    fn normalize_checkpoints_sorts_dedups_and_clamps(
        #[case] input: Vec<u64>,
        #[case] to_block: u64,
        #[case] expected: Vec<u64>,
    ) {
        assert_eq!(normalize_checkpoints(&input, to_block), expected);
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
