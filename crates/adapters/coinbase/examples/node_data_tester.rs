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

//! Example demonstrating live data testing with the Coinbase adapter.
//!
//! Run with: `cargo run --example coinbase-data-tester --package nautilus-coinbase --features examples`
//!
//! Environment variables (optional for public market data):
//! - `COINBASE_API_KEY`: CDP API key name (`organizations/{org_id}/apiKeys/{key_id}`)
//! - `COINBASE_API_SECRET`: PEM-encoded EC private key

use nautilus_coinbase::{config::CoinbaseDataClientConfig, factories::CoinbaseDataClientFactory};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::bar::BarType,
    identifiers::{ClientId, InstrumentId, TraderId},
    stubs::TestDefault,
};
use nautilus_network::websocket::TransportBackend;
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

// *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
// *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let node_name = "COINBASE-TESTER-001".to_string();
    let client_id = ClientId::new("COINBASE");

    let instrument_ids = vec![
        InstrumentId::from("BTC-USD.COINBASE"),
        // InstrumentId::from("ETH-USD.COINBASE"),
    ];

    let bar_types: Vec<BarType> = instrument_ids
        .iter()
        .map(|id| BarType::from(format!("{id}-1-MINUTE-LAST-EXTERNAL").as_str()))
        .collect();

    let coinbase_config = CoinbaseDataClientConfig {
        api_key: None,    // Will use 'COINBASE_API_KEY' env var if available
        api_secret: None, // Will use 'COINBASE_API_SECRET' env var if available
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let client_factory = CoinbaseDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_load_state(false)
        .with_save_state(false)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(coinbase_config))?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .subscribe_book_deltas(true)
        .bar_types(bar_types)
        .subscribe_bars(true)
        .request_bars(true)
        .request_book_snapshot(true)
        .manage_book(true)
        .build();

    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
