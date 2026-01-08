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

//! Manual verification script for Ax HTTP public endpoints.
//!
//! Tests the instruments endpoint to verify connectivity and response parsing.
//! Defaults to sandbox environment.
//!
//! Usage:
//! ```bash
//! cargo run --bin ax-http-public -p nautilus-architect-ax
//! ```

use nautilus_architect_ax::{
    common::consts::{AX_HTTP_SANDBOX_URL, AX_HTTP_URL, AX_ORDERS_SANDBOX_URL, AX_ORDERS_URL},
    http::client::AxRawHttpClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let is_sandbox = std::env::var("AX_IS_SANDBOX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true);

    let (base_url, orders_base_url) = if is_sandbox {
        (AX_HTTP_SANDBOX_URL, AX_ORDERS_SANDBOX_URL)
    } else {
        (AX_HTTP_URL, AX_ORDERS_URL)
    };

    log::info!("Connecting to Ax HTTP API: {base_url}");
    log::info!(
        "Environment: {}",
        if is_sandbox { "SANDBOX" } else { "PRODUCTION" }
    );

    let client = AxRawHttpClient::new(
        Some(base_url.to_string()),
        Some(orders_base_url.to_string()),
        Some(30),
        None,
        None,
        None,
        None,
    )?;

    log::info!("Fetching all instruments...");
    let start = std::time::Instant::now();
    let instruments_response = client.get_instruments().await?;
    let elapsed = start.elapsed();

    log::info!(
        "Fetched {} instruments in {:.2}s",
        instruments_response.instruments.len(),
        elapsed.as_secs_f64()
    );

    for inst in instruments_response.instruments.iter().take(5) {
        log::info!(
            "  {} ({:?}) tick={} min_size={}",
            inst.symbol,
            inst.state,
            inst.tick_size,
            inst.minimum_order_size
        );
    }
    if instruments_response.instruments.len() > 5 {
        log::info!(
            "  ... and {} more",
            instruments_response.instruments.len() - 5
        );
    }

    let test_symbol = instruments_response
        .instruments
        .first()
        .map_or("EURUSD-PERP", |i| i.symbol.as_str());

    log::info!("Fetching single instrument: {test_symbol}");
    let start = std::time::Instant::now();
    let instrument = client.get_instrument(test_symbol).await?;
    let elapsed = start.elapsed();

    log::info!(
        "Fetched {} in {:.2}s",
        instrument.symbol,
        elapsed.as_secs_f64()
    );
    log::info!("  State: {:?}", instrument.state);
    log::info!("  Tick size: {}", instrument.tick_size);
    log::info!("  Min order size: {}", instrument.minimum_order_size);
    log::info!("  Quote currency: {}", instrument.quote_currency);
    log::info!("  Multiplier: {}", instrument.multiplier);

    log::info!("Done");

    Ok(())
}
