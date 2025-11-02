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

//! Hyperliquid HTTP public API client binary.

use nautilus_hyperliquid::http::client::HyperliquidHttpClient;
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    tracing::info!("🚀 Hyperliquid HTTP Public API Client");

    // Create HTTP client (no credentials needed for public endpoints)
    let client = HyperliquidHttpClient::new(None, None)?;

    // Request market universe (available assets)
    tracing::info!("📊 Requesting market universe...");
    match client.get_universe().await {
        Ok(universe) => {
            tracing::info!("✅ Found {} assets in universe", universe.universe.len());
            for (i, asset) in universe.universe.iter().enumerate().take(10) {
                tracing::debug!("   Asset {}: {} (sz_decimals: {})", i + 1, asset.name, asset.sz_decimals);
            }
        }
        Err(e) => tracing::error!("❌ Failed to get universe: {}", e),
    }

    // Request all mid prices
    tracing::info!("💰 Requesting all mid prices...");
    match client.get_all_mids().await {
        Ok(mids) => {
            tracing::info!("✅ Got mid prices for {} assets", mids.mids.len());
            let btc_price = mids.mids.get("BTC");
            let eth_price = mids.mids.get("ETH");
            let sol_price = mids.mids.get("SOL");
            
            if let Some(price) = btc_price {
                tracing::info!("   📈 BTC: ${}", price);
            }
            if let Some(price) = eth_price {
                tracing::info!("   📈 ETH: ${}", price);
            }
            if let Some(price) = sol_price {
                tracing::info!("   📈 SOL: ${}", price);
            }
        }
        Err(e) => tracing::error!("❌ Failed to get mids: {}", e),
    }

    // Request L2 order book data for BTC
    tracing::info!("📚 Requesting L2 order book for BTC...");
    match client.get_l2_book("BTC").await {
        Ok(book) => {
            tracing::info!("✅ Got L2 book for BTC");
            if let Some(levels) = book.get("levels").and_then(|l| l.as_array()) {
                if levels.len() >= 2 {
                    if let Some(bids) = levels.get(0).and_then(|b| b.as_array()) {
                        tracing::debug!("   📖 Bids: {} levels", bids.len());
                    }
                    if let Some(asks) = levels.get(1).and_then(|a| a.as_array()) {
                        tracing::debug!("   📖 Asks: {} levels", asks.len());
                    }
                }
            }
        }
        Err(e) => tracing::error!("❌ Failed to get L2 book: {}", e),
    }

    // Request recent trades for BTC
    tracing::info!("🔄 Requesting recent trades for BTC...");
    match client.get_recent_trades("BTC").await {
        Ok(trades) => {
            if let Some(trades_array) = trades.as_array() {
                tracing::info!("✅ Got {} recent trades for BTC", trades_array.len());
                for (i, trade) in trades_array.iter().enumerate().take(5) {
                    if let (Some(side), Some(sz), Some(px), Some(time)) = (
                        trade.get("side").and_then(|s| s.as_str()),
                        trade.get("sz").and_then(|s| s.as_str()),
                        trade.get("px").and_then(|p| p.as_str()),
                        trade.get("time").and_then(|t| t.as_u64())
                    ) {
                        tracing::debug!(
                            "   Trade {}: {} {} @ ${} (time: {})",
                            i + 1,
                            side,
                            sz,
                            px,
                            time
                        );
                    }
                }
            } else {
                tracing::info!("✅ Got recent trades for BTC");
            }
        }
        Err(e) => tracing::error!("❌ Failed to get recent trades: {}", e),
    }

    tracing::info!("🎉 Public API demonstration completed!");
    Ok(())
}
