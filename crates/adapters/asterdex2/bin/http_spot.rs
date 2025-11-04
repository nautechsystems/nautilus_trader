// Example: Asterdex HTTP Spot Market Data
//
// Demonstrates how to use the Asterdex HTTP client to fetch spot market data.

use nautilus_asterdex2::http::AsterdexHttpClient;
use nautilus_model::instruments::Instrument;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Asterdex HTTP Spot Market Example ===\n");

    // Create HTTP client (no credentials needed for public endpoints)
    let client = AsterdexHttpClient::new(None, None, None, None)?;

    // 1. Fetch spot exchange info
    println!("Fetching spot exchange info...");
    match client.request_spot_exchange_info().await {
        Ok(info) => {
            if let Some(symbols) = info.get("symbols").and_then(|s| s.as_array()) {
                println!("✓ Found {} spot trading pairs", symbols.len());
                // Print first 3 symbols as example
                for (i, symbol) in symbols.iter().take(3).enumerate() {
                    if let Some(name) = symbol.get("symbol").and_then(|s| s.as_str()) {
                        println!("  {}. {}", i + 1, name);
                    }
                }
                if symbols.len() > 3 {
                    println!("  ... and {} more", symbols.len() - 3);
                }
            }
        }
        Err(e) => eprintln!("✗ Error fetching exchange info: {}", e),
    }

    // 2. Fetch order book for a specific pair
    println!("\nFetching BTCUSDT order book...");
    match client.request_spot_order_book("BTCUSDT", Some(5)).await {
        Ok(book) => {
            println!("✓ Order book received");
            if let Some(bids) = book.get("bids").and_then(|b| b.as_array()) {
                println!("  Top {} bids:", bids.len());
                for bid in bids.iter().take(3) {
                    println!("    {:?}", bid);
                }
            }
            if let Some(asks) = book.get("asks").and_then(|a| a.as_array()) {
                println!("  Top {} asks:", asks.len());
                for ask in asks.iter().take(3) {
                    println!("    {:?}", ask);
                }
            }
        }
        Err(e) => eprintln!("✗ Error fetching order book: {}", e),
    }

    // 3. Fetch recent trades
    println!("\nFetching recent BTCUSDT trades...");
    match client.request_spot_trades("BTCUSDT").await {
        Ok(trades) => {
            if let Some(trade_list) = trades.as_array() {
                println!("✓ Received {} recent trades", trade_list.len());
                for (i, trade) in trade_list.iter().take(3).enumerate() {
                    println!("  Trade {}: {:?}", i + 1, trade);
                }
            }
        }
        Err(e) => eprintln!("✗ Error fetching trades: {}", e),
    }

    // 4. Load instruments
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
