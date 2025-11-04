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

//! Example: Using Gate.io HTTP client for spot markets.

use nautilus_gateio2::http::GateioHttpClient;
use nautilus_model::instruments::Instrument;
use tracing::info;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Gate.io HTTP client example (spot markets)");

    // Create HTTP client (no credentials needed for public endpoints)
    let client = GateioHttpClient::new(
        None, // Use default HTTP URL
        None, // Use default spot WS URL
        None, // Use default futures WS URL
        None, // Use default options WS URL
        None, // No credentials for public endpoints
    );

    // Fetch spot currency pairs
    info!("\nFetching spot currency pairs...");
    match client.request_spot_currency_pairs().await {
        Ok(pairs) => {
            info!("Successfully fetched {} spot currency pairs", pairs.len());
            for (i, pair) in pairs.iter().take(5).enumerate() {
                info!(
                    "Pair {}: {} (base: {}, quote: {}, status: {})",
                    i + 1,
                    pair.id,
                    pair.base,
                    pair.quote,
                    pair.trade_status
                );
            }
        }
        Err(e) => {
            eprintln!("Error fetching currency pairs: {}", e);
        }
    }

    // Load instruments
    info!("\nLoading instruments...");
    match client.load_instruments().await {
        Ok(instruments) => {
            let spot_instruments: Vec<_> = instruments
                .iter()
                .filter(|i| matches!(i, nautilus_model::instruments::InstrumentAny::CurrencyPair(_)))
                .collect();

            info!("Successfully loaded {} spot instruments", spot_instruments.len());
            for (i, instrument) in spot_instruments.iter().take(5).enumerate() {
                info!("Instrument {}: {:?}", i + 1, instrument.id());
            }
        }
        Err(e) => {
            eprintln!("Error loading instruments: {}", e);
        }
    }

    // Fetch order book for BTC_USDT
    info!("\nFetching order book for BTC_USDT...");
    match client.request_spot_order_book("BTC_USDT").await {
        Ok(order_book) => {
            info!("Successfully fetched order book");
            info!("  Bids: {} levels", order_book.bids.len());
            info!("  Asks: {} levels", order_book.asks.len());
            if !order_book.bids.is_empty() {
                info!("  Best bid: {} @ {}", order_book.bids[0].quantity, order_book.bids[0].price);
            }
            if !order_book.asks.is_empty() {
                info!("  Best ask: {} @ {}", order_book.asks[0].quantity, order_book.asks[0].price);
            }
        }
        Err(e) => {
            eprintln!("Error fetching order book: {}", e);
        }
    }

    // Fetch recent trades for BTC_USDT
    info!("\nFetching recent trades for BTC_USDT...");
    match client.request_spot_trades("BTC_USDT").await {
        Ok(trades) => {
            info!("Successfully fetched {} trades", trades.len());
            for (i, trade) in trades.iter().take(3).enumerate() {
                info!(
                    "Trade {}: {} @ {} (side: {}, time: {})",
                    i + 1,
                    trade.amount,
                    trade.price,
                    trade.side,
                    trade.create_time
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
