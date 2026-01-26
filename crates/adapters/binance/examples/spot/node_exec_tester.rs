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

//! Example demonstrating live execution testing with the Binance Spot adapter.
//!
//! Run with: `cargo run --example binance-spot-exec-tester --package nautilus-binance`
//!
//! Requires environment variables:
//! - BINANCE_API_KEY: Your Binance API key
//! - BINANCE_API_SECRET: Your Binance API secret
//!
//! Optional environment variables (for SBE data streams):
//! - BINANCE_ED25519_API_KEY
//! - BINANCE_ED25519_API_SECRET

use nautilus_binance::{
    common::enums::{BinanceEnvironment, BinanceProductType},
    config::{BinanceDataClientConfig, BinanceExecClientConfig},
    factories::{BinanceDataClientFactory, BinanceExecutionClientFactory},
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BINANCE-001");
    let node_name = "BINANCE-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("BINANCE");
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    let data_config = BinanceDataClientConfig {
        product_types: vec![BinanceProductType::Spot],
        environment: BinanceEnvironment::Mainnet,
        api_key: None,
        api_secret: None,
        ed25519_api_key: None,
        ed25519_api_secret: None,
        ..Default::default()
    };

    let exec_config = BinanceExecClientConfig {
        trader_id,
        account_id,
        product_types: vec![BinanceProductType::Spot],
        environment: BinanceEnvironment::Mainnet,
        api_key: None,    // Will use 'BINANCE_API_KEY' env var
        api_secret: None, // Will use 'BINANCE_API_SECRET' env var
        base_url_http: None,
        base_url_ws: None,
    };

    let data_factory = BinanceDataClientFactory::new();
    let exec_factory = BinanceExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_timeout_connection(10)
        .with_delay_post_stop_secs(5)
        .build()?;

    let mut tester_config = ExecTesterConfig::new(
        StrategyId::from("EXEC_TESTER-001"),
        instrument_id,
        client_id,
        Quantity::from("0.0001"), // Small quantity for testing
    )
    .with_log_data(false)
    .with_enable_limit_sells(false)
    .with_close_positions_on_stop(false);

    // Use UUIDs for unique client order IDs across restarts
    tester_config.base.use_uuid_client_order_ids = true;

    tester_config.base.external_order_claims = Some(vec![instrument_id]);
    tester_config.use_post_only = true;

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
