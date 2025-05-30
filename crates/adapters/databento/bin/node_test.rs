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

use std::path::PathBuf;

use nautilus_common::enums::Environment;
use nautilus_core::env::get_env_var;
use nautilus_databento::factories::{DatabentoDataClientFactory, DatabentoLiveClientConfig};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::TraderId;
use tokio::time::Duration;

// Run with `cargo run -p nautilus-databento --bin databento-node-test --features live`

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Initialize Python interpreter only if python feature is enabled
    // #[cfg(feature = "python")]
    pyo3::prepare_freethreaded_python();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::default();
    let node_name = "DATABENTO-TESTER-001".to_string();

    // Get Databento API key from environment
    let api_key = get_env_var("DATABENTO_API_KEY").unwrap_or_else(|_| {
        println!("⚠️  DATABENTO_API_KEY not found, using placeholder");
        "db-placeholder-key".to_string()
    });

    // Determine publishers file path
    let publishers_filepath = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json");
    if !publishers_filepath.exists() {
        println!(
            "⚠️  Publishers file not found at: {}",
            publishers_filepath.display()
        );
        println!("   This is expected in CI/test environments");
    }

    // Configure Databento client
    let databento_config = DatabentoLiveClientConfig::new(
        api_key,
        publishers_filepath,
        true, // use_exchange_as_venue
        true, // bars_timestamp_on_close
    );

    let client_factory = Box::new(DatabentoDataClientFactory::new());

    // Build the live node with Databento data client
    let mut node = LiveNode::builder(node_name, trader_id, environment)?
        .with_load_state(false)
        .with_save_state(false)
        .add_data_client(
            Some("DATABENTO".to_string()), // Use custom name
            client_factory,
            Box::new(databento_config),
        )?
        .build()?;

    node.start().await?;

    // Let it run briefly to ensure all components are properly initialized
    tokio::time::sleep(Duration::from_millis(100)).await;

    node.stop().await?;

    Ok(())
}

#[cfg(not(feature = "live"))]
fn main() {
    println!("⚠️  databento-node-test binary requires the 'live' feature to be enabled.");
    println!(
        "   Run with: cargo run -p nautilus-databento --bin databento-node-test --features live"
    );
    std::process::exit(1);
}
