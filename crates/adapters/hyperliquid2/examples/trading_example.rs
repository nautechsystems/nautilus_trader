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

//! Example demonstrating the Hyperliquid HTTP trading client.

use std::env;

use nautilus_hyperliquid::{
    common::credentials::HyperliquidCredentials,
    http::client::HyperliquidHttpClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🏛️ Hyperliquid Trading Client Example");
    println!("⚠️  WARNING: This is for testnet only - do not use with real funds!");
    
    // Load credentials from environment
    let private_key = env::var("HYPERLIQUID_PRIVATE_KEY")
        .expect("HYPERLIQUID_PRIVATE_KEY environment variable must be set");
    let wallet_address = env::var("HYPERLIQUID_WALLET_ADDRESS").ok();
    
    let credentials = HyperliquidCredentials::new(private_key, wallet_address, true); // testnet = true
    
    // Create HTTP client
    let client = HyperliquidHttpClient::new(
        Some("https://api.hyperliquid-testnet.xyz".to_string()),
        Some(credentials),
    )?;
    
    println!("✅ Created authenticated HTTP client for testnet");
    
    // Example wallet address for queries
    let user_address = env::var("HYPERLIQUID_WALLET_ADDRESS")
        .unwrap_or_else(|_| "0x1234567890123456789012345678901234567890".to_string());
    
    // 1. Get market data first
    println!("\n📊 Getting market data...");
    
    match client.get_universe().await {
        Ok(universe) => {
            println!("✅ Found {} assets in universe", universe.universe.len());
            for asset in universe.universe.iter().take(3) {
                println!("   📈 {}: {} decimals", asset.name, asset.sz_decimals);
            }
        }
        Err(e) => println!("❌ Failed to get universe: {}", e),
    }
    
    match client.get_all_mids().await {
        Ok(mids) => {
            println!("✅ Got mid prices for {} assets", mids.mids.len());
            for (symbol, price) in mids.mids.iter().take(3) {
                println!("   💰 {}: ${}", symbol, price);
            }
        }
        Err(e) => println!("❌ Failed to get mids: {}", e),
    }
    
    // 2. Get account information
    println!("\n👤 Getting account information...");
    
    match client.get_user_state(&user_address).await {
        Ok(user_state) => {
            println!("✅ Account value: ${}", user_state.cross_margin_summary.account_value);
            println!("   💸 Total margin used: ${}", user_state.cross_margin_summary.total_margin_used);
            println!("   🏦 Withdrawable: ${}", user_state.withdrawable);
            println!("   📊 Active positions: {}", user_state.asset_positions.len());
            
            for position in user_state.asset_positions.iter().take(3) {
                if position.position.szi != "0" {
                    println!("   📍 {}: {} @ ${}", 
                        position.position.coin,
                        position.position.szi,
                        position.position.entry_px.as_deref().unwrap_or("N/A")
                    );
                }
            }
        }
        Err(e) => println!("❌ Failed to get user state: {}", e),
    }
    
    // 3. Get open orders
    match client.get_open_orders(&user_address).await {
        Ok(orders) => {
            println!("✅ Found {} open orders", orders.len());
            for order in orders.iter().take(3) {
                println!("   📋 Order {}: {} {} @ ${}", 
                    order.oid,
                    order.side,
                    order.sz,
                    order.limit_px
                );
            }
        }
        Err(e) => println!("❌ Failed to get open orders: {}", e),
    }
    
    // 4. Get recent fills
    match client.get_user_fills(&user_address).await {
        Ok(fills) => {
            println!("✅ Found {} recent fills", fills.len());
            for fill in fills.iter().take(3) {
                println!("   🔄 {}: {} {} @ ${} (PnL: ${})", 
                    fill.coin,
                    fill.dir,
                    fill.sz,
                    fill.px,
                    fill.closed_pnl
                );
            }
        }
        Err(e) => println!("❌ Failed to get user fills: {}", e),
    }
    
    // 5. Get portfolio history
    match client.get_portfolio(&user_address).await {
        Ok(portfolio) => {
            println!("✅ Portfolio history:");
            println!("   📈 Account value points: {}", portfolio.account_value.len());
            println!("   💹 PnL history points: {}", portfolio.pnl_history.len());
            println!("   📊 Volume history points: {}", portfolio.volume_history.len());
        }
        Err(e) => println!("❌ Failed to get portfolio: {}", e),
    }
    
    // 6. DEMONSTRATION ONLY: Example order operations (commented out for safety)
    println!("\n⚠️  Trading operations are COMMENTED OUT for safety");
    println!("   Uncomment the following sections to test actual trading:");
    
    /*
    // Example: Place a small test order
    let order_request = HyperliquidOrderRequest {
        asset: "BTC".to_string(),
        is_buy: true,
        limit_px: "30000.0".to_string(), // Well below market to avoid execution
        sz: "0.001".to_string(), // Very small size
        reduce_only: false,
        order_type: HyperliquidOrderType::Limit,
        time_in_force: Some(HyperliquidTimeInForce::Gtc),
        client_id: Some("nautilus-test-001".to_string()),
        post_only: Some(true),
    };
    
    match client.place_order(&order_request).await {
        Ok(response) => {
            println!("✅ Order placed: {:?}", response);
            
            // Example: Cancel the order immediately
            if let Some(order_id) = response.get("response").and_then(|r| r.get("data")).and_then(|d| d.get("statuses")).and_then(|s| s.get(0)).and_then(|o| o.get("resting").and_then(|r| r.get("oid").and_then(|id| id.as_u64()))) {
                let cancel_request = HyperliquidCancelOrderRequest {
                    asset: "BTC".to_string(),
                    oid: order_id,
                };
                
                match client.cancel_order(&cancel_request).await {
                    Ok(cancel_response) => println!("✅ Order cancelled: {:?}", cancel_response),
                    Err(e) => println!("❌ Failed to cancel order: {}", e),
                }
            }
        }
        Err(e) => println!("❌ Failed to place order: {}", e),
    }
    
    // Example: Update leverage
    let leverage_request = HyperliquidUpdateLeverageRequest {
        asset: "BTC".to_string(),
        is_cross: true,
        leverage: 2,
    };
    
    match client.update_leverage(&leverage_request).await {
        Ok(response) => println!("✅ Leverage updated: {:?}", response),
        Err(e) => println!("❌ Failed to update leverage: {}", e),
    }
    */
    
    println!("\n🎉 Example completed successfully!");
    println!("💡 To enable trading operations:");
    println!("   1. Set environment variables: HYPERLIQUID_PRIVATE_KEY, HYPERLIQUID_WALLET_ADDRESS");
    println!("   2. Ensure you're using TESTNET only");
    println!("   3. Uncomment the trading sections in the code");
    println!("   4. Test with very small amounts first");
    
    Ok(())
}