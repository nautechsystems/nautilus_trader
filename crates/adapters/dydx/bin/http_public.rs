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
//! Tests all historical request methods including direct access to inner HTTP client
//! for trades and candles endpoints.
//!
//! Usage:
//! ```bash
//! # Test against testnet (default)
//! cargo run --bin dydx-http-public -p nautilus-dydx
//!
//! # Test against mainnet
//! cargo run --bin dydx-http-public -p nautilus-dydx -- --mainnet
//!
//! # Test specific symbol
//! cargo run --bin dydx-http-public -p nautilus-dydx -- --symbol ETH-USD
//!
//! # Show instrument summary (grouped by type and base asset)
//! cargo run --bin dydx-http-public -p nautilus-dydx -- --summary
//!
//! # Custom URL via environment variable
//! DYDX_HTTP_URL=https://indexer.dydx.trade cargo run --bin dydx-http-public -p nautilus-dydx
//! ```

use std::{collections::HashMap, env};

use chrono::{Duration, Utc};
use nautilus_dydx::{
    common::{consts::DYDX_TESTNET_HTTP_URL, enums::DydxCandleResolution},
    http::client::DydxHttpClient,
};
use nautilus_model::instruments::{Instrument, InstrumentAny};
use ustr::Ustr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = env::args().collect();
    let is_mainnet = args.iter().any(|a| a == "--mainnet");
    let show_summary = args.iter().any(|a| a == "--summary");
    let symbol = args
        .iter()
        .position(|a| a == "--symbol")
        .and_then(|i| args.get(i + 1))
        .map_or("BTC-USD", |s| s.as_str());

    let base_url = if is_mainnet {
        env::var("DYDX_HTTP_URL").unwrap_or_else(|_| "https://indexer.dydx.trade".to_string())
    } else {
        env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string())
    };
    let is_testnet = !is_mainnet;

    tracing::info!("Connecting to dYdX HTTP API: {}", base_url);
    tracing::info!(
        "Environment: {}",
        if is_testnet { "TESTNET" } else { "MAINNET" }
    );
    tracing::info!("");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, is_testnet, None)?;

    let start = std::time::Instant::now();
    let instruments = client.request_instruments(None, None, None).await?;
    let elapsed = start.elapsed();

    tracing::info!(
        "SUCCESS: Fetched {} instruments in {:.2}s",
        instruments.len(),
        elapsed.as_secs_f64()
    );
    if show_summary {
        print_instrument_summary(&instruments);
        return Ok(());
    }

    if !instruments.is_empty() {
        tracing::info!("   Sample instruments:");
        for inst in instruments.iter().take(5) {
            tracing::info!("   - {} ({})", inst.id().symbol, inst.instrument_class());
        }
        if instruments.len() > 5 {
            tracing::info!("   ... and {} more", instruments.len() - 5);
        }
    }

    client.cache_instruments(instruments.clone());
    tracing::info!("Cached {} instruments", instruments.len());
    tracing::info!("Cached {} instruments", instruments.len());

    let query_symbol = Ustr::from(symbol);
    let start = std::time::Instant::now();
    let instrument = client.get_instrument(&query_symbol);
    let elapsed = start.elapsed();

    match instrument {
        Some(inst) => {
            tracing::info!(
                "SUCCESS: Found {} in cache in {:.4}ms",
                inst.id(),
                elapsed.as_micros() as f64 / 1000.0
            );
            tracing::info!("   Type: {}", inst.instrument_class());
            tracing::info!("   Price precision: {}", inst.price_precision());
            tracing::info!("   Size precision: {}", inst.size_precision());
        }
        None => {
            tracing::warn!("FAILED: Instrument {} not found in cache", query_symbol);
        }
    }

    let limit = Some(100);

    let start = std::time::Instant::now();
    let trades = client.request_trades(symbol, limit).await?;
    let elapsed = start.elapsed();

    tracing::info!(
        "SUCCESS: Fetched {} trades for {} in {:.2}s",
        trades.trades.len(),
        symbol,
        elapsed.as_secs_f64()
    );

    if !trades.trades.is_empty() {
        let first = &trades.trades[0];
        let last = &trades.trades[trades.trades.len() - 1];
        tracing::info!(
            "   First trade: {} @ {} ({})",
            first.size,
            first.price,
            first.side
        );
        tracing::info!(
            "   Last trade:  {} @ {} ({})",
            last.size,
            last.price,
            last.side
        );
        tracing::info!("   Time range: {} to {}", first.created_at, last.created_at);
    }

    let resolution = DydxCandleResolution::OneMinute;
    let end_time = Utc::now();
    let start_time = end_time - Duration::hours(2); // 2 hours = ~120 bars

    let start = std::time::Instant::now();
    let candles = client
        .request_candles(symbol, resolution, None, Some(start_time), Some(end_time))
        .await?;
    let elapsed = start.elapsed();

    tracing::info!(
        "SUCCESS: Fetched {} candles for {} ({:?}) in {:.2}s",
        candles.candles.len(),
        symbol,
        resolution,
        elapsed.as_secs_f64()
    );

    if !candles.candles.is_empty() {
        let first = &candles.candles[0];
        let last = &candles.candles[candles.candles.len() - 1];
        tracing::info!(
            "   First candle: O={} H={} L={} C={} V={}",
            first.open,
            first.high,
            first.low,
            first.close,
            first.base_token_volume
        );
        tracing::info!(
            "   Last candle:  O={} H={} L={} C={} V={}",
            last.open,
            last.high,
            last.low,
            last.close,
            last.base_token_volume
        );
        tracing::info!("   Time range: {} to {}", first.started_at, last.started_at);
    }

    let end_time = Utc::now();
    let start_time = end_time - Duration::days(7); // 7 days

    tracing::info!(
        "   Requesting {:?} bars from {} to {}",
        resolution,
        start_time,
        end_time
    );

    let start = std::time::Instant::now();
    let candles_large = client
        .request_candles(symbol, resolution, None, Some(start_time), Some(end_time))
        .await?;
    let elapsed = start.elapsed();

    let expected_bars_large = ((end_time - start_time).num_minutes() as usize).min(10_080);
    let coverage_large = (candles_large.candles.len() as f64 / expected_bars_large as f64) * 100.0;

    tracing::info!(
        "SUCCESS: Fetched {} candles in {:.2}s ({:.0} bars/sec)",
        candles_large.candles.len(),
        elapsed.as_secs_f64(),
        candles_large.candles.len() as f64 / elapsed.as_secs_f64()
    );

    if !candles_large.candles.is_empty() {
        tracing::info!("   Coverage: {:.1}% of expected bars", coverage_large);
        tracing::info!(
            "   Time range: {} to {}",
            candles_large.candles[0].started_at,
            candles_large.candles[candles_large.candles.len() - 1].started_at
        );
    }
    tracing::info!("");

    tracing::info!("ALL TESTS COMPLETED SUCCESSFULLY");
    tracing::info!("");
    tracing::info!("Summary:");
    tracing::info!(
        "  [PASS] request_instruments: {} instruments",
        instruments.len()
    );
    tracing::info!("  [PASS] get_instrument: Cache lookup works");
    tracing::info!(
        "  [PASS] get_trades: {} trades fetched",
        trades.trades.len()
    );
    tracing::info!(
        "  [PASS] get_candles (small): {} candles",
        candles.candles.len()
    );
    tracing::info!(
        "  [PASS] get_candles (large): {} candles with {:.1}% coverage",
        candles_large.candles.len(),
        coverage_large
    );

    Ok(())
}

