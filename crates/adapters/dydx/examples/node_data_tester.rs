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

//! Example demonstrating live data testing with the dYdX adapter.
//!
//! Run with: `cargo run --example dydx-data-tester --package nautilus-dydx`

use std::num::NonZeroUsize;

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_dydx::{config::DydxDataClientConfig, factories::DydxDataClientFactory};
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
    let node_name = "DYDX-DATA-TESTER-001".to_string();
    let instrument_ids = vec![
        InstrumentId::from("BTC-USD-PERP.DYDX"),
        // InstrumentId::from("ETH-USD-PERP.DYDX"),
    ];

    let dydx_config = DydxDataClientConfig {
        is_testnet: false, // Set to true for testnet
        ..Default::default()
    };

    let client_factory = DydxDataClientFactory::new();
    let client_id = ClientId::new("DYDX");

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(dydx_config))?
        .build()?;

    let tester_config = DataTesterConfig::new(client_id, instrument_ids)
        // .with_subscribe_quotes(true)
        // .with_subscribe_trades(true);
        .with_subscribe_book_at_interval(true)
        .with_book_interval_ms(NonZeroUsize::new(10).unwrap());
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
