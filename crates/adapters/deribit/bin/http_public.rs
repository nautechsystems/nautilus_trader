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

use std::env;

use nautilus_deribit::http::{client::DeribitHttpClient, models::DeribitCurrency};
use nautilus_model::identifiers::InstrumentId;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    let args: Vec<String> = env::args().collect();
    let is_testnet = args.iter().any(|a| a == "--testnet");

    // Create HTTP client
    let client = DeribitHttpClient::new(is_testnet, None, None, None, None, None)?;

    // Fetch BTC-PERPETUAL instrument
    tracing::info!("Fetching BTC-PERPETUAL instrument...");
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let instrument = client.request_instrument(instrument_id).await?;
    println!("Single instrument:");
    println!("{instrument:?}\n");

    // Fetch BTC instruments
    tracing::info!("Fetching BTC instruments...");
    let instruments = client
        .request_instruments(DeribitCurrency::BTC, None)
        .await?;
    println!("First 2 instruments from BTC:");
    for (i, inst) in instruments.iter().take(2).enumerate() {
        let num = i + 1;
        println!("{num}. {inst:?}");
    }

    Ok(())
}
