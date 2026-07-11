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

//! Example demonstrating live data testing with the Polymarket adapter.
//!
//! Connects to Polymarket via WebSocket, loads the configured event's
//! instruments from the Gamma API, then subscribes to trade ticks for the
//! selected market tokens.
//!
//! Edit the constants below to change the target event and market tokens.
//!
//! Run with: `cargo run --example polymarket-data-tester --package nautilus-polymarket --features examples`
//!
//! Credentials are read from the environment when set:
//! - `POLYMARKET_API_KEY`.
//! - `POLYMARKET_API_SECRET`.
//! - `POLYMARKET_PASSPHRASE`.

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{InstrumentId, TraderId};
use nautilus_polymarket::{
    common::consts::POLYMARKET_CLIENT_ID,
    config::{PolymarketDataClientConfig, PolymarketInstrumentProviderConfig},
    factories::PolymarketDataClientFactory,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "POLYMARKET-DATA-TESTER-001";
const EVENT_SLUG: &str = "fed-decision-in-september-762";

// Fed Decision in September (Yes/No)
// https://polymarket.com/event/fed-decision-in-september-762
// These IDs select both outcomes; the provider loads their metadata at startup
const INSTRUMENT_ID_YES: &str = "0xac02cbb049e46d6a3627c0fdf52fa554982a9025d45968207b362acb6ca4b830-57748138085022719760345772310040703848567377822400132842014290209986511882046.POLYMARKET";
const INSTRUMENT_ID_NO: &str = "0xac02cbb049e46d6a3627c0fdf52fa554982a9025d45968207b362acb6ca4b830-28239418772633645184924651434956000849078365566842629564562475378531350731731.POLYMARKET";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from(TRADER_ID);
    let node_name = NODE_NAME.to_string();

    let instrument_ids = vec![
        InstrumentId::from(INSTRUMENT_ID_YES),
        InstrumentId::from(INSTRUMENT_ID_NO),
    ];

    let polymarket_config = PolymarketDataClientConfig {
        instrument_config: Some(PolymarketInstrumentProviderConfig {
            event_slugs: Some(vec![EVENT_SLUG.to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let client_factory = PolymarketDataClientFactory;
    let client_id = *POLYMARKET_CLIENT_ID;

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(polymarket_config))?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .subscribe_trades(true)
        .subscribe_quotes(true)
        .manage_book(true)
        .build()?;
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
