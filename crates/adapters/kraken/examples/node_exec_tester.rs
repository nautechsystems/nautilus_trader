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

//! Example demonstrating live execution testing with the Kraken adapter.
//!
//! Run with: `cargo run -p nautilus-kraken --example kraken-exec-tester`
//!
//! Environment variables (for Spot):
//! - KRAKEN_SPOT_API_KEY: Your Kraken Spot API key
//! - KRAKEN_SPOT_API_SECRET: Your Kraken Spot API secret

use nautilus_common::enums::Environment;
use nautilus_kraken::{
    common::{credential::KrakenCredential, enums::KrakenProductType},
    config::{KrakenDataClientConfig, KrakenExecClientConfig},
    factories::{KrakenDataClientFactory, KrakenExecutionClientFactory},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};

// *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
// *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Configuration - Change product_type to switch between trading modes
    let product_type = KrakenProductType::Futures; // Spot or Futures

    // Symbol and settings based on product type
    let (symbol, order_qty) = match product_type {
        KrakenProductType::Spot => {
            // Spot symbols are normalized to BTC (from Kraken's XBT)
            let symbol = "BTC/USD";
            let order_qty = Quantity::from("0.0001"); // Minimum BTC quantity
            (symbol, order_qty)
        }
        KrakenProductType::Futures => {
            // Futures perpetual symbols use PF_ prefix (e.g., PF_XBTUSD, PF_ETHUSD)
            let symbol = "PF_XBTUSD";
            let order_qty = Quantity::from("0.001");
            (symbol, order_qty)
        }
    };

    let instrument_id = InstrumentId::from(format!("{symbol}.KRAKEN").as_str());

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("KRAKEN-001");
    let node_name = "KRAKEN-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("KRAKEN");

    let credential = match product_type {
        KrakenProductType::Spot => KrakenCredential::resolve_spot(None, None),
        KrakenProductType::Futures => KrakenCredential::resolve_futures(None, None, false),
    }
    .ok_or(
        "API credentials required (set KRAKEN_SPOT_API_KEY/KRAKEN_SPOT_API_SECRET \
         or KRAKEN_FUTURES_API_KEY/KRAKEN_FUTURES_API_SECRET)",
    )?;

    let (api_key, api_secret) = credential.into_parts();

    let data_config = KrakenDataClientConfig {
        api_key: Some(api_key.clone()),
        api_secret: Some(api_secret.clone()),
        product_type,
        ..Default::default()
    };

    let exec_config = KrakenExecClientConfig {
        trader_id,
        account_id,
        api_key,
        api_secret,
        product_type,
        ..Default::default()
    };

    let data_factory = KrakenDataClientFactory::new();
    let exec_factory = KrakenExecutionClientFactory::new();

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
        order_qty,
    )
    .with_subscribe_trades(true)
    .with_subscribe_quotes(true)
    .with_use_post_only(true)
    .with_open_position_on_start(order_qty.as_decimal())
    // .with_tob_offset_ticks(0)
    .with_test_reject_post_only(true)
    .with_log_data(false);

    // Use UUIDs for unique client order IDs across restarts
    tester_config.base.use_uuid_client_order_ids = true;

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
