//! Blockchain management and synchronization utilities.

use crate::{
    blockchain::{
        analyze::run_analyze_pool,
        sync::{run_sync_blocks, run_sync_dex},
    },
    opt::{BlockchainCommand, BlockchainOpt},
};

pub mod analyze;
pub mod sync;

/// Runs blockchain commands based on the provided options.
///
/// # Errors
///
/// Returns an error if execution of the specified blockchain command fails.
pub async fn run_blockchain_command(opt: BlockchainOpt) -> anyhow::Result<()> {
    match opt.command {
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
                multicall_calls_per_rpc_request,
            )
            .await
        }
    }
}
