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

//! Example: Hyperliquid HTTP Client
//!
//! Demonstrates how to use the Hyperliquid HTTP client to fetch market data.

use nautilus_hyperliquid2::http::Hyperliquid2HttpClient;
use nautilus_model::instruments::Instrument;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Hyperliquid HTTP Client Example ===\n");

    // Create HTTP client (no credentials needed for public endpoints)
    let client = Hyperliquid2HttpClient::new(None, None, false)?;

    // 1. Fetch meta info (universe of assets)
    println!("Fetching meta info...");
    match client.request_meta_info().await {
        Ok(meta_info) => {
            println!("✓ Found {} assets", meta_info.universe.len());
            // Print first 3 assets as example
            for (i, asset) in meta_info.universe.iter().take(3).enumerate() {
                println!("  {}. {} (sz_decimals: {})", i + 1, asset.name, asset.sz_decimals);
            }
            if meta_info.universe.len() > 3 {
                println!("  ... and {} more", meta_info.universe.len() - 3);
            }
        }
        Err(e) => eprintln!("✗ Error fetching meta info: {}", e),
    }

    // 2. Fetch all mids
    println!("\nFetching all mids...");
    match client.request_all_mids().await {
        Ok(mids) => {
            println!("✓ Received {} mid prices", mids.len());
            // Print first 3 mids as example
            for (i, (coin, price)) in mids.iter().take(3).enumerate() {
                println!("  {}. {}: ${}", i + 1, coin, price);
            }
            if mids.len() > 3 {
                println!("  ... and {} more", mids.len() - 3);
            }
        }
        Err(e) => eprintln!("✗ Error fetching mids: {}", e),
    }

    // 3. Fetch L2 book for BTC
    println!("\nFetching BTC L2 order book...");
    match client.request_l2_book("BTC").await {
        Ok(book) => {
            println!("✓ Order book received for {}", book.coin);
            println!("  Timestamp: {}", book.time);
            if !book.levels.is_empty() {
                println!("  Number of price levels: {}", book.levels.len());
            }
        }
        Err(e) => eprintln!("✗ Error fetching order book: {}", e),
    }

    // 4. Fetch recent trades for BTC
    println!("\nFetching recent BTC trades...");
    match client.request_trades("BTC").await {
        Ok(trades) => {
            println!("✓ Received {} recent trades", trades.len());
            for (i, trade) in trades.iter().take(3).enumerate() {
                println!(
                    "  Trade {}: {} {} @ ${}",
                    i + 1,
                    if trade.side == "A" { "BUY" } else { "SELL" },
                    trade.sz,
                    trade.px
                );
            }
        }
        Err(e) => eprintln!("✗ Error fetching trades: {}", e),
    }

    // 5. Load instruments
    println!("\nLoading all instruments...");
    match client.load_instruments().await {
        Ok(instruments) => {
            println!("✓ Loaded {} instruments total", instruments.len());
            // Print first few instruments
            for (i, instrument) in instruments.iter().take(5).enumerate() {
                println!("  {}. {:?}", i + 1, instrument.id());
            }
            if instruments.len() > 5 {
                println!("  ... and {} more", instruments.len() - 5);
            }
        }
        Err(e) => eprintln!("✗ Error loading instruments: {}", e),
    }

    println!("\n=== Example completed ===");
    Ok(())
}
