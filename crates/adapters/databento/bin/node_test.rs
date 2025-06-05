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
use nautilus_databento::{
    actor::{DatabentoSubscriberActor, DatabentoSubscriberActorConfig},
    factories::{DatabentoDataClientFactory, DatabentoLiveClientConfig},
};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{ClientId, InstrumentId, TraderId};

// Run with `cargo run --bin databento-node-test --features high-precision`

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Initialize Python interpreter only if python feature is enabled
    // #[cfg(feature = "python")]
    pyo3::prepare_freethreaded_python();

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
    }

    // Configure Databento client
    let databento_config = DatabentoLiveClientConfig::new(
        api_key,
        publishers_filepath,
        true, // use_exchange_as_venue
        true, // bars_timestamp_on_close
    );

    let client_factory = DatabentoDataClientFactory::new();

    // Create and register a Databento subscriber actor
    let client_id = ClientId::new("DATABENTO");
    let instrument_ids = vec![
        InstrumentId::from("ESM5.XCME"),
        // Add more instruments as needed
    ];

    // Build the live node with Databento data client
    let mut node = LiveNode::builder(node_name, trader_id, environment)?
        .with_load_state(false)
        .with_save_state(false)
        .add_data_client(None, client_factory, databento_config)?
        .build()?;

    let actor_config = DatabentoSubscriberActorConfig::new(client_id, instrument_ids);
    let actor = DatabentoSubscriberActor::new(actor_config);

    node.add_actor(actor)?;

    node.run().await?;

    Ok(())
}
