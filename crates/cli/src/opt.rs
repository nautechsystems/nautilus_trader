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

use clap::Parser;

/// Main CLI structure for parsing command-line arguments and options.
///
/// This is the entry point for the NautilusTrader command-line interface,
/// providing access to various system management and operational commands.
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
        /// The blockchain chain name (case-insensitive). Examples: ethereum, arbitrum, base, polygon, bsc
        #[arg(long)]
        chain: String,
        /// The DEX name (case-insensitive). Examples: `UniswapV3`, uniswapv3, `SushiSwapV2`, `PancakeSwapV3`
        #[arg(long)]
        dex: String,
        /// RPC HTTP URL for blockchain calls (optional, falls back to `RPC_HTTP_URL` env var)
        #[arg(long)]
        rpc_url: Option<String>,
        /// Reset sync progress and start from the beginning, ignoring last synced block
        #[arg(long)]
        reset: bool,
        /// Maximum number of Multicall calls per RPC request (optional, defaults to 100)
        #[arg(long)]
        multicall_calls_per_rpc_request: Option<u32>,
        /// Database configuration options
        #[clap(flatten)]
        database: DatabaseConfig,
    },
    /// Analyze a specific DEX pool.
    AnalyzePool {
        /// The blockchain chain name (case-insensitive). Examples: ethereum, arbitrum, base, polygon, bsc
        #[arg(long)]
        chain: String,
        /// The DEX name (case-insensitive). Examples: UniswapV3, uniswapv3, SushiSwapV2, PancakeSwapV3
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
        #[arg(long)]
        rpc_url: Option<String>,
        /// Reset sync progress and start from the beginning, ignoring last synced block
        #[arg(long)]
        reset: bool,
        /// Maximum number of Multicall calls per RPC request (optional, defaults to 100)
        #[arg(long)]
        multicall_calls_per_rpc_request: Option<u32>,
        /// Database configuration options
        #[clap(flatten)]
        database: DatabaseConfig,
    },
}
