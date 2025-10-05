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

//! Minimal WS post example: info (l2Book) and a stubbed order action.

use std::{env, time::Duration};

use nautilus_hyperliquid::{
    common::consts::ws_url,
    websocket::{
        client::HyperliquidWebSocketClient,
        messages::{ActionPayload, ActionRequest, SignatureData, TimeInForceRequest},
        post::{Grouping, OrderBuilder},
    },
};
use tracing::{info, level_filters::LevelFilter, warn};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Structured logging with env-controlled filter (e.g. RUST_LOG=debug)
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(env_filter)
        .with_max_level(LevelFilter::INFO)
        .init();

    let args: Vec<String> = env::args().collect();
    let testnet = args.get(1).is_some_and(|s| s == "testnet");
    let ws_url = ws_url(testnet);

    info!(component = "ws_post", %ws_url, ?testnet, "connecting");
    let mut client = HyperliquidWebSocketClient::connect(ws_url).await?;
    info!(component = "ws_post", "websocket connected");

    let book = client.info_l2_book("BTC", Duration::from_secs(2)).await?;
    let best_bid = book
        .levels
        .first()
        .and_then(|bids| bids.first())
        .map(|l| l.px.clone())
        .unwrap_or_default();
    let best_ask = book
        .levels
        .get(1)
        .and_then(|asks| asks.first())
        .map(|l| l.px.clone())
        .unwrap_or_default();
    info!(component = "ws_post", best_bid = %best_bid, best_ask = %best_ask, "BTC top of book");

    // Only attempt the action when explicitly requested (HL_SEND=1).
    let should_send = std::env::var("HL_SEND").map(|v| v == "1").unwrap_or(false);
    if !should_send {
        warn!(
            component = "ws_post",
            "skipping action: set HL_SEND=1 to send the stubbed order"
        );
        return Ok(());
    }

    if best_bid.is_empty() {
        warn!(
            component = "ws_post",
            "no best bid available; aborting action"
        );
        return Ok(());
    }

    // === ACTION (stub): place a post-only limit (requires real signature!) ===
    let action: ActionRequest = OrderBuilder::new()
        .grouping(Grouping::Na)
        .push_limit(
            /*asset*/ 0, // BTC (adapter maps 0 → BTC)
            /*is_buy*/ true, // buy
            /*px*/ best_bid.clone(), // price from book
            /*sz*/ "0.001", // size
            /*reduce_only*/ false,
            TimeInForceRequest::Alo, // post-only
            Some("test-cloid-1".to_string()),
        )
        .build();

    // TODO: sign properly; below is a placeholder signature (r,s,v must be valid!)
    let payload = ActionPayload {
        action,
        nonce: 0, // e.g., time-based nonce or your NonceManager
        signature: SignatureData {
            r: "0x0".into(),
            s: "0x0".into(),
            v: "0x1b".into(),
        },
        vault_address: None,
    };

    match client
        .post_action_raw(payload, Duration::from_secs(2))
        .await
    {
        Ok(resp) => info!(component = "ws_post", ?resp, "action response"),
        Err(e) => {
            warn!(component = "ws_post", error = %e, "action failed (expected with dummy signature)")
        }
    }

    Ok(())
}
