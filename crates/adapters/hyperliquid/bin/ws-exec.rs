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

//! Hyperliquid WebSocket execution example.
//!
//! This example demonstrates:
//! - Connecting to Hyperliquid WebSocket execution endpoint
//! - Subscribing to order updates and user events
//! - Placing limit and market orders
//! - Canceling orders
//! - Handling execution responses and order updates
//!
//! ## Usage
//! ```bash
//! cargo run --bin hyperliquid-ws-exec
//! ```
//!
//! ## Environment Variables
//! - `HYPERLIQUID_API_KEY`: Your Hyperliquid API key
//! - `HYPERLIQUID_SECRET`: Your Hyperliquid API secret
//! - `HYPERLIQUID_TESTNET`: Set to "true" for testnet (optional, defaults to mainnet)

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use nautilus_hyperliquid::websocket::{
    client::HyperliquidWebSocketClient,
    messages::{
        ActionPayload, ActionRequest, CancelRequest, HyperliquidWsRequest, OrderRequest,
        OrderTypeRequest, PostRequest, SignatureData, SubscriptionRequest, TimeInForceRequest,
    },
};
use nautilus_model::{
    enums::OrderSide,
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use tokio::{pin, signal, time::sleep};
use tracing::{debug, error, info, warn};
use tracing_subscriber::filter::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::DEBUG)
        .init();

    info!("Starting Hyperliquid WebSocket execution example");

    // Determine WebSocket URL based on environment
    let is_testnet = std::env::var("HYPERLIQUID_TESTNET")
        .unwrap_or_default()
        .to_lowercase()
        == "true";

    let ws_url = if is_testnet {
        "wss://api.hyperliquid-testnet.xyz/ws"
    } else {
        "wss://api.hyperliquid.xyz/ws"
    };

    info!("Connecting to {} (testnet: {})", ws_url, is_testnet);

    // Connect to WebSocket
    let (client, message_stream) = HyperliquidWebSocketClient::connect(ws_url).await?;
    info!("Successfully connected to Hyperliquid WebSocket");

    // Subscribe to private execution channels
    subscribe_to_execution_channels(&client).await?;

    // Wait for subscriptions to be active
    sleep(Duration::from_secs(2)).await;

    // Define trading parameters using Nautilus model types
    let trader_id = TraderId::from("TRADER-001");
    let strategy_id = StrategyId::from("HYPERLIQUID-EXAMPLE");
    let instrument_id = InstrumentId::from("BTC-USD.HYPERLIQUID");
    let client_order_id = ClientOrderId::from("HL20250902001");

    // Example: Place a limit order
    let order_result = place_limit_order(
        &client,
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.001"),
        Some("50000.0".to_string()),
    )
    .await;
    match order_result {
        Ok(()) => info!("Limit order placement request sent"),
        Err(e) => error!("Failed to place limit order: {}", e),
    }

    // Wait a bit before placing another order
    sleep(Duration::from_secs(3)).await;

    // Example: Place a market order
    let market_client_order_id = ClientOrderId::from("HL20250902002");
    let market_order_result = place_market_order(
        &client,
        trader_id,
        strategy_id,
        instrument_id,
        market_client_order_id,
        OrderSide::Sell,
        Quantity::from("0.001"),
    )
    .await;
    match market_order_result {
        Ok(()) => info!("Market order placement request sent"),
        Err(e) => error!("Failed to place market order: {}", e),
    }

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    info!("Listening for execution updates (Press Ctrl+C to exit)...");

    // Convert the receiver into a stream for consistent API with other adapters
    let stream = futures_util::stream::unfold(message_stream, |mut stream| async move {
        stream.recv().await.map(|msg| (msg, stream))
    });
    tokio::pin!(stream);

    // Main event loop
    loop {
        tokio::select! {
            Some(message) = stream.next() => {
                handle_execution_message(message).await;
            }
            _ = &mut sigint => {
                info!("Received SIGINT, closing connection...");
                break;
            }
            else => {
                warn!("Message stream ended unexpectedly");
                break;
            }
        }
    }

    info!("Shutting down...");
    Ok(())
}

/// Subscribe to execution-related WebSocket channels
async fn subscribe_to_execution_channels(
    client: &HyperliquidWebSocketClient,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Subscribing to execution channels...");

    // Subscribe to order updates
    let order_updates_request = HyperliquidWsRequest::Subscribe {
        subscription: SubscriptionRequest::OrderUpdates {
            user: get_user_address(),
        },
    };
    client.send(&order_updates_request).await?;
    info!("Subscribed to order updates");

    // Subscribe to user events (fills, funding, etc.)
    let user_events_request = HyperliquidWsRequest::Subscribe {
        subscription: SubscriptionRequest::UserEvents {
            user: get_user_address(),
        },
    };
    client.send(&user_events_request).await?;
    info!("Subscribed to user events");

    Ok(())
}

