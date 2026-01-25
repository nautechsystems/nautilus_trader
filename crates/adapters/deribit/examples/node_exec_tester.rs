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

//! Example demonstrating live execution testing with the Deribit adapter.
//!
//! Run with: `cargo run --example deribit-exec-tester --package nautilus-deribit`
//!
//! For production, set USE_TESTNET=false:
//! `USE_TESTNET=false cargo run --example deribit-exec-tester --package nautilus-deribit`
//!
//! Environment variables:
//! - DERIBIT_TESTNET_API_KEY / DERIBIT_API_KEY: Your Deribit API key
//! - DERIBIT_TESTNET_API_SECRET / DERIBIT_API_SECRET: Your Deribit API secret

use nautilus_common::enums::Environment;
use nautilus_deribit::{
    config::{DeribitDataClientConfig, DeribitExecClientConfig},
    factories::{DeribitDataClientFactory, DeribitExecutionClientFactory},
    http::models::DeribitInstrumentKind,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Read USE_TESTNET from environment (default true for safety)
    let use_testnet = std::env::var("USE_TESTNET")
        .map(|v| v.to_lowercase() != "false")
        .unwrap_or(true);

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("DERIBIT-001");
    let node_name = "DERIBIT-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("DERIBIT");
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");

    let data_config = DeribitDataClientConfig {
        api_key: None,    // Will use env var
        api_secret: None, // Will use env var
        instrument_kinds: vec![DeribitInstrumentKind::Future],
        use_testnet,
        ..Default::default()
    };

    let exec_config = DeribitExecClientConfig {
        trader_id,
        account_id,
        api_key: None,    // Will use env var
        api_secret: None, // Will use env var
        instrument_kinds: vec![DeribitInstrumentKind::Future],
        use_testnet,
        ..Default::default()
    };

    let data_factory = DeribitDataClientFactory::new();
    let exec_factory = DeribitExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let mut tester_config = ExecTesterConfig::new(
        StrategyId::from("EXEC_TESTER-001"),
        instrument_id,
        client_id,
        Quantity::from("10"), // 10 USD contracts (Deribit minimum)
    )
    .with_subscribe_trades(true)
    .with_subscribe_quotes(true)
    .with_use_post_only(true)
    .with_log_data(false);

    // Use UUIDs for unique client order IDs across restarts
    tester_config.base.use_uuid_client_order_ids = true;

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
