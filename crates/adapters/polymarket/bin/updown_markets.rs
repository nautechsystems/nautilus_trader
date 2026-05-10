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

//! Demonstrates dynamic instrument loading with [`MarketSlugFilter`].
//!
//! Polymarket "Up/Down" markets follow a predictable slug pattern:
//! `{asset}-updown-15m-{unix_timestamp}`, where the timestamp is aligned to
//! 15-minute boundaries. This binary loads instruments for the current period
//! and the next two upcoming periods across BTC, ETH, SOL, and XRP.
//!
//! Because [`MarketSlugFilter`] accepts a closure, the slug list is
//! re-evaluated on each `load_all()` call — so a long-running process can
//! call `load_all()` periodically and always get the latest time window
//! without rebuilding the filter.
//!
//! # Usage
//!
//! ```sh
//! cargo run -p nautilus-polymarket --bin updown_markets
//! ```

use std::sync::Arc;

use nautilus_common::providers::InstrumentProvider;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::retry::RetryConfig;
use nautilus_polymarket::{
    filters::MarketSlugFilter, http::gamma::PolymarketGammaHttpClient,
    providers::PolymarketInstrumentProvider,
};

const PERIOD_SECS: u64 = 15 * 60; // 15 minutes
const ASSETS: &[&str] = &["btc", "eth", "sol", "xrp"];
const NUM_PERIODS: u64 = 3; // current + next 2

/// Generates market slugs for the current and next [`NUM_PERIODS`] 15-minute
/// Up/Down windows across all configured [`ASSETS`].
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

    let http_client = PolymarketGammaHttpClient::new(None, 60, RetryConfig::default())?;
    let filter = MarketSlugFilter::new(build_updown_slugs);
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));
    provider.load_all(None).await?;

    let instruments = provider.store().list_all();
    println!("Loaded {} instruments:\n", instruments.len());

    for instrument in instruments {
        let id = Instrument::id(instrument);
        let expiration = Instrument::expiration_ns(instrument).map_or("N/A".to_string(), |ns| {
            let secs = (ns.as_u64() / 1_000_000_000) as i64;
            chrono::DateTime::from_timestamp(secs, 0).map_or("N/A".to_string(), |dt| {
                dt.format("%Y-%m-%d %H:%M UTC").to_string()
            })
        });

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
