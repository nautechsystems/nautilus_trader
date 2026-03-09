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

use nautilus_common::providers::InstrumentProvider;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_polymarket::{
    http::gamma::PolymarketGammaHttpClient, providers::PolymarketInstrumentProvider,
};

const PERIOD_SECS: u64 = 15 * 60; // 15 minutes
const ASSETS: &[&str] = &["btc", "eth", "sol", "xrp"];
const NUM_PERIODS: u64 = 3; // current + next 2

fn build_updown_slugs() -> Vec<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_secs();

    // Align to current 15-minute period start
    let period_start = (now / PERIOD_SECS) * PERIOD_SECS;

    let mut slugs = Vec::new();
    for i in 0..NUM_PERIODS {
        let timestamp = period_start + i * PERIOD_SECS;
        for asset in ASSETS {
            slugs.push(format!("{asset}-updown-15m-{timestamp}"));
        }
    }
    slugs
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let http_client = PolymarketGammaHttpClient::new(None, None)?;
    let mut provider = PolymarketInstrumentProvider::new(http_client);

    let slugs = build_updown_slugs();
    provider.load_by_slugs(slugs).await?;

    let instruments = provider.store().list_all();
    println!("Loaded {} instruments:\n", instruments.len());

    for instrument in instruments {
        let id = Instrument::id(instrument);
        let expiration =
            Instrument::expiration_ns(instrument).map_or("N/A".to_string(), |ns| format!("{ns}"));

        if let InstrumentAny::BinaryOption(opt) = instrument {
            println!(
                "  {id}\n    outcome:     {}\n    description: {}\n    expiration:  {expiration}\n",
                opt.outcome.unwrap_or_default(),
                opt.description.unwrap_or_default(),
            );
        } else {
            println!("  {id}\n    expiration: {expiration}\n");
        }
    }

    Ok(())
}
