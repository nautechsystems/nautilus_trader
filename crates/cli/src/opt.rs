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

use clap::Parser;

/// Command-line interface for NautilusTrader.
#[derive(Debug, Parser)]
#[clap(version, about, author)]
pub struct NautilusCli {
    #[clap(subcommand)]
    pub command: Commands,
}

/// Available top-level commands for the NautilusTrader CLI.
#[derive(Parser, Debug)]
pub enum Commands {
    Database(DatabaseOpt),
    #[cfg(feature = "defi")]
    Blockchain(BlockchainOpt),
}

/// Database management options and subcommands.
#[derive(Parser, Debug)]
#[command(about = "Postgres database operations", long_about = None)]
pub struct DatabaseOpt {
    #[clap(subcommand)]
    pub command: DatabaseCommand,
}

/// Configuration parameters for database connection and operations.
#[derive(Parser, Debug, Clone)]
pub struct DatabaseConfig {
    /// Hostname or IP address of the database server.
    #[arg(long)]
    pub host: Option<String>,
    /// Port number of the database server.
    #[arg(long)]
    pub port: Option<u16>,
    /// Username for connecting to the database.
    #[arg(long)]
    pub username: Option<String>,
    /// Name of the database.
    #[arg(long)]
    pub database: Option<String>,
    /// Password for connecting to the database.
    #[arg(long)]
    pub password: Option<String>,
    /// Directory path to the schema files.
    #[arg(long)]
    pub schema: Option<String>,
}

/// Available database management commands.
#[derive(Parser, Debug, Clone)]
#[command(about = "Postgres database operations", long_about = None)]
pub enum DatabaseCommand {
    /// Initializes a new Postgres database with the latest schema.
    Init(DatabaseConfig),
    /// Drops roles, privileges and deletes all data from the database.
    Drop(DatabaseConfig),
}

#[cfg(feature = "defi")]
/// Blockchain management options and subcommands.
#[derive(Parser, Debug)]
#[command(about = "Blockchain operations", long_about = None)]
pub struct BlockchainOpt {
    #[clap(subcommand)]
    pub command: BlockchainCommand,
}

#[cfg(feature = "defi")]
/// Available blockchain management commands.
#[derive(Parser, Debug, Clone)]
#[command(about = "Blockchain operations", long_about = None)]
pub enum BlockchainCommand {
    /// Syncs blockchain blocks.
    SyncBlocks {
        /// The blockchain chain name (case-insensitive). Examples: ethereum, arbitrum, base, polygon, bsc
        #[arg(long)]
        chain: String,
        /// Starting block number to sync from (optional)
        #[arg(long)]
        from_block: Option<u64>,
        /// Ending block number to sync to (optional, defaults to current chain head)
        #[arg(long)]
        to_block: Option<u64>,
        /// Database configuration options
        #[clap(flatten)]
        database: DatabaseConfig,
    },
    /// Sync DEX pools.
    SyncDex {
        /// The blockchain chain name (case-insensitive). Supported chains are listed below.
        #[arg(long)]
        chain: String,
        /// The DEX name (case-insensitive). Supported DEX names are listed below.
        #[arg(long)]
        dex: String,
        /// RPC HTTP URL for blockchain calls (optional, falls back to `RPC_HTTP_URL` env var)
        #[arg(long)]
        rpc_url: Option<String>,
        /// Reset sync progress and start from the beginning, ignoring last synced block
        #[arg(long)]
        reset: bool,
        /// Maximum number of Multicall calls per RPC request (optional, defaults to 200)
        #[arg(long)]
        multicall_calls_per_rpc_request: Option<u32>,
        /// Database configuration options
        #[clap(flatten)]
        database: DatabaseConfig,
    },
    /// Analyze a specific DEX pool.
    AnalyzePool {
        /// The blockchain chain name (case-insensitive). Supported chains are listed below.
        #[arg(long)]
        chain: String,
        /// The DEX name (case-insensitive). Supported DEX names are listed below.
        #[arg(long)]
        dex: String,
        /// The pool contract address
        #[arg(long)]
        address: String,
        /// Starting block number to sync from (optional)
        #[arg(long)]
        from_block: Option<u64>,
        /// Ending block number to sync to (optional, defaults to current chain head)
        #[arg(long)]
        to_block: Option<u64>,
        /// RPC HTTP URL for blockchain calls (optional, falls back to RPC_HTTP_URL env var)
        #[expect(
            clippy::doc_markdown,
            reason = "clap renders doc comments as plain help text"
        )]
        #[arg(long)]
        rpc_url: Option<String>,
        /// Reset sync progress and start from the beginning, ignoring last synced block
        #[arg(long)]
        reset: bool,
        /// Return needs_bootstrap for pools without a valid snapshot before the target block
        #[expect(
            clippy::doc_markdown,
            reason = "clap renders doc comments as plain help text"
        )]
        #[arg(long)]
        require_existing_snapshot: bool,
        /// Checkpoint block numbers to snapshot in one pass (comma-separated, each at or below to-block)
        #[arg(long, value_delimiter = ',')]
        checkpoint_blocks: Vec<u64>,
        /// Skip on-chain validation and persist replay-derived snapshots without the multicall compare
        #[arg(long)]
        skip_validation: bool,
        /// Maximum number of Multicall calls per RPC request (optional, defaults to 200)
        #[arg(long)]
        multicall_calls_per_rpc_request: Option<u32>,
        /// Database configuration options
        #[clap(flatten)]
        database: DatabaseConfig,
    },
    /// Analyze several DEX pools in one runtime.
    AnalyzePools {
        /// The blockchain chain name (case-insensitive). Supported chains are listed below.
        #[arg(long)]
        chain: String,
        /// The DEX name (case-insensitive). Supported DEX names are listed below.
        #[arg(long)]
        dex: String,
        /// Pool contract address. Can be repeated.
        #[arg(long = "address")]
        addresses: Vec<String>,
        /// File containing one pool contract address per line. Empty lines and comment lines are ignored.
        #[arg(long)]
        addresses_file: Option<String>,
        /// Starting block number to sync from (optional)
        #[arg(long)]
        from_block: Option<u64>,
        /// Ending block number to sync to (optional, defaults to current chain head)
        #[arg(long)]
        to_block: Option<u64>,
        /// RPC HTTP URL for blockchain calls (optional, falls back to RPC_HTTP_URL env var)
        #[expect(
            clippy::doc_markdown,
            reason = "clap renders doc comments as plain help text"
        )]
        #[arg(long)]
        rpc_url: Option<String>,
        /// Reset sync progress and start from the beginning, ignoring last synced block
        #[arg(long)]
        reset: bool,
        /// Return needs_bootstrap for pools without a valid snapshot before the target block
        #[expect(
            clippy::doc_markdown,
            reason = "clap renders doc comments as plain help text"
        )]
        #[arg(long)]
        require_existing_snapshot: bool,
        /// Checkpoint block numbers to snapshot in one pass (comma-separated, each at or below to-block)
        #[arg(long, value_delimiter = ',')]
        checkpoint_blocks: Vec<u64>,
        /// Skip on-chain validation and persist replay-derived snapshots without the multicall compare
        #[arg(long)]
        skip_validation: bool,
        /// Maximum number of pools to analyze concurrently (optional, defaults to 4)
        #[arg(long)]
        concurrency: Option<usize>,
        /// Maximum number of Multicall calls per RPC request (optional, defaults to 200)
        #[arg(long)]
        multicall_calls_per_rpc_request: Option<u32>,
        /// Database configuration options
        #[clap(flatten)]
        database: DatabaseConfig,
    },
}

