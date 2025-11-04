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

//! Example: Using Lighter WebSocket client for real-time data.

use futures_util::StreamExt;
use nautilus_lighter2::{
    common::enums::LighterWsChannel,
    websocket::LighterWebSocketClient,
};
use tracing::{error, info};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Lighter WebSocket client example");

    // Create WebSocket client
    let client = LighterWebSocketClient::new(
        None,  // Use default mainnet HTTP URL
        None,  // Use default mainnet WS URL
        false, // Not testnet
        None,  // No credentials for public channels
    );

    // Subscribe to order book for market 0
    info!("Subscribing to order book for market 0...");
    client.subscribe(LighterWsChannel::OrderBook { market_id: 0 }).await?;

    // Subscribe to trades for market 0
    info!("Subscribing to trades for market 0...");
    client.subscribe(LighterWsChannel::Trades { market_id: 0 }).await?;

    info!("Active subscriptions: {}", client.subscription_count().await);

    // Note: WebSocket streaming would normally be implemented here
    // This is a placeholder example showing subscription management
    info!("WebSocket subscriptions set up successfully");
    info!("In a real implementation, you would:");
    info!("  1. Connect to the WebSocket");
    info!("  2. Stream messages");
    info!("  3. Handle order book and trade updates");

    // Example of what the streaming code would look like:
    // let mut stream = client.connect().await?;
    // while let Some(result) = stream.next().await {
    //     match result {
    //         Ok(message) => info!("Message: {:?}", message),
    //         Err(e) => error!("Error: {}", e),
    //     }
    // }

    info!("Example completed!");
    Ok(())
}
