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

//! Hyperliquid HTTP private API client binary.

use std::env;

use nautilus_hyperliquid::{
    common::credentials::HyperliquidCredentials,
    http::client::HyperliquidHttpClient,
};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    tracing::info!("🔐 Hyperliquid HTTP Private API Client");
    tracing::warn!("⚠️  This demo uses testnet only - never use with mainnet funds!");

    // Create credentials from environment variables
    let credentials = match HyperliquidCredentials::from_env(true) {
        Ok(creds) => {
            tracing::info!("✅ Loaded credentials from environment");
            creds
        }
        Err(_) => {
            tracing::info!("🔧 Using demo credentials (set HYPERLIQUID_TESTNET_PRIVATE_KEY for real usage)");
            let demo_private_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
            let demo_wallet = Some("0x1234567890123456789012345678901234567890".to_string());
            HyperliquidCredentials::new(demo_private_key.to_string(), demo_wallet, true)
        }
    };

    // Create HTTP client with credentials
    let client = HyperliquidHttpClient::new(
        Some("https://api.hyperliquid-testnet.xyz".to_string()),
        Some(credentials),
    )?;

    // Get user address for queries
    let user_address = env::var("HYPERLIQUID_TESTNET_WALLET_ADDRESS")
        .unwrap_or_else(|_| "0x1234567890123456789012345678901234567890".to_string());

    tracing::info!("👤 Using wallet address: {}", user_address);

    // Request user state (account information)
    tracing::info!("📊 Requesting user state...");
    match client.get_user_state(&user_address).await {
        Ok(user_state) => {
            tracing::info!("✅ Account state retrieved");
            tracing::info!("   💰 Account value: ${}", user_state.cross_margin_summary.account_value);
            tracing::info!("   💸 Total margin used: ${}", user_state.cross_margin_summary.total_margin_used);
            tracing::info!("   🏦 Withdrawable: ${}", user_state.withdrawable);
            tracing::info!("   📈 Active positions: {}", user_state.asset_positions.len());
            
            // Show position details
            for position in user_state.asset_positions.iter().take(5) {
                if position.position.szi != "0" {
                    tracing::debug!(
                        "   📍 {}: {} @ ${} (PnL: ${})",
                        position.position.coin,
                        position.position.szi,
                        position.position.entry_px.as_deref().unwrap_or("N/A"),
                        position.position.unrealized_pnl
                    );
                }
            }
        }
        Err(e) => tracing::error!("❌ Failed to get user state: {}", e),
    }

    // Request open orders
    tracing::info!("📋 Requesting open orders...");
    match client.get_open_orders(&user_address).await {
        Ok(orders) => {
            tracing::info!("✅ Found {} open orders", orders.len());
            for (i, order) in orders.iter().enumerate().take(10) {
                tracing::debug!(
                    "   Order {}: {} {} {} @ ${} (ID: {})",
                    i + 1,
                    order.coin,
                    order.side,
                    order.sz,
                    order.limit_px,
                    order.oid
                );
            }
        }
        Err(e) => tracing::error!("❌ Failed to get open orders: {}", e),
    }

    // Request recent fills (trade history)
    tracing::info!("🔄 Requesting recent fills...");
    match client.get_user_fills(&user_address).await {
        Ok(fills) => {
            tracing::info!("✅ Found {} recent fills", fills.len());
            for (i, fill) in fills.iter().enumerate().take(10) {
                tracing::debug!(
                    "   Fill {}: {} {} {} @ ${} (PnL: ${}, Fee: ${})",
                    i + 1,
                    fill.coin,
                    fill.dir,
                    fill.sz,
                    fill.px,
                    fill.closed_pnl,
                    fill.fee
                );
            }
        }
        Err(e) => tracing::error!("❌ Failed to get user fills: {}", e),
    }

    // Request portfolio history
    tracing::info!("📈 Requesting portfolio history...");
    match client.get_portfolio(&user_address).await {
        Ok(portfolio) => {
            tracing::info!("✅ Portfolio history retrieved");
            tracing::info!("   📊 Account value points: {}", portfolio.account_value.len());
            tracing::info!("   💹 PnL history points: {}", portfolio.pnl_history.len());
            tracing::info!("   📊 Volume history points: {}", portfolio.volume_history.len());
        }
        Err(e) => tracing::error!("❌ Failed to get portfolio: {}", e),
    }

    tracing::info!("🎉 Private API demonstration completed!");
    tracing::info!("💡 Set environment variables HYPERLIQUID_TESTNET_PRIVATE_KEY and HYPERLIQUID_TESTNET_WALLET_ADDRESS for real usage");
    
    Ok(())
}
