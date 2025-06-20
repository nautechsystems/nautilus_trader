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
    config::BlockchainDataClientConfig, data::BlockchainDataClient, exchanges,
};
use nautilus_common::logging::{
    logger::{Logger, LoggerConfig},
    writer::FileWriterConfig,
};
use nautilus_core::{UUID4, env::get_env_var};
use nautilus_data::DataClient;
use nautilus_live::runner::AsyncRunner;
use nautilus_model::{defi::chain::chains, identifiers::TraderId};
use tokio::sync::Notify;

// Run with `cargo run -p nautilus-blockchain --bin sync_pool_events --features hypersync`

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let _logger_guard = Logger::init_with_config(
        TraderId::default(),
        UUID4::new(),
        LoggerConfig::default(),
        FileWriterConfig::new(None, None, None, None),
    )?;

    let _ = AsyncRunner::default(); // Needed for live channels

    // Setup graceful shutdown with signal handling in different task
    let notify = Arc::new(Notify::new());
    let notifier = notify.clone();
    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to create SIGTERM listener");
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("Failed to create SIGINT listener");
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = sigint.recv() => {}
        }
        log::info!("Shutdown signal received, shutting down...");
        notifier.notify_one();
    });

    // Initialize the blockchain data client, connect and subscribe to live blocks with RPC
    let chain = Arc::new(chains::ETHEREUM.clone());
    // WETH/USDC Uniswap V3 pool
    let weth_usdc_pool = "0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640";
    let pool_creation_block = 12376729;
    let from_block = Some(22550000);

    let http_rpc_url = get_env_var("RPC_HTTP_URL")?;
    let blockchain_config = BlockchainDataClientConfig::new(
        chain.clone(),
        http_rpc_url,
        Some(3), // RPC requests per second
        None,    // WSS RPC URL
        true,    // Use hypersync for live data
        from_block,
    );

    let mut data_client = BlockchainDataClient::new(blockchain_config);
    data_client.initialize_cache_database(None).await;

    let univ3 = exchanges::ethereum::UNISWAP_V3.clone();
    let dex_id = univ3.id();

    data_client.connect().await?;
    data_client.register_exchange(univ3.clone()).await?;

    loop {
        tokio::select! {
            () = notify.notified() => break,
             () = async {
                data_client.sync_exchange_pools(
                    dex_id.as_str(),
                    Some(pool_creation_block),
                    Some(pool_creation_block + 1),
                ).await.unwrap();
                data_client.sync_pool_swaps(
                    dex_id.as_str(),
                    weth_usdc_pool.to_string(),
                    from_block,
                    None,
                ).await.unwrap();
                data_client.sync_pool_mints(
                    dex_id.as_str(),
                    weth_usdc_pool.to_string(),
                    from_block,
                    None,
                ).await.unwrap();
                data_client.sync_pool_burns(
                    dex_id.as_str(),
                    weth_usdc_pool.to_string(),
                    from_block,
                    None,
                ).await.unwrap();
            } => break,
        }
    }

    data_client.disconnect().await?;
    Ok(())
}