/// Place a limit order example
#[allow(clippy::too_many_arguments)]
async fn place_limit_order(
    client: &HyperliquidWebSocketClient,
    _trader_id: TraderId,
    _strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    quantity: Quantity,
    price: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Placing limit order: {} {} {} @ {}",
        side,
        quantity,
        instrument_id,
        price.as_ref().unwrap_or(&"MARKET".to_string())
    );

    let order = OrderRequest {
        a: 0, // BTC asset ID (example - should be dynamic based on instrument_id)
        b: matches!(side, OrderSide::Buy), // Convert OrderSide to bool
        p: price.unwrap_or_else(|| "50000.0".to_string()), // Price
        s: quantity.to_string(), // Size
        r: false, // Not reduce-only
        t: OrderTypeRequest::Limit {
            tif: TimeInForceRequest::Gtc, // Good Till Cancel
        },
        c: Some(client_order_id.to_string()), // Client order ID
    };

    let action_request = ActionRequest::Order {
        orders: vec![order],
        grouping: "na".to_string(), // No grouping
    };

    let post_request = create_signed_post_request(action_request).await?;
    client.send(&post_request).await?;

    Ok(())
}

/// Place a market order example
async fn place_market_order(
    client: &HyperliquidWebSocketClient,
    _trader_id: TraderId,
    _strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    quantity: Quantity,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Placing market order: {} {} {}",
        side, quantity, instrument_id
    );

    let order = OrderRequest {
        a: 0, // BTC asset ID (example - should be dynamic based on instrument_id)
        b: matches!(side, OrderSide::Buy), // Convert OrderSide to bool
        p: "0".to_string(), // Market orders use 0 for price
        s: quantity.to_string(), // Size
        r: false, // Not reduce-only
        t: OrderTypeRequest::Limit {
            tif: TimeInForceRequest::Ioc, // Immediate or Cancel (simulates market order)
        },
        c: Some(client_order_id.to_string()), // Client order ID
    };

    let action_request = ActionRequest::Order {
        orders: vec![order],
        grouping: "na".to_string(),
    };

    let post_request = create_signed_post_request(action_request).await?;
    client.send(&post_request).await?;

    Ok(())
}

/// Cancel an order example
#[allow(dead_code)]
async fn cancel_order(
    client: &HyperliquidWebSocketClient,
    asset_id: u32,
    order_id: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Canceling order {} on asset {}", order_id, asset_id);

    let cancel_request = CancelRequest {
        a: asset_id,
        o: order_id,
    };

    let action_request = ActionRequest::Cancel {
        cancels: vec![cancel_request],
    };

    let post_request = create_signed_post_request(action_request).await?;
    client.send(&post_request).await?;

    Ok(())
}

/// Create a signed POST request for actions
async fn create_signed_post_request(
    action: ActionRequest,
) -> Result<HyperliquidWsRequest, Box<dyn std::error::Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;

    // NOTE: In a real implementation, you would:
    // 1. Load your private key from environment variables or secure storage
    // 2. Create the proper signature hash of the action + nonce
    // 3. Sign with your private key using secp256k1
    // 4. Format the signature as r, s, v components
    //
    // For this example, we use placeholder values
    let signature = create_placeholder_signature();

    let payload = ActionPayload {
        action,
        nonce,
        signature,
        vault_address: None,
    };

    Ok(HyperliquidWsRequest::Post {
        id: nonce,
        request: PostRequest::Action { payload },
    })
}

/// Create a placeholder signature (DO NOT use in production)
fn create_placeholder_signature() -> SignatureData {
    SignatureData {
        r: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        s: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        v: "0x1b".to_string(),
    }
}

/// Generate a unique client order ID
#[allow(dead_code)]
fn generate_client_order_id() -> String {
    format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    )
}

/// Get user address (placeholder - should come from your wallet/API key)
fn get_user_address() -> String {
    std::env::var("HYPERLIQUID_USER_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string())
}

/// Handle incoming execution messages
async fn handle_execution_message(
    message: nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage,
) {
    debug!("Received message: {:?}", message);

    match message {
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::OrderUpdates { data } => {
            info!("📋 Order Updates: {:?}", data);
            // Parse and handle order status changes
        }
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::UserEvents { data } => {
            info!("👤 User Events: {:?}", data);
            // Parse and handle fills, funding, liquidations, etc.
        }
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::UserFills { data } => {
            info!("💰 User Fills: {:?}", data);
            // Handle trade executions
        }
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::UserFundings { data } => {
            info!("💸 User Funding: {:?}", data);
            // Handle funding payments
        }
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::Post { data } => {
            info!("📨 Post Response: {:?}", data);
            // Handle responses to POST requests (order placement, cancellation, etc.)
        }
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::SubscriptionResponse {
            data,
        } => {
            info!("✅ Subscription Confirmed: {:?}", data);
        }
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::Notification { data } => {
            info!("🔔 Notification: {:?}", data);
        }
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::Trades { data } => {
            debug!("📈 Trades: {} trades received", data.len());
        }
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::L2Book { data } => {
            debug!("📊 L2 Book Update: {:?}", data);
        }
        nautilus_hyperliquid::websocket::messages::HyperliquidWsMessage::Pong => {
            debug!("🏓 Pong received");
        }
        _ => {
            debug!("📦 Other message type received: {:?}", message);
        }
    }
}
