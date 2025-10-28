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

use std::{env, time::Duration};

use nautilus_hyperliquid::{common::consts::ws_url, websocket::client::HyperliquidWebSocketClient};
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let args: Vec<String> = env::args().collect();
    let testnet = args.get(1).is_some_and(|s| s == "testnet");

    tracing::info!("Starting Hyperliquid WebSocket execution example");
    tracing::info!("Testnet: {testnet}");

    let ws_url = ws_url(testnet);
    tracing::info!("WebSocket URL: {ws_url}");

    let client = HyperliquidWebSocketClient::connect(ws_url).await?;
    tracing::info!("Connected to Hyperliquid WebSocket");

    // Subscribe to execution channels
    let user_addr = env::var("HYPERLIQUID_USER_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string());

    // Subscribe to all user channels using the convenience method
    client.subscribe_all_user_channels(&user_addr).await?;
    tracing::info!("Subscribed to all user channels for {}", user_addr);

    // Wait briefly to ensure subscriptions are active
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    loop {
        tokio::select! {
            Some(message) = client.next_event() => {
                tracing::debug!("{message:?}");
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                client.disconnect().await?;
                break;
            }
            else => break,
        }
    }

    Ok(())
}