#[cfg(all(test, feature = "defi"))]
mod tests {
    use clap::Parser;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn analyze_pools_cli_parses_repeated_addresses_file_and_shared_options() {
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
            "--address",
            "0x2222222222222222222222222222222222222222",
            "--addresses-file",
            "/tmp/pools.txt",
            "--from-block",
            "100",
            "--to-block",
            "200",
            "--rpc-url",
            "http://localhost:8545",
            "--reset",
            "--require-existing-snapshot",
            "--multicall-calls-per-rpc-request",
            "25",
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
            Commands::Blockchain(BlockchainOpt {
                command:
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
                        concurrency,
                        multicall_calls_per_rpc_request,
                        database,
                    },
            }) => {
                assert_eq!(chain, "ethereum");
                assert_eq!(dex, "UniswapV3");
                assert_eq!(
                    addresses,
                    vec![
                        "0x1111111111111111111111111111111111111111".to_string(),
                        "0x2222222222222222222222222222222222222222".to_string(),
                    ]
                );
                assert_eq!(addresses_file.as_deref(), Some("/tmp/pools.txt"));
                assert_eq!(from_block, Some(100));
                assert_eq!(to_block, Some(200));
                assert_eq!(rpc_url.as_deref(), Some("http://localhost:8545"));
                assert!(reset);
                assert!(require_existing_snapshot);
                assert!(checkpoint_blocks.is_empty());
                assert!(!skip_validation);
                assert_eq!(concurrency, None);
                assert_eq!(multicall_calls_per_rpc_request, Some(25));
                assert_eq!(database.host.as_deref(), Some("localhost"));
                assert_eq!(database.port, Some(5433));
                assert_eq!(database.username.as_deref(), Some("postgres"));
                assert_eq!(database.database.as_deref(), Some("nautilus"));
                assert_eq!(database.password.as_deref(), Some("secret"));
                assert_eq!(database.schema, None);
            }
            _ => panic!("Expected analyze-pools blockchain command"),
        }
    }

    #[rstest]
    #[case("analyze-pool")]
    #[case("analyze-pools")]
    fn blockchain_analysis_help_lists_capabilities_as_plain_text(#[case] subcommand: &str) {
        let mut command = crate::cli_command();
        let help = command
            .find_subcommand_mut("blockchain")
            .and_then(|command| command.find_subcommand_mut(subcommand))
            .map(|command| command.render_long_help().to_string())
            .unwrap();

        // Snapshot-capable DEXes are listed; the registered-but-unsupported SushiSwapV2 is not.
        assert!(help.contains("UniswapV3"));
        assert!(help.contains("PancakeSwapV3"));
        assert!(help.contains("AerodromeSlipstream"));
        assert!(!help.contains("SushiSwapV2"));
        assert!(help.contains("RPC_HTTP_URL"));
        assert!(help.contains("needs_bootstrap"));
        // Help is rendered as plain text, so doc-markdown backticks must not survive.
        assert!(!help.contains("`UniswapV3`"));
        assert!(!help.contains("`PancakeSwapV3`"));
        assert!(!help.contains("`RPC_HTTP_URL`"));
        assert!(!help.contains("`needs_bootstrap`"));
    }

    #[rstest]
    fn blockchain_sync_dex_help_lists_discoverable_dexes() {
        let mut command = crate::cli_command();
        let help = command
            .find_subcommand_mut("blockchain")
            .and_then(|command| command.find_subcommand_mut("sync-dex"))
            .map(|command| command.render_long_help().to_string())
            .unwrap();

        // sync-dex receives the discovery block, not the snapshot block.
        assert!(help.contains("Discoverable DEXes"));
        assert!(!help.contains("Snapshot-capable"));
        // UniswapV2 is discovery-only, so it appears here but never in the snapshot listing.
        assert!(help.contains("UniswapV2"));
    }
}
