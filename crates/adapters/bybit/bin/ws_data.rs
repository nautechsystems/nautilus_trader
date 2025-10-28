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

//! Connects to the Bybit public WebSocket feed and streams specified market data topics.
//! Useful when manually validating the Rust WebSocket client implementation.

use std::error::Error;

use futures_util::StreamExt;
use nautilus_bybit::{
    common::enums::{BybitEnvironment, BybitProductType},
    websocket::{client::BybitWebSocketClient, messages::BybitWebSocketMessage},
};
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .with_target(false)
        .compact()
        .init();

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        None,
        None,
    );
    client.connect().await?;

    client
        .subscribe(vec![
            "orderbook.1.BTCUSDT".to_string(),
            "publicTrade.BTCUSDT".to_string(),
            "tickers.BTCUSDT".to_string(),
        ])
        .await?;

    let stream = client.stream();
    let shutdown = signal::ctrl_c();
    pin!(stream);
    pin!(shutdown);

    tracing::info!("Streaming Bybit market data; press Ctrl+C to exit");

    loop {
        tokio::select! {
            Some(event) = stream.next() => {
                match event {
                    BybitWebSocketMessage::Orderbook(msg) => {
                        tracing::info!(topic = %msg.topic, "orderbook update");
                    }
                    BybitWebSocketMessage::Trade(msg) => {
                        tracing::info!(topic = %msg.topic, trades = msg.data.len(), "trade update");
                    }
                    BybitWebSocketMessage::TickerLinear(msg) => {
                        tracing::info!(topic = %msg.topic, bid = ?msg.data.bid1_price, ask = ?msg.data.ask1_price, "linear ticker update");
                    }
                    BybitWebSocketMessage::TickerOption(msg) => {
                        tracing::info!(topic = %msg.topic, bid = %msg.data.bid_price, ask = %msg.data.ask_price, "option ticker update");
                    }
                    BybitWebSocketMessage::Response(msg) => {
                        tracing::debug!(?msg, "response frame");
                    }
                    BybitWebSocketMessage::Subscription(msg) => {
                        tracing::info!(op = %msg.op, success = msg.success, "subscription ack");
                    }
                    BybitWebSocketMessage::Auth(msg) => {
                        tracing::info!(op = %msg.op, "auth ack");
                    }
                    BybitWebSocketMessage::Error(e) => {
                        tracing::error!(code = e.code, message = %e.message, "bybit websocket error");
                    }
                    BybitWebSocketMessage::Raw(value) => {
                        tracing::debug!(payload = %value, "raw message");
                    }
                    BybitWebSocketMessage::Reconnected => {
                        tracing::warn!("WebSocket reconnected");
                    }
                    BybitWebSocketMessage::Pong => {
                        tracing::trace!("Received pong");
                    }
                    BybitWebSocketMessage::Kline(msg) => {
                        tracing::info!(topic = %msg.topic, bars = msg.data.len(), "kline update");
                    }
                    BybitWebSocketMessage::AccountOrder(msg) => {
                        tracing::info!(topic = %msg.topic, orders = msg.data.len(), "account order update");
                    }
                    BybitWebSocketMessage::AccountExecution(msg) => {
                        tracing::info!(topic = %msg.topic, executions = msg.data.len(), "account execution update");
                    }
                    BybitWebSocketMessage::AccountWallet(msg) => {
                        tracing::info!(topic = %msg.topic, wallets = msg.data.len(), "account wallet update");
                    }
                    BybitWebSocketMessage::AccountPosition(msg) => {
                        tracing::info!(topic = %msg.topic, positions = msg.data.len(), "account position update");
                    }
                }
            }
            _ = &mut shutdown => {
                tracing::info!("Received Ctrl+C, closing connection");
                client.close().await?;
                break;
            }
            else => break,
        }
    }

    Ok(())
}
