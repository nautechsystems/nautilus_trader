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

//! Sandbox example replaying Tardis Machine data through a LiveNode.
//!
//! Edit the constants below to change the replay request and subscribed instrument.
//!
//! Run with: `cargo run --example tardis-data-tester -p nautilus-tardis --features examples`
//!
//! Prerequisites:
//! - Set `TARDIS_API_KEY` (used by the HTTP client to fetch instrument metadata)
//! - Set `TM_API_KEY` (used by the tardis-machine Docker container)
//! - Set `TARDIS_MACHINE_WS_URL=ws://localhost:8001`
//! - `docker run --platform linux/amd64 -p 8000:8000 -p 8001:8001 -e TM_API_KEY -d tardisdev/tardis-machine`

use chrono::NaiveDate;
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{InstrumentId, TraderId};
use nautilus_tardis::{
    common::{consts::TARDIS_CLIENT_ID, enums::TardisExchange},
    config::TardisDataClientConfig,
    factories::TardisDataClientFactory,
    machine::types::ReplayNormalizedRequestOptions,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "TARDIS-SANDBOX";
const INSTRUMENT_ID: &str = "BTCUSDT-PERP.BINANCE";

const REPLAY_EXCHANGE: TardisExchange = TardisExchange::BinanceFutures;
const REPLAY_SYMBOL: &str = "BTCUSDT";
const REPLAY_FROM: (i32, u32, u32) = (2024, 1, 1);
const REPLAY_TO: (i32, u32, u32) = (2024, 1, 2);
const REPLAY_DATA_TYPES: [&str; 2] = ["trade", "book_change"];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let options = vec![ReplayNormalizedRequestOptions {
        exchange: REPLAY_EXCHANGE,
        symbols: Some(vec![REPLAY_SYMBOL.to_string()]),
        from: NaiveDate::from_ymd_opt(REPLAY_FROM.0, REPLAY_FROM.1, REPLAY_FROM.2).unwrap(),
        to: NaiveDate::from_ymd_opt(REPLAY_TO.0, REPLAY_TO.1, REPLAY_TO.2).unwrap(),
        data_types: REPLAY_DATA_TYPES.iter().map(|s| s.to_string()).collect(),
        with_disconnect_messages: Some(false),
    }];

    let tardis_config = TardisDataClientConfig {
        options,
        ..Default::default()
    };

    let client_id = *TARDIS_CLIENT_ID;
    let instrument_ids = vec![InstrumentId::from(INSTRUMENT_ID)];

    let mut node = LiveNode::builder(TraderId::from(TRADER_ID), Environment::Sandbox)?
        .with_name(NODE_NAME)
        .with_delay_post_stop_secs(2)
        .add_data_client(
            None,
            Box::new(TardisDataClientFactory::new()),
            Box::new(tardis_config),
        )?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(instrument_ids)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .subscribe_mark_prices(true)
        .subscribe_index_prices(true)
        .subscribe_funding_rates(true)
        // .subscribe_book_deltas(true)
        .manage_book(true)
        .build()?;
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
