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

//! Manual verification script for dYdX WebSocket private channels (subaccount updates).
//!
//! Subscribes to subaccount order, fill, and position updates via WebSocket.
//! dYdX v4 uses wallet-based subscriptions (no API key signing required for WS).
//!
//! Usage:
//! ```bash
//! # Test against testnet (default)
//! DYDX_MNEMONIC="your mnemonic" cargo run --bin dydx-ws-exec -p nautilus-dydx
//!
//! # Test against mainnet
//! DYDX_MNEMONIC="your mnemonic" \
//! DYDX_WS_URL=wss://indexer.dydx.trade/v4/ws \
//! DYDX_HTTP_URL=https://indexer.dydx.trade \
//! cargo run --bin dydx-ws-exec -p nautilus-dydx -- --mainnet
//!
//! # With custom subaccount
//! DYDX_MNEMONIC="your mnemonic" cargo run --bin dydx-ws-exec -p nautilus-dydx -- --subaccount 1
//! ```

use std::{env, time::Duration};

use nautilus_dydx::{
    common::consts::{DYDX_TESTNET_HTTP_URL, DYDX_TESTNET_WS_URL},
    grpc::wallet::Wallet,
    http::client::DydxHttpClient,
    websocket::client::DydxWebSocketClient,
};
use tracing::level_filters::LevelFilter;

const DEFAULT_SUBACCOUNT: u32 = 0;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::DEBUG)
        .init();

    let args: Vec<String> = env::args().collect();
    let is_mainnet = args.iter().any(|a| a == "--mainnet");
    let subaccount_number = args
        .iter()
        .position(|a| a == "--subaccount")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(DEFAULT_SUBACCOUNT);

    let mnemonic = env::var("DYDX_MNEMONIC").expect("DYDX_MNEMONIC environment variable not set");

    let ws_url = if is_mainnet {
        env::var("DYDX_WS_URL").unwrap_or_else(|_| "wss://indexer.dydx.trade/v4/ws".to_string())
    } else {
        env::var("DYDX_WS_URL").unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string())
    };

    let http_url = if is_mainnet {
        env::var("DYDX_HTTP_URL").unwrap_or_else(|_| "https://indexer.dydx.trade".to_string())
    } else {
        env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string())
    };

    tracing::info!("Connecting to dYdX WebSocket API: {}", ws_url);
    tracing::info!(
        "Environment: {}",
        if is_mainnet { "MAINNET" } else { "TESTNET" }
    );
    tracing::info!("Subaccount: {}", subaccount_number);
    tracing::info!("");

    let wallet = Wallet::from_mnemonic(&mnemonic)?;
    let account = wallet.account_offline(subaccount_number)?;
    let wallet_address = account.address.clone();
    tracing::info!("Wallet address: {}", wallet_address);
    tracing::info!("");

    let http_client =
        DydxHttpClient::new(Some(http_url.clone()), Some(30), None, !is_mainnet, None)?;

    tracing::info!("Fetching instruments from HTTP API...");
    let instruments = http_client.request_instruments(None, None, None).await?;
    tracing::info!("Fetched {} instruments", instruments.len());

    let mut ws_client = DydxWebSocketClient::new_public(ws_url, Some(30));
    ws_client.cache_instruments(instruments);
    ws_client.connect().await?;

    tokio::time::sleep(Duration::from_millis(500)).await;

    tracing::info!(
        "Subscribing to subaccount: {}/{}",
        wallet_address,
        subaccount_number
    );
    ws_client
        .subscribe_subaccount(&wallet_address, subaccount_number)
        .await?;

    let Some(mut rx) = ws_client.take_receiver() else {
        tracing::warn!("No inbound WebSocket receiver available; exiting");
        return Ok(());
    };

    let sigint = tokio::signal::ctrl_c();
    tokio::pin!(sigint);

    let mut event_count = 0;
    tracing::info!("Listening for subaccount updates (press Ctrl+C to stop)...");
    tracing::info!("");

    loop {
        tokio::select! {
            maybe_event = rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        event_count += 1;
                        tracing::debug!("[Event #{}] {:?}", event_count, event);

                        match event {
                            nautilus_dydx::websocket::messages::NautilusWsMessage::Order(_) => {
                                tracing::info!("[Event #{}] Order status update received", event_count);
                            }
                            nautilus_dydx::websocket::messages::NautilusWsMessage::Fill(_) => {
                                tracing::info!("[Event #{}] Fill update received", event_count);
                            }
                            nautilus_dydx::websocket::messages::NautilusWsMessage::Position(_) => {
                                tracing::info!("[Event #{}] Position update received", event_count);
                            }
                            _ => {}
                        }
                    }
                    None => {
                        tracing::info!("WebSocket message stream closed");
                        break;
                    }
                }
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                ws_client.disconnect().await?;
                break;
            }
            else => break,
        }
    }

    tracing::info!("");
    tracing::info!("WebSocket execution test completed");
    tracing::info!("Total events received: {}", event_count);

    Ok(())
}
