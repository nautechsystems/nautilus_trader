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

//! Live Kalshi demo data testing with the built-in `DataTester` actor.
//!
//! Connects to the demo environment and subscribes to quotes, trades, full L2 books,
//! trading status, and final market closes for two markets in one event. It also
//! requests a book snapshot and the last hour of trades for each market, and the last
//! hour of one-minute candles for the first market. No orders are placed.
//!
//! Edit the instrument constants to target open demo markets.
//!
//! Run with:
//! `cargo run --locked -p nautilus-kalshi --example kalshi-data-tester`
//!
//! Demo credentials are read from the environment when both config fields are absent:
//! - `KALSHI_DEMO_API_KEY_ID`
//! - `KALSHI_DEMO_PRIVATE_KEY`

use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_kalshi::{
    common::{consts::KALSHI_CLIENT_ID, enums::KalshiEnvironment},
    config::KalshiDataClientConfig,
    factories::KalshiDataClientFactory,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::bar::BarType,
    identifiers::{InstrumentId, TraderId},
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "KALSHI-DATA-TESTER-001";
const INSTRUMENT_ID: &str = "KXNBA1HTOTAL-26NOV01LACGSW-125.KALSHI";
const SECOND_INSTRUMENT_ID: &str = "KXNBA1HTOTAL-26NOV01LACGSW-97.KALSHI";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let trader_id = TraderId::from(TRADER_ID);
    let client_id = *KALSHI_CLIENT_ID;
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);
    let second_instrument_id = InstrumentId::from(SECOND_INSTRUMENT_ID);
    let instrument_ids = vec![instrument_id, second_instrument_id];
    let bar_type = BarType::from(format!("{INSTRUMENT_ID}-1-MINUTE-LAST-EXTERNAL").as_str());

    let data_config = KalshiDataClientConfig {
        environment: KalshiEnvironment::Demo,
        instrument_ids: instrument_ids.clone(),
        ..Default::default()
    };

    let log_config = LoggerConfig {
        stdout_level: log::LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name(NODE_NAME.to_string())
        .with_load_state(false)
        .with_save_state(false)
        .with_logging(log_config)
        .add_data_client(
            None,
            Box::new(KalshiDataClientFactory),
            Box::new(data_config),
        )?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .bar_types(vec![bar_type])
        .subscribe_book_deltas(true)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .subscribe_instrument_status(true)
        .subscribe_instrument_close(true)
        .request_trades(true)
        .request_bars(true)
        .request_book_snapshot(true)
        .manage_book(true)
        .log_data(true)
        .build()?;
    node.add_actor(DataTester::new(tester_config))?;
    node.run().await?;
    Ok(())
}
