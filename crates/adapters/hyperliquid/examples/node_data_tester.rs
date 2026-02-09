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

//! Example demonstrating live data testing with the Hyperliquid adapter.
//!
//! Run with: `cargo run --example hyperliquid-data-tester --package nautilus-hyperliquid`

use std::num::NonZeroUsize;

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_hyperliquid::{HyperliquidDataClientConfig, HyperliquidDataClientFactory};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{ClientId, InstrumentId, TraderId},
    stubs::TestDefault,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let node_name = "HYPERLIQUID-DATA-TESTER-001".to_string();
    let instrument_ids = vec![
        InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"),
        // InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"),
    ];

    let hyperliquid_config = HyperliquidDataClientConfig {
        is_testnet: false, // Set to true for testnet
        ..Default::default()
    };

    let client_factory = HyperliquidDataClientFactory::new();
    let client_id = ClientId::new("HYPERLIQUID");

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(hyperliquid_config))?
        .build()?;

    let tester_config = DataTesterConfig::new(client_id, instrument_ids)
        .with_subscribe_quotes(true)
        .with_subscribe_trades(true)
        .with_subscribe_mark_prices(true)
        .with_subscribe_index_prices(true)
        .with_subscribe_funding_rates(true)
        // .with_subscribe_book_at_interval(true)
        .with_book_interval_ms(NonZeroUsize::new(10).unwrap());
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
