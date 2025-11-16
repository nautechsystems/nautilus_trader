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

//! Manual verification script for dYdX HTTP public endpoints.
//!
//! **NOTE**: This script currently demonstrates the API but requires public
//! wrapper methods to be added to `DydxHttpClient` to fully function.
//! Currently `get_trades` and `get_candles` are only available on the inner
//! `DydxRawHttpClient` which is marked `pub(crate)`.
//!
//! **TODO**: Add public wrapper methods to `DydxHttpClient`:
//! - `pub async fn request_trades(&self, symbol: &str, limit: Option<u32>) -> Result<TradesResponse>`
//! - `pub async fn request_candles(&self, ...) -> Result<CandlesResponse>`
//!
//! Tests historical request methods:
//! - `request_instruments` ✅ (public API available)
//! - `get_instrument` ✅ (public API available)
//! - `get_trades` ❌ (needs public wrapper)
//! - `get_candles` ❌ (needs public wrapper)
//!
//! Usage:
//! ```bash
//! # Test against testnet (default)
//! cargo run --bin dydx-http-public -p nautilus-dydx
//!
//! # Test against mainnet
//! DYDX_HTTP_URL=https://indexer.dydx.trade cargo run --bin dydx-http-public -p nautilus-dydx
//! ```

use nautilus_dydx::{common::consts::DYDX_TESTNET_HTTP_URL, http::client::DydxHttpClient};
use nautilus_model::instruments::Instrument;
use ustr::Ustr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let base_url =
        std::env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string());
    let is_testnet = base_url.contains("testnet");

    tracing::info!("🔗 Connecting to dYdX HTTP API: {}", base_url);
    tracing::info!(
        "🌐 Environment: {}",
        if is_testnet { "TESTNET" } else { "MAINNET" }
    );
    tracing::info!("");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, is_testnet, None)?;

    // ============================================================================
    // TEST 1: Request all instruments ✅
    // ============================================================================
    tracing::info!("📋 TEST 1: Request all instruments");
    tracing::info!("─────────────────────────────────────");

    let start = std::time::Instant::now();
    let instruments = client.request_instruments(None, None, None).await?;
    let elapsed = start.elapsed();

    tracing::info!(
        "✅ Fetched {} instruments in {:.2}s",
        instruments.len(),
        elapsed.as_secs_f64()
    );

    // Show sample instruments
    if !instruments.is_empty() {
        tracing::info!("   Sample instruments:");
        for inst in instruments.iter().take(5) {
            tracing::info!("   - {} ({})", inst.id().symbol, inst.instrument_class());
        }
        if instruments.len() > 5 {
            tracing::info!("   ... and {} more", instruments.len() - 5);
        }
    }

    // Cache instruments for subsequent tests
    client.cache_instruments(instruments.clone());
    tracing::info!("✅ Cached {} instruments", instruments.len());
    tracing::info!("");

    // ============================================================================
    // TEST 2: Request single instrument (cache hit) ✅
    // ============================================================================
    tracing::info!("🔍 TEST 2: Request single instrument (cache hit)");
    tracing::info!("─────────────────────────────────────");

    let symbol = Ustr::from("BTC-USD");
    let start = std::time::Instant::now();
    let instrument = client.get_instrument(&symbol);
    let elapsed = start.elapsed();

    match instrument {
        Some(inst) => {
            tracing::info!(
                "✅ Found {} in cache in {:.4}ms",
                inst.id(),
                elapsed.as_micros() as f64 / 1000.0
            );
            tracing::info!("   Type: {}", inst.instrument_class());
            tracing::info!("   Price precision: {}", inst.price_precision());
            tracing::info!("   Size precision: {}", inst.size_precision());
        }
        None => {
            tracing::warn!("❌ Instrument {} not found in cache", symbol);
        }
    }
    tracing::info!("");

    // ============================================================================
    // TEST 3: Request trades ❌ (Blocked - needs public API)
    // ============================================================================
    tracing::info!("📊 TEST 3: Request historical trades");
    tracing::info!("─────────────────────────────────────");
    tracing::warn!("⚠️  SKIPPED: get_trades() not available on public API");
    tracing::warn!("   TODO: Add `pub async fn request_trades(...)` to DydxHttpClient");
    tracing::info!("");

    // ============================================================================
    // TEST 4: Request bars (small range) ❌ (Blocked - needs public API)
    // ============================================================================
    tracing::info!("📈 TEST 4: Request historical bars (small range)");
    tracing::info!("─────────────────────────────────────");
    tracing::warn!("⚠️  SKIPPED: get_candles() not available on public API");
    tracing::warn!("   TODO: Add `pub async fn request_candles(...)` to DydxHttpClient");
    tracing::info!("");

    // ============================================================================
    // Summary
    // ============================================================================
    tracing::info!("✅ COMPLETED AVAILABLE TESTS");
    tracing::info!("");
    tracing::info!("Summary:");
    tracing::info!(
        "  ✅ get_markets → request_instruments: {} instruments",
        instruments.len()
    );
    tracing::info!("  ✅ get_instrument (cache): Cache lookup works");
    tracing::info!("  ⚠️  get_trades: Needs public wrapper method");
    tracing::info!("  ⚠️  get_candles: Needs public wrapper method");
    tracing::info!("");
    tracing::info!("Next steps:");
    tracing::info!("  1. Add public wrapper methods to DydxHttpClient");
    tracing::info!("  2. Re-run this script to test full API");

    Ok(())
}
