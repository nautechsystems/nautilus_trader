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

//! Example: Using Gate.io WebSocket client for futures markets.

use nautilus_gateio2::{
    common::enums::GateioWsChannel,
    websocket::GateioWebSocketClient,
};
use tracing::info;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Gate.io WebSocket client example (futures markets)");

    // Create WebSocket client
    let client = GateioWebSocketClient::new(
        None, // Use default HTTP URL
        None, // Use default spot WS URL
        None, // Use default futures WS URL
        None, // Use default options WS URL
        None, // No credentials for public channels
    );

    // Subscribe to futures ticker for BTC_USDT
    info!("\nSubscribing to BTC_USDT futures ticker...");
    client
        .subscribe(GateioWsChannel::FuturesTicker {
            contract: "BTC_USDT".to_string(),
        })
        .await?;

    // Subscribe to futures order book for BTC_USDT
    info!("Subscribing to BTC_USDT futures order book...");
    client
        .subscribe(GateioWsChannel::FuturesOrderBook {
            contract: "BTC_USDT".to_string(),
        })
        .await?;

    // Subscribe to futures trades for BTC_USDT
    info!("Subscribing to BTC_USDT futures trades...");
    client
        .subscribe(GateioWsChannel::FuturesTrades {
            contract: "BTC_USDT".to_string(),
        })
        .await?;

    // Subscribe to ETH_USDT futures ticker
    info!("Subscribing to ETH_USDT futures ticker...");
    client
        .subscribe(GateioWsChannel::FuturesTicker {
            contract: "ETH_USDT".to_string(),
        })
        .await?;

    info!("\nActive subscriptions: {}", client.subscription_count().await);
    info!("Subscriptions: {:?}", client.subscriptions().await);

    // Note: WebSocket streaming would normally be implemented here
    // This is a placeholder example showing subscription management
    info!("\nWebSocket subscriptions set up successfully");
    info!("In a real implementation, you would:");
    info!("  1. Connect to the WebSocket");
    info!("  2. Send subscription messages");
    info!("  3. Stream and process messages");
    info!("  4. Handle ticker, order book, and trade updates");

    info!("\nExample completed!");
    Ok(())
}
