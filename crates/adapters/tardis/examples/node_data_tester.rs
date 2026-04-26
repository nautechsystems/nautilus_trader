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
use nautilus_model::identifiers::{ClientId, InstrumentId, TraderId};
use nautilus_tardis::{
    common::enums::TardisExchange, config::TardisDataClientConfig,
    factories::TardisDataClientFactory, machine::types::ReplayNormalizedRequestOptions,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let options = vec![ReplayNormalizedRequestOptions {
        exchange: TardisExchange::BinanceFutures,
        symbols: Some(vec!["BTCUSDT".to_string()]),
        from: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        to: NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
        data_types: vec!["trade".to_string(), "book_change".to_string()],
        with_disconnect_messages: Some(false),
    }];

    let tardis_config = TardisDataClientConfig {
        options,
        ..Default::default()
    };

    let client_id = ClientId::new("TARDIS");
    let instrument_ids = vec![InstrumentId::from("BTCUSDT-PERP.BINANCE")];

    let mut node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)?
        .with_name("TARDIS-SANDBOX")
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
        .build();
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
