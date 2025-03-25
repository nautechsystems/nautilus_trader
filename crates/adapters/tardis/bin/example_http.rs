// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use chrono::{TimeZone, Utc};
use nautilus_model::instruments::Instrument;
use nautilus_tardis::{
    enums::Exchange,
    http::{client::TardisHttpClient, query::InstrumentFilterBuilder},
};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let client = TardisHttpClient::new(None, None, None, true).unwrap();

    // Tardis instrument definitions
    let resp = client.instruments_info(Exchange::Binance, None, None).await;
    println!("Received: {resp:?}");

    let start = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let filter = InstrumentFilterBuilder::default()
        .available_since(Some(start))
        .build()
        .unwrap();

    let resp = client
        .instruments_info(Exchange::Binance, Some("BTCUSDT"), Some(&filter))
        .await;
    println!("Received: {resp:?}");

    // Nautilus instrument definitions
    let resp = client
        .instruments(Exchange::Deribit, None, None, None, None)
        .await;
    println!("Received: {resp:?}");

    for inst in resp.unwrap() {
        println!("{}", inst.id());
    }
}
