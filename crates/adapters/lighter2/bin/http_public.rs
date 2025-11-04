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

//! Example: Using Lighter HTTP client for public endpoints.

use nautilus_lighter2::http::LighterHttpClient;
use nautilus_model::instruments::Instrument;
use tracing::info;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Lighter HTTP client example (public endpoints)");

    // Create HTTP client (no credentials needed for public endpoints)
    let client = LighterHttpClient::new(
        None,  // Use default mainnet HTTP URL
        None,  // Use default mainnet WS URL
        false, // Not testnet
        None,  // No credentials for public endpoints
    );

    // Fetch markets
    info!("Fetching markets...");
    match client.request_markets().await {
        Ok(markets) => {
            info!("Successfully fetched {} markets", markets.len());
            for (i, market) in markets.iter().take(5).enumerate() {
                info!(
                    "Market {}: {} (ID: {}, Type: {:?})",
                    i + 1,
                    market.symbol,
                    market.id,
                    market.instrument_type
                );
            }
        }
        Err(e) => {
            eprintln!("Error fetching markets: {}", e);
        }
    }

    // Load instruments
    info!("\nLoading instruments...");
    match client.load_instruments().await {
        Ok(instruments) => {
            info!("Successfully loaded {} instruments", instruments.len());
            for (i, instrument) in instruments.iter().take(5).enumerate() {
                info!("Instrument {}: {:?}", i + 1, instrument.id());
            }
        }
        Err(e) => {
            eprintln!("Error loading instruments: {}", e);
        }
    }

    // Fetch order book for market 0 (if it exists)
    info!("\nFetching order book for market 0...");
    match client.request_order_book(0).await {
        Ok(order_book) => {
            info!("Successfully fetched order book: {}", order_book);
        }
        Err(e) => {
            eprintln!("Error fetching order book: {}", e);
        }
    }

    // Fetch recent trades for market 0
    info!("\nFetching recent trades for market 0...");
    match client.request_trades(0).await {
        Ok(trades) => {
            info!("Successfully fetched {} trades", trades.len());
            for (i, trade) in trades.iter().take(3).enumerate() {
                info!(
                    "Trade {}: {} @ {} (ID: {})",
                    i + 1,
                    trade.quantity,
                    trade.price,
                    trade.id
                );
            }
        }
        Err(e) => {
            eprintln!("Error fetching trades: {}", e);
        }
    }

    info!("\nExample completed!");
    Ok(())
}
