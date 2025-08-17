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
    config::BlockchainDataClientConfig, data::core::BlockchainDataClientCore,
};
use nautilus_infrastructure::sql::pg::get_postgres_connect_options;
use nautilus_model::defi::chain::Chain;

use crate::opt::{BlockchainCommand, BlockchainOpt};

/// Runs blockchain commands based on the provided options.
///
/// # Errors
///
/// Returns an error if execution of the specified blockchain command fails.
pub async fn run_blockchain_command(opt: BlockchainOpt) -> anyhow::Result<()> {
    match opt.command {
        BlockchainCommand::SyncBlocks {
            chain,
            from_block,
            to_block,
            database,
        } => {
            let chain = Chain::from_chain_name(&chain)
                .ok_or_else(|| anyhow::anyhow!("Invalid chain name: {}", chain))?;
            let chain = Arc::new(chain.to_owned());
            let from_block = from_block.unwrap_or(0);

            let postgres_connect_options = get_postgres_connect_options(
                database.host,
                database.port,
                database.username,
                database.password,
                database.database,
            );
            let config = BlockchainDataClientConfig::new(
                chain.clone(),
                vec![],
                "".to_string(), // we dont need to http rpc url for block syncing
                None,
                None,
                true,
                None,
                None,
                Some(postgres_connect_options),
            );
            let mut data_client = BlockchainDataClientCore::new(config, None);
            data_client.initialize_cache_database().await;

            data_client.cache.initialize_chain().await;
            data_client
                .sync_blocks_checked(from_block, to_block)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to sync blocks: {}", e))?;
        }
    }
    Ok(())
}
