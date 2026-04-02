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

//! Demonstrates event-based market discovery using the provider and raw HTTP client.
//!
//! This example exercises three features:
//!
//! 1. **Tags** — `provider.list_tags()` to list available event categories.
//! 2. **Raw event query** — `provider.http_client().inner().get_gamma_events()`
//!    to inspect enriched `GammaEvent` fields (liquidity, volume, category).
//! 3. **`EventParamsFilter`** — Provider integration that fetches instruments
//!    from events matching the query params.
//!
//! # Usage
//!
//! ```sh
//! cargo run -p nautilus-polymarket --bin polymarket-event-discovery
//! ```

use std::sync::Arc;

use nautilus_common::providers::InstrumentProvider;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::retry::RetryConfig;
use nautilus_polymarket::{
    filters::EventParamsFilter,
    http::{gamma::PolymarketGammaHttpClient, query::GetGammaEventsParams},
    providers::PolymarketInstrumentProvider,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let http_client = PolymarketGammaHttpClient::new(None, 60, RetryConfig::default())?;
    let provider = PolymarketInstrumentProvider::new(http_client.clone());

    // ---- Step 1: List available tags ----
    println!("=== Available Tags ===\n");

    let tags = provider.list_tags().await?;
    for (i, tag) in tags.iter().enumerate() {
        println!(
            "  {:>2}. {} (slug: {}, id: {})",
            i + 1,
            tag.label.as_deref().unwrap_or("N/A"),
            tag.slug.as_deref().unwrap_or("N/A"),
            tag.id,
        );
    }
    println!("\nTotal: {} tags\n", tags.len());

    // ---- Step 2: Query events with full params via raw HTTP client ----
    println!("=== Featured High-Volume Events ===\n");

    let event_params = GetGammaEventsParams {
        active: Some(true),
        closed: Some(false),
        order: Some("volume".into()),
        ascending: Some(false),
        limit: Some(10),
        ..Default::default()
    };

    let events = provider
        .http_client()
        .inner()
        .get_gamma_events(event_params)
        .await?;
    println!(
        "Found {} events (active, sorted by volume desc):\n",
        events.len()
    );

    for (i, event) in events.iter().enumerate() {
        println!(
            "  {:>2}. {}",
            i + 1,
            event.title.as_deref().unwrap_or("N/A"),
        );
        println!(
            "      slug:         {}",
            event.slug.as_deref().unwrap_or("N/A")
        );
        println!("      markets:      {}", event.markets.len());
        println!(
            "      liquidity:    {}",
            event.liquidity.map_or("N/A".into(), |v| format!("{v:.0}"))
        );
        println!(
            "      volume:       {}",
            event.volume.map_or("N/A".into(), |v| format!("{v:.0}"))
        );
        println!(
            "      vol_24hr:     {}",
            event
                .volume_24hr
                .map_or("N/A".into(), |v| format!("{v:.0}"))
        );
        println!(
            "      open_int:     {}",
            event
                .open_interest
                .map_or("N/A".into(), |v| format!("{v:.0}"))
        );
        println!(
            "      category:     {}",
            event.category.as_deref().unwrap_or("N/A")
        );
        println!("      featured:     {}", event.featured.unwrap_or(false));
        println!("      neg_risk:     {}", event.neg_risk.unwrap_or(false));

        // Show enriched market-level fields for the first market
        if let Some(m) = event.markets.first() {
            println!("      --- top market ---");
            println!("      question:     {}", m.question);
            println!(
                "      best_bid:     {}",
                m.best_bid.map_or("N/A".into(), |v| format!("{v:.4}"))
            );
            println!(
                "      best_ask:     {}",
                m.best_ask.map_or("N/A".into(), |v| format!("{v:.4}"))
            );
            println!(
                "      spread:       {}",
                m.spread.map_or("N/A".into(), |v| format!("{v:.4}"))
            );
            println!(
                "      competitive:  {}",
                m.competitive.map_or("N/A".into(), |v| format!("{v:.2}"))
            );
            println!(
                "      1d_change:    {}",
                m.one_day_price_change
                    .map_or("N/A".into(), |v| format!("{v:+.4}"))
            );
            println!(
                "      vol_1wk:      {}",
                m.volume_1wk.map_or("N/A".into(), |v| format!("{v:.0}"))
            );
        }

        println!();
    }

    // ---- Step 3: Load instruments via EventParamsFilter ----
    println!("=== Instruments via EventParamsFilter ===\n");

    let filter = EventParamsFilter::new(GetGammaEventsParams {
        active: Some(true),
        closed: Some(false),
        order: Some("volume".into()),
        ascending: Some(false),
        max_events: Some(5),
        ..Default::default()
    });

    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));
    provider.load_all(None).await?;

    let instruments = provider.store().list_all();
    println!(
        "Loaded {} instruments from top 5 events by volume:\n",
        instruments.len()
    );

    for (i, instrument) in instruments.into_iter().enumerate().take(30) {
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
