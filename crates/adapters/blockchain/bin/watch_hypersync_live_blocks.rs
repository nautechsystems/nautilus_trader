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

use nautilus_blockchain::{config::BlockchainAdapterConfig, data::BlockchainDataClient};
use nautilus_common::logging::{
    logger::{Logger, LoggerConfig},
    writer::FileWriterConfig,
};
use nautilus_core::{UUID4, env::get_env_var};
use nautilus_model::{
    defi::chain::{Blockchain, Chain, chains},
    identifiers::TraderId,
};
use tokio::sync::Notify;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    // Setup logger
    let _logger_guard = Logger::init_with_config(
        TraderId::default(),
        UUID4::new(),
        LoggerConfig::default(),
        FileWriterConfig::new(None, None, None, None),
    )?;

    // Setup graceful shutdown with signal handling in a different task
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

    // Initialize the blockchain data client, connect and subscribe to live blocks with HyperSync watch
    let chain: Chain = match std::env::var("CHAIN") {
        Ok(chain_str) => {
            if let Ok(blockchain) = chain_str.parse::<Blockchain>() {
                match blockchain {
                    Blockchain::Ethereum => chains::ETHEREUM.clone(),
                    Blockchain::Base => chains::BASE.clone(),
                    Blockchain::Arbitrum => chains::ARBITRUM.clone(),
                    Blockchain::Polygon => chains::POLYGON.clone(),
                    _ => panic!("Invalid chain {chain_str}"),
                }
            } else {
                panic!("Invalid chain {chain_str}");
            }
        }
        Err(_) => chains::ETHEREUM.clone(), // default
    };
    let chain = Arc::new(chain);
    let http_rpc_url = get_env_var("RPC_HTTP_URL")?;
    let blockchain_config = BlockchainAdapterConfig::new(http_rpc_url, None, None, true);
    let mut data_client = BlockchainDataClient::new(chain.clone(), blockchain_config);
    data_client.connect().await?;
    data_client.subscribe_blocks().await;

    // Main loop to keep the app running
    loop {
        tokio::select! {
            () = notify.notified() => break,
            () = data_client.process_hypersync_message() => {}
        }
    }
    data_client.disconnect()?;
    Ok(())
}
