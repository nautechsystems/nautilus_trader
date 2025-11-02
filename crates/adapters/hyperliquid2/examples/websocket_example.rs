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

//! Example demonstrating the Hyperliquid WebSocket client.

use std::sync::Arc;
use std::time::Duration;

use nautilus_hyperliquid::websocket::client::{HyperliquidWebSocketClient, MessageHandler};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize TLS crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    
    println!("🚀 Hyperliquid WebSocket Client Example");
    
    // Create message handler
    let handler: MessageHandler = Arc::new(|message| {
        println!("📨 Received message: {}", serde_json::to_string_pretty(&message).unwrap_or_default());
    });
    
    // Create WebSocket client
    let mut client = HyperliquidWebSocketClient::new(None, None)?;
    client.set_message_handler(handler);
    
    println!("🔌 Connecting to Hyperliquid WebSocket...");
    
    // Connect with retry logic
    match client.connect_with_retry(3).await {
        Ok(_) => {
            println!("✅ Connected successfully!");
            
            // Subscribe to different data streams
            println!("📊 Subscribing to market data...");
            
            // Subscribe to all mids (quote data)
            client.subscribe_all_mids().await?;
            println!("✅ Subscribed to allMids");
            
            // Subscribe to BTC order book
            client.subscribe_l2_book("BTC").await?;
            println!("✅ Subscribed to BTC L2 book");
            
            // Subscribe to BTC trades
            client.subscribe_trades("BTC").await?;
            println!("✅ Subscribed to BTC trades");
            
            // Keep connection alive and monitor
            println!("🔄 Listening for messages... (Press Ctrl+C to exit)");
            
            for i in 0..60 {
                sleep(Duration::from_secs(5)).await;
                
                let connected = client.is_connected();
                let attempts = client.reconnect_attempts();
                let heartbeat = client.time_since_heartbeat().unwrap_or(Duration::from_secs(0));
                
                println!(
                    "📊 Status check {}: Connected={}, Reconnect attempts={}, Time since heartbeat={:.1}s",
                    i + 1,
                    connected,
                    attempts,
                    heartbeat.as_secs_f64()
                );
                
                if !connected {
                    println!("❌ Connection lost, attempting to reconnect...");
                    if let Err(e) = client.connect_with_retry(3).await {
                        println!("❌ Reconnection failed: {}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ Failed to connect: {}", e);
        }
    }
    
    // Cleanup
    println!("🧹 Cleaning up...");
    client.disconnect().await?;
    println!("👋 Disconnected from Hyperliquid WebSocket");
    
    Ok(())
}