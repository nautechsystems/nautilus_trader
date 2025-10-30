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

// Simple script to capture real API responses for test fixtures

use std::fs;

use nautilus_hyperliquid::http::client::HyperliquidHttpClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Capturing Hyperliquid test data...");

    let client = HyperliquidHttpClient::new(false, Some(60), None)?;

    // Capture perpetuals metadata (first 3 markets to keep file small)
    println!("Fetching perpetuals metadata...");
    let meta = client.info_meta().await?;
    let sample_meta = serde_json::json!({
        "universe": meta.universe.iter().take(3).collect::<Vec<_>>()
    });
    fs::write(
        "test_data/http_meta_perp_sample.json",
        serde_json::to_string_pretty(&sample_meta)?,
    )?;
    println!("Saved http_meta_perp_sample.json (3 markets)");

    // Note: Spot metadata endpoint not yet implemented in client

    // Capture BTC order book
    println!("Fetching BTC order book...");
    let book = client.info_l2_book("BTC").await?;
    // Keep only top 5 levels each side
    let sample_book = serde_json::json!({
        "coin": book.coin,
        "levels": vec![
            book.levels.first().unwrap().iter().take(5).collect::<Vec<_>>(),
            book.levels[1].iter().take(5).collect::<Vec<_>>()
        ],
        "time": book.time
    });
    fs::write(
        "test_data/http_l2_book_btc.json",
        serde_json::to_string_pretty(&sample_book)?,
    )?;
    println!("Saved http_l2_book_btc.json (5 levels each side)");

    println!("\nTest data capture complete!");
    println!("Files saved in test_data/");

    Ok(())
}
