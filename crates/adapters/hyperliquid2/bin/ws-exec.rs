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

//! Hyperliquid WebSocket execution client binary.
//! Demonstrates authenticated WebSocket connections for order updates and account events.

use std::sync::Arc;
use std::time::Duration;

use nautilus_hyperliquid::{
    common::credentials::HyperliquidCredentials,
    websocket::client::{HyperliquidWebSocketClient, MessageHandler},
};
use tokio::{signal, time::sleep};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    // Initialize TLS crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    tracing::info!("⚡ Hyperliquid WebSocket Execution Client");
    tracing::warn!("⚠️  This demo uses testnet only - never use with mainnet funds!");
    tracing::info!("Press Ctrl+C to exit");

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

    // Create message handler that logs execution events
    let handler: MessageHandler = Arc::new(|message| {
        match message.get("channel").and_then(|c| c.as_str()) {
            Some("orderUpdates") => {
                tracing::info!("📋 Order update received");
                if let Some(data) = message.get("data") {
                    if let Some(orders) = data.as_array() {
                        for order in orders {
                            if let (Some(coin), Some(side), Some(sz), Some(px)) = (
                                order.get("coin").and_then(|c| c.as_str()),
                                order.get("side").and_then(|s| s.as_str()),
                                order.get("sz").and_then(|s| s.as_str()),
                                order.get("px").and_then(|p| p.as_str())
                            ) {
                                tracing::info!("   🔄 {} {} {} @ ${}", coin, side, sz, px);
                            }
                        }
                    }
                }
            }
            Some("userEvents") => {
                tracing::info!("👤 User event received");
                if let Some(data) = message.get("data") {
                    if let Some(fills) = data.get("fills").and_then(|f| f.as_array()) {
                        for fill in fills {
                            if let (Some(coin), Some(dir), Some(sz), Some(px)) = (
                                fill.get("coin").and_then(|c| c.as_str()),
                                fill.get("dir").and_then(|d| d.as_str()),
                                fill.get("sz").and_then(|s| s.as_str()),
                                fill.get("px").and_then(|p| p.as_str())
                            ) {
                                tracing::info!("   ✅ Fill: {} {} {} @ ${}", coin, dir, sz, px);
                            }
                        }
                    }
                    
                    if let Some(liquidations) = data.get("liquidations").and_then(|l| l.as_array()) {
                        for liquidation in liquidations {
                            if let Some(coin) = liquidation.get("coin").and_then(|c| c.as_str()) {
                                tracing::warn!("   ⚠️  Liquidation event for {}", coin);
                            }
                        }
                    }
                }
            }
            Some("notification") => {
                tracing::info!("🔔 Notification received");
                if let Some(notification) = message.get("data").and_then(|d| d.as_str()) {
                    tracing::info!("   📢 {}", notification);
                }
            }
            Some("subscriptionResponse") => {
                tracing::debug!("✅ Subscription confirmed");
            }
            Some(channel) => {
                tracing::debug!("📨 Received {} message", channel);
            }
            None => {
                tracing::debug!("📨 Received message without channel");
            }
        }
    });

    // Create WebSocket client with credentials for authenticated feeds
    let mut client = HyperliquidWebSocketClient::new(
        Some("wss://api.hyperliquid-testnet.xyz/ws".to_string()),
        Some(credentials),
    )?;
    client.set_message_handler(handler);

    tracing::info!("🔌 Connecting to Hyperliquid WebSocket (authenticated)...");

    // Connect with retry logic
    match client.connect_with_retry(3).await {
        Ok(_) => {
            tracing::info!("✅ Connected successfully!");
        }
        Err(e) => {
            tracing::error!("❌ Failed to connect after retries: {}", e);
            return Ok(());
        }
    }

    // Subscribe to execution-related feeds
    tracing::info!("🔐 Setting up authenticated subscriptions...");

    // Subscribe to order updates
    if let Err(e) = client.subscribe_order_updates().await {
        tracing::error!("❌ Failed to subscribe to order updates: {}", e);
    } else {
        tracing::info!("✅ Subscribed to order updates");
    }

    // Subscribe to user events (fills, liquidations, etc.)
    if let Err(e) = client.subscribe_user_events().await {
        tracing::error!("❌ Failed to subscribe to user events: {}", e);
    } else {
        tracing::info!("✅ Subscribed to user events");
    }

    // Subscribe to notifications
    if let Err(e) = client.subscribe_notification().await {
        tracing::error!("❌ Failed to subscribe to notifications: {}", e);
    } else {
        tracing::info!("✅ Subscribed to notifications");
    }

    tracing::info!("🎧 Listening for execution events...");
    tracing::info!("💡 This client will receive:");
    tracing::info!("   📋 Order status updates (filled, cancelled, etc.)");
    tracing::info!("   ✅ Trade fills and execution reports");
    tracing::info!("   ⚠️  Liquidation events");
    tracing::info!("   🔔 Account notifications");

    // Set up CTRL+C handler
    let mut sigint = Box::pin(signal::ctrl_c());

    // Main event loop - monitor connection and wait for CTRL+C
    loop {
        tokio::select! {
            // Handle CTRL+C
            Ok(_) = sigint.as_mut() => {
                tracing::info!("🛑 Received CTRL+C, shutting down...");
                break;
            }
            // Monitor connection every 60 seconds
            _ = sleep(Duration::from_secs(60)) => {
                let connected = client.is_connected();
                let attempts = client.reconnect_attempts();
                let heartbeat = client.time_since_heartbeat().unwrap_or(Duration::from_secs(0));
                
                tracing::info!(
                    "📊 Status: Connected={}, Reconnect attempts={}, Heartbeat={:.1}s ago",
                    connected,
                    attempts,
                    heartbeat.as_secs_f64()
                );
                
                // Attempt reconnection if disconnected
                if !connected {
                    tracing::warn!("🔄 Connection lost, attempting to reconnect...");
                    if let Err(e) = client.connect_with_retry(3).await {
                        tracing::error!("❌ Reconnection failed: {}", e);
                        break;
                    } else {
                        tracing::info!("✅ Reconnected successfully!");
                        
                        // Re-subscribe after reconnection
                        tracing::info!("🔄 Re-subscribing to feeds...");
                        let _ = client.subscribe_order_updates().await;
                        let _ = client.subscribe_user_events().await;
                        let _ = client.subscribe_notification().await;
                    }
                }
            }
        }
    }

    // Cleanup
    tracing::info!("🧹 Disconnecting...");
    if let Err(e) = client.disconnect().await {
        tracing::error!("❌ Error during disconnect: {}", e);
    }

    tracing::info!("👋 WebSocket execution client shutdown complete");
    tracing::info!("💡 Set environment variables HYPERLIQUID_TESTNET_PRIVATE_KEY and HYPERLIQUID_TESTNET_WALLET_ADDRESS for real usage");
    
    Ok(())
}
