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
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_core::{UUID4, env::get_env_var};
use nautilus_live::node::TradingNode;
use nautilus_model::{
    defi::chain::{Blockchain, Chain, chains},
    identifiers::TraderId,
};
use nautilus_system::{
    config::NautilusKernelConfig,
    factories::{DataClientFactory, DataClientFactoryRegistry},
};
use tokio::time::{Duration, sleep};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::default();

    let kernel_config = NautilusKernelConfig::new(
        environment,
        trader_id,
        Some(false),                   // load_state
        Some(false),                   // save_state
        None,                          // timeout_connection
        None,                          // timeout_reconciliation
        None,                          // timeout_portfolio
        None,                          // timeout_disconnection
        None,                          // timeout_post_stop
        None,                          // timeout_shutdown
        Some(LoggerConfig::default()), // logging
        Some(UUID4::new()),            // instance_id
        None,                          // cache
        None,                          // msgbus
        None,                          // data_engine
        None,                          // risk_engine
        None,                          // exec_engine
        None,                          // portfolio
        None,                          // streaming
    );

    let mut node = TradingNode::build("TESTER-001".to_string(), Some(kernel_config))?;

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

    // Create blockchain client using the factory pattern
    let blockchain_client_config = BlockchainClientConfig::new(blockchain_config, chain.clone());
    let blockchain_factory = BlockchainDataClientFactory::new();

    println!("✅ Blockchain factory created");
    println!("   - Factory name: {}", blockchain_factory.name());
    println!("   - Config type: {}", blockchain_factory.config_type());

    // Test factory registry
    let mut factory_registry = DataClientFactoryRegistry::new();
    factory_registry.register("blockchain".to_string(), Box::new(blockchain_factory))?;

    println!("✅ Factory registered with registry");
    println!("   - Registered factories: {:?}", factory_registry.names());

    // TODO: Move this to node builder
    // Create client through factory
    let factory = factory_registry.get("blockchain").unwrap();
    let _blockchain_client = factory.create(
        "blockchain-ethereum",
        &blockchain_client_config,
        node.kernel().cache(),
        node.kernel().clock(),
    )?;

    // TODO: Improve this API
    // Note: We're not connecting to avoid requiring actual RPC endpoints for basic testing
    // blockchain_client.connect().await?;
    // trading_node.kernel().data_engine().register_client(Box::new(blockchain_client)).await?;

    // Test trading node lifecycle (start/stop)
    println!("\n🎮 Testing trading node lifecycle...");

    println!("   - Starting trading node...");
    node.start().await?;
    println!("   - Trading node started, running: {}", node.is_running());

    // Let it run briefly
    sleep(Duration::from_millis(100)).await;

    println!("   - Stopping trading node...");
    node.stop().await?;
    println!("   - Trading node stopped, running: {}", node.is_running());

    Ok(())
}

#[cfg(not(feature = "hypersync"))]
fn main() {
    println!("⚠️  kernel_test binary requires the 'hypersync' feature to be enabled.");
    println!("   Run with: cargo run --bin kernel_test --features hypersync");
    std::process::exit(1);
}
