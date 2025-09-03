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

use std::time::Duration;

use nautilus_hyperliquid::{
    common::consts::{HyperliquidNetwork, ws_url},
    websocket::{
        client::HyperliquidWebSocketClient,
        messages::{HyperliquidWsRequest, SubscriptionRequest},
    },
};
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    tracing::info!("Starting Hyperliquid WebSocket execution example");

    // Determine network and get WebSocket URL
    let network = HyperliquidNetwork::from_env();
    let ws_url = ws_url(network);

    let (mut client, message_rx) = HyperliquidWebSocketClient::connect(ws_url).await?;
    tracing::info!("Connected to Hyperliquid WebSocket");

    // Subscribe to execution channels
    let user_addr = std::env::var("HYPERLIQUID_USER_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string());

    // Subscribe to order updates
    let order_updates_request = HyperliquidWsRequest::Subscribe {
        subscription: SubscriptionRequest::OrderUpdates {
            user: user_addr.clone(),
        },
    };
    client.send(&order_updates_request).await?;

    // Subscribe to user events
    let user_events_request = HyperliquidWsRequest::Subscribe {
        subscription: SubscriptionRequest::UserEvents {
            user: user_addr.clone(),
        },
    };
    client.send(&user_events_request).await?;

    // Wait briefly to ensure subscriptions are active
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    let stream = message_rx;
    tokio::pin!(stream);

    loop {
        tokio::select! {
            Some(message) = stream.recv() => {
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
