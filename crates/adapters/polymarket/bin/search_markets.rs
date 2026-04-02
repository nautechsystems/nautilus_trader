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

//! Demonstrates text-based market search using [`SearchFilter`] with the
//! instrument provider.
//!
//! Uses `SearchFilter::from_query()` to search via the Gamma `GET /public-search`
//! endpoint, loading matching instruments through the provider lifecycle.
//!
//! # Usage
//!
//! ```sh
//! cargo run -p nautilus-polymarket --bin polymarket-search-markets
//! cargo run -p nautilus-polymarket --bin polymarket-search-markets -- "world cup"
//! ```

use std::sync::Arc;

use nautilus_common::providers::InstrumentProvider;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::retry::RetryConfig;
use nautilus_polymarket::{
    filters::SearchFilter, http::gamma::PolymarketGammaHttpClient,
    providers::PolymarketInstrumentProvider,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "bitcoin".to_string());

    let http_client = PolymarketGammaHttpClient::new(None, 60, RetryConfig::default())?;

    let filter = SearchFilter::from_query(&query);
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));
    provider.load_all(None).await?;

    let instruments = provider.store().list_all();
    println!("Search \"{query}\" → {} instruments:\n", instruments.len());

    for (i, instrument) in instruments.into_iter().enumerate().take(20) {
        let id = Instrument::id(instrument);
        let expiration = Instrument::expiration_ns(instrument).map_or("N/A".to_string(), |ns| {
            let secs = (ns.as_u64() / 1_000_000_000) as i64;
            chrono::DateTime::from_timestamp(secs, 0).map_or("N/A".to_string(), |dt| {
                dt.format("%Y-%m-%d %H:%M UTC").to_string()
            })
        });

        if let InstrumentAny::BinaryOption(opt) = instrument {
            println!(
                "  {:>2}. {id}\n      outcome:     {}\n      description: {}\n      expiration:  {expiration}\n",
                i + 1,
                opt.outcome.unwrap_or_default(),
                opt.description.unwrap_or_default(),
            );
        } else {
            println!("  {:>2}. {id}\n      expiration: {expiration}\n", i + 1);
        }
    }

    Ok(())
}
