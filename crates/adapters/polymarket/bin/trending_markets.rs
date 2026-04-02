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

//! Demonstrates market discovery with [`GammaQueryFilter`].
//!
//! Unlike slug-based filters, [`GammaQueryFilter`] uses Gamma API query
//! parameters to discover markets by their characteristics — no prior
//! knowledge of slugs is required.
//!
//! Key filterable fields on [`GetGammaMarketsParams`]:
//!
//! - `active` / `closed` / `archived` — market lifecycle state
//! - `volume_num_min` / `volume_num_max` — traded volume range
//! - `liquidity_num_min` / `liquidity_num_max` — order-book liquidity
//! - `start_date_min` / `end_date_max` — date boundaries (ISO 8601)
//! - `tag_id` / `related_tags` — categorical tags
//! - `order` / `ascending` — sort field and direction
//! - `limit` / `offset` — pagination
//!
//! Use this filter when you want to scan for markets matching certain
//! criteria (e.g., trending high-volume markets) without knowing specific
//! slugs upfront.
//!
//! # Usage
//!
//! ```sh
//! cargo run -p nautilus-polymarket --bin trending_markets
//! ```

use std::sync::Arc;

use nautilus_common::providers::InstrumentProvider;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::retry::RetryConfig;
use nautilus_polymarket::{
    filters::GammaQueryFilter,
    http::{gamma::PolymarketGammaHttpClient, query::GetGammaMarketsParams},
    providers::PolymarketInstrumentProvider,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let params = GetGammaMarketsParams {
        active: Some(true),
        closed: Some(false),
        volume_num_min: Some(1_000_000.0),
        order: Some("liquidity".into()),
        ascending: Some(false),
        max_markets: Some(20),
        ..Default::default()
    };
    log::info!(
        "Query params: active=true, closed=false, volume_min=1mil, order=liquidity desc, max_markets=20"
    );

    log::info!("Creating HTTP client");
    let http_client = PolymarketGammaHttpClient::new(None, 60, RetryConfig::default())?;

    log::info!("Building filter and provider");
    let filter = GammaQueryFilter::new(params);
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));

    log::info!("Loading instruments from Gamma API...");
    provider.load_all(None).await?;

    let instruments = provider.store().list_all();
    log::info!(
        "Loaded {} trending instruments (by liquidity, descending)",
        instruments.len()
    );
    println!();

    for (i, instrument) in instruments.into_iter().enumerate() {
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
