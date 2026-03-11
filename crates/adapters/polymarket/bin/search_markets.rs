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

//! Demonstrates text-based market search with [`PolymarketDataLoader`] and [`SearchFilter`].
//!
//! This example shows two approaches to free-text search:
//!
//! 1. **Data loader** — Standalone `PolymarketDataLoader::search()` for quick
//!    one-off queries that return instruments directly.
//! 2. **Search filter** — `SearchFilter` plugged into the provider for
//!    integration with the instrument provider lifecycle.
//!
//! Both use the Gamma `GET /public-search` endpoint under the hood. The raw
//! `GammaMarket` objects contain enriched fields (best_bid, best_ask, spread,
//! volume_1wk, etc.) that are printed alongside the parsed instruments.
//!
//! # Usage
//!
//! ```sh
//! cargo run -p nautilus-polymarket --bin polymarket-search-markets
//! cargo run -p nautilus-polymarket --bin polymarket-search-markets -- "world cup"
//! ```

use nautilus_common::providers::InstrumentProvider;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_polymarket::{
    filters::SearchFilter,
    http::{gamma::PolymarketGammaHttpClient, query::GetSearchParams},
    loader::PolymarketDataLoader,
    providers::PolymarketInstrumentProvider,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "bitcoin".to_string());

    let http_client = PolymarketGammaHttpClient::new(None, None)?;

    // ---- Approach 1: Data Loader (standalone, no caching) ----
    println!("=== Data Loader: search(\"{query}\") ===\n");

    let loader = PolymarketDataLoader::new(http_client.clone());

    // Fetch raw search results to inspect enriched GammaMarket fields
    let raw_response = http_client
        .inner()
        .get_public_search(GetSearchParams {
            q: Some(query.clone()),
            limit_per_type: Some(5),
            ..Default::default()
        })
        .await?;

    if let Some(markets) = &raw_response.markets {
        println!("Raw market results ({} found):\n", markets.len());
        for (i, m) in markets.iter().enumerate() {
            println!("  {:>2}. {}", i + 1, m.question);
            println!(
                "      slug:      {}",
                m.market_slug.as_deref().unwrap_or("N/A")
            );
            println!("      active:    {}", m.active.unwrap_or(false));
            println!(
                "      best_bid:  {}",
                m.best_bid.map_or("N/A".into(), |v| format!("{v:.4}"))
            );
            println!(
                "      best_ask:  {}",
                m.best_ask.map_or("N/A".into(), |v| format!("{v:.4}"))
            );
            println!(
                "      spread:    {}",
                m.spread.map_or("N/A".into(), |v| format!("{v:.4}"))
            );
            println!(
                "      vol_24hr:  {}",
                m.volume_24hr.map_or("N/A".into(), |v| format!("{v:.0}"))
            );
            println!(
                "      vol_1wk:   {}",
                m.volume_1wk.map_or("N/A".into(), |v| format!("{v:.0}"))
            );
            println!(
                "      vol_1mo:   {}",
                m.volume_1mo.map_or("N/A".into(), |v| format!("{v:.0}"))
            );
            println!(
                "      liquidity: {}",
                m.liquidity_num.map_or("N/A".into(), |v| format!("{v:.0}"))
            );
            println!(
                "      category:  {}",
                m.category.as_deref().unwrap_or("N/A")
            );
            println!(
                "      rewards:   {}",
                if m.rewards_min_size.is_some() {
                    "yes"
                } else {
                    "no"
                }
            );
            println!();
        }
    }

    if let Some(events) = &raw_response.events {
        println!("Raw event results ({} found):\n", events.len());
        for (i, e) in events.iter().enumerate() {
            println!(
                "  {:>2}. {} (slug: {})",
                i + 1,
                e.title.as_deref().unwrap_or("N/A"),
                e.slug.as_deref().unwrap_or("N/A"),
            );
            println!("      markets:      {}", e.markets.len());
            println!(
                "      liquidity:    {}",
                e.liquidity.map_or("N/A".into(), |v| format!("{v:.0}"))
            );
            println!(
                "      volume:       {}",
                e.volume.map_or("N/A".into(), |v| format!("{v:.0}"))
            );
            println!(
                "      vol_24hr:     {}",
                e.volume_24hr.map_or("N/A".into(), |v| format!("{v:.0}"))
            );
            println!(
                "      category:     {}",
                e.category.as_deref().unwrap_or("N/A")
            );
            println!("      featured:     {}", e.featured.unwrap_or(false));
            println!("      neg_risk:     {}", e.neg_risk.unwrap_or(false));
            println!();
        }
    }

    // Also parse into Nautilus instruments via the loader
    let instruments = loader.search(&query).await?;
    println!(
        "Parsed {} Nautilus instruments from search\n",
        instruments.len()
    );

    // ---- Approach 2: Search Filter (provider lifecycle) ----
    println!("=== Search Filter: provider.load_all() ===\n");

    let filter = SearchFilter::from_query(&query);
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Box::new(filter));
    provider.load_all(None).await?;

    let stored = provider.store().list_all();
    println!("Provider loaded {} instruments:\n", stored.len());

    for (i, instrument) in stored.into_iter().enumerate().take(20) {
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
