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

//! Command implementation for the `nautilus-blockchain` binary.
//!
//! This module is not a stable library API.

use clap::FromArgMatches;

use crate::cli::{
    analyze::{run_analyze_pool, run_analyze_pools},
    help::augment_help,
    opt::{BlockchainCli, BlockchainCommand},
    sync::{run_sync_blocks, run_sync_dex},
};

mod analyze;
mod help;
mod opt;
mod sync;

/// Builds the `nautilus-blockchain` command, including capability-derived help.
#[must_use]
pub fn command() -> clap::Command {
    let command = <BlockchainCli as clap::CommandFactory>::command();
    augment_help(command)
}

/// Runs a parsed `nautilus-blockchain` invocation.
///
/// Exits the process with a clap error if `matches` did not come from [`command`].
///
/// # Errors
///
/// Returns an error if the selected command fails.
pub async fn run(matches: &clap::ArgMatches) -> anyhow::Result<()> {
    let parsed = BlockchainCli::from_arg_matches(matches);
    let cli = parsed.unwrap_or_else(|e| e.exit());
    run_command(cli).await
}

async fn run_command(cli: BlockchainCli) -> anyhow::Result<()> {
    match cli.command {
        BlockchainCommand::SyncBlocks {
            chain,
            to_block,
            from_block,
            database,
        } => run_sync_blocks(chain, from_block, to_block, database).await,
        BlockchainCommand::SyncDex {
            chain,
            dex,
            rpc_url,
            database,
            reset,
            multicall_calls_per_rpc_request,
        } => {
            run_sync_dex(
                chain,
                dex,
                rpc_url,
                database,
                reset,
                multicall_calls_per_rpc_request,
            )
            .await
        }
        BlockchainCommand::AnalyzePool {
            chain,
            dex,
            address,
            from_block,
            to_block,
            rpc_url,
            reset,
            require_existing_snapshot,
            checkpoint_blocks,
            skip_validation,
            snapshot_from_rpc,
            database,
            multicall_calls_per_rpc_request,
        } => {
            run_analyze_pool(
                chain,
                dex,
                address,
                from_block,
                to_block,
                rpc_url,
                database,
                reset,
                require_existing_snapshot,
                checkpoint_blocks,
                skip_validation,
                snapshot_from_rpc,
                multicall_calls_per_rpc_request,
            )
            .await
        }
        BlockchainCommand::AnalyzePools {
            chain,
            dex,
            addresses,
            addresses_file,
            from_block,
            to_block,
            rpc_url,
            reset,
            require_existing_snapshot,
            checkpoint_blocks,
            skip_validation,
            snapshot_from_rpc,
            concurrency,
            database,
            multicall_calls_per_rpc_request,
        } => {
            run_analyze_pools(
                chain,
                dex,
                addresses,
                addresses_file,
                from_block,
                to_block,
                rpc_url,
                database,
                reset,
                require_existing_snapshot,
                checkpoint_blocks,
                skip_validation,
                snapshot_from_rpc,
                concurrency,
                multicall_calls_per_rpc_request,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("sync-blocks", &[] as &[&str])]
    #[case("sync-dex", &["--dex", "UniswapV3"])]
    #[case(
        "analyze-pool",
        &[
            "--dex",
            "UniswapV3",
            "--address",
            "0x1111111111111111111111111111111111111111",
        ]
    )]
    #[case(
        "analyze-pools",
        &[
            "--dex",
            "UniswapV3",
            "--address",
            "0x1111111111111111111111111111111111111111",
        ]
    )]
    #[tokio::test]
    async fn run_rejects_unknown_chain_before_io(#[case] subcommand: &str, #[case] extra: &[&str]) {
        let mut args = vec![
            "nautilus-blockchain".to_string(),
            subcommand.to_string(),
            "--chain".to_string(),
            "not-a-chain".to_string(),
        ];
        args.extend(extra.iter().map(|arg| (*arg).to_string()));
        let command = command();
        let matches = command
            .try_get_matches_from(args)
            .expect("unknown chain is a command error, not a parse error");
        let error = run(&matches)
            .await
            .expect_err("unknown chain should fail before I/O");

        assert_eq!(error.to_string(), "Invalid chain name: not-a-chain");
    }
}
