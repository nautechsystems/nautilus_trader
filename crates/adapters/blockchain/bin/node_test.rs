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
    config::BlockchainAdapterConfig,
    factories::{BlockchainClientConfig, BlockchainDataClientFactory},
};
use nautilus_common::enums::Environment;
use nautilus_core::env::get_env_var;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    defi::chain::{Blockchain, Chain, chains},
    identifiers::TraderId,
};
use tokio::time::Duration;

// Run with `cargo run -p nautilus-blockchain --bin node_test --features hypersync,python`

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Initialize Python interpreter only if python feature is enabled
    // #[cfg(feature = "python")]
    pyo3::prepare_freethreaded_python();

    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::default();
    let node_name = "TESTER-001".to_string();

    let chain: Chain = match std::env::var("CHAIN")
        .ok()
        .and_then(|s| s.parse::<Blockchain>().ok())
    {
        Some(Blockchain::Ethereum) => chains::ETHEREUM.clone(),
        Some(Blockchain::Base) => chains::BASE.clone(),
        Some(Blockchain::Arbitrum) => chains::ARBITRUM.clone(),
        Some(Blockchain::Polygon) => chains::POLYGON.clone(),
        _ => {
            println!("⚠️  No valid CHAIN env var found, using Ethereum as default");
            chains::ETHEREUM.clone()
        }
    };

    let chain = Arc::new(chain);
    println!("   - Using chain: {}", chain.name);

    // Try to get RPC URLs from environment, fallback to test values if not available
    let http_rpc_url = get_env_var("RPC_HTTP_URL").unwrap_or_else(|_| {
        println!("⚠️  RPC_HTTP_URL not found, using placeholder");
        "https://eth-mainnet.example.com".to_string()
    });
    let wss_rpc_url = get_env_var("RPC_WSS_URL").ok();

    let blockchain_config = BlockchainAdapterConfig::new(
        http_rpc_url,
        None, // HyperSync URL not needed for this test
        wss_rpc_url,
        false, // Don't cache locally for this test
    );

    let client_factory = Box::new(BlockchainDataClientFactory::new());
    let client_config = BlockchainClientConfig::new(blockchain_config, chain.clone());

    let mut node = LiveNode::builder(node_name, trader_id, environment)?
        .with_load_state(false)
        .with_save_state(false)
        .add_data_client(
            None, // Use factory name
            client_factory,
            Box::new(client_config),
        )?
        .build()?;

    node.start().await?;

    // Let it run briefly to ensure all components are properly initialized
    tokio::time::sleep(Duration::from_millis(100)).await;

    node.stop().await?;

    Ok(())
}

#[cfg(not(feature = "hypersync"))]
fn main() {
    println!("⚠️  kernel_test binary requires the 'hypersync' feature to be enabled.");
    println!("   Run with: cargo run --bin kernel_test --features hypersync");
    std::process::exit(1);
}
