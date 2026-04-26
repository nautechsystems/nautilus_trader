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

//! Example demonstrating live data testing with the Binance Spot SBE adapter.
//!
//! Run with: `cargo run --example binance-spot-data-tester --package nautilus-binance --features examples`
//!
//! Requires environment variables based on the configured environment
//! (Ed25519 keys are auto-detected):
//! - Mainnet: `BINANCE_API_KEY` / `BINANCE_API_SECRET`
//! - Testnet: `BINANCE_TESTNET_API_KEY` / `BINANCE_TESTNET_API_SECRET`
//! - Demo: `BINANCE_DEMO_API_KEY` / `BINANCE_DEMO_API_SECRET`

use std::num::NonZeroUsize;

use nautilus_binance::{
    common::enums::{BinanceEnvironment, BinanceProductType},
    config::BinanceDataClientConfig,
    factories::BinanceDataClientFactory,
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{ClientId, InstrumentId, TraderId},
    stubs::TestDefault,
};
use nautilus_network::websocket::TransportBackend;
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let node_name = "BINANCE-TESTER-001".to_string();
    let instrument_ids = vec![
        InstrumentId::from("BTCUSDT.BINANCE"),
        // InstrumentId::from("ETHUSDT.BINANCE"),
    ];

    let binance_config = BinanceDataClientConfig {
        product_types: vec![BinanceProductType::Spot],
        environment: BinanceEnvironment::Mainnet,
        api_key: None,
        api_secret: None,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let client_factory = BinanceDataClientFactory::new();
    let client_id = ClientId::new("BINANCE");

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(binance_config))?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .subscribe_book_at_interval(true)
        .book_interval_ms(NonZeroUsize::new(10).unwrap())
        .manage_book(true)
        .build();
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