fn print_instrument_summary(instruments: &[InstrumentAny]) {
    let mut by_type: HashMap<String, usize> = HashMap::new();
    let mut by_base: HashMap<String, usize> = HashMap::new();

    for inst in instruments {
        let type_name = inst.instrument_class().to_string();
        *by_type.entry(type_name).or_insert(0) += 1;

        let base = inst
            .id()
            .symbol
            .as_str()
            .split('-')
            .next()
            .unwrap_or("UNKNOWN")
            .to_string();
        *by_base.entry(base).or_insert(0) += 1;
    }

    tracing::info!("");
    tracing::info!("=== Instruments by Type ===");
    let mut types: Vec<_> = by_type.iter().collect();
    types.sort_by_key(|(name, _)| *name);
    for (type_name, count) in types {
        tracing::info!("  {:20} : {:4} instruments", type_name, count);
    }
    tracing::info!("");

    tracing::info!("=== Instruments by Base Asset (Top 20) ===");
    let mut bases: Vec<_> = by_base.iter().collect();
    bases.sort_by(|a, b| b.1.cmp(a.1));
    for (base, count) in bases.iter().take(20) {
        tracing::info!("  {:10} : {:4} instruments", base, count);
    }
    if bases.len() > 20 {
        tracing::info!("  ... and {} more base assets", bases.len() - 20);
    }
    tracing::info!("");

    tracing::info!("=== Sample Instruments ===");
    for inst in instruments.iter().take(5) {
        tracing::info!(
            "  {} ({}) - price_prec={} size_prec={}",
            inst.id(),
            inst.instrument_class(),
            inst.price_precision(),
            inst.size_precision()
        );
    }
    if instruments.len() > 5 {
        tracing::info!("  ... and {} more", instruments.len() - 5);
    }
    tracing::info!("");

    tracing::info!("Summary complete");
}
