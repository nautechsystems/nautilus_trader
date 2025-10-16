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

//! Minimal WS post example: info (l2Book) and an order action with real signing.

use std::{env, time::Duration};

use nautilus_hyperliquid::{
    common::{consts::ws_url, credential::EvmPrivateKey},
    signing::{HyperliquidActionType, HyperliquidEip712Signer, SignRequest, TimeNonce},
    websocket::{
        client::HyperliquidWebSocketClient,
        messages::{ActionPayload, ActionRequest, SignatureData, TimeInForceRequest},
        post::{Grouping, OrderBuilder},
    },
};
use tracing::level_filters::LevelFilter;
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

    tracing::info!(component = "ws_post", %ws_url, ?testnet, "connecting");
    let client = HyperliquidWebSocketClient::connect(ws_url).await?;
    tracing::info!(component = "ws_post", "websocket connected");

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
    tracing::info!(component = "ws_post", best_bid = %best_bid, best_ask = %best_ask, "BTC top of book");

    // Only attempt the action when explicitly requested (HYPERLIQUID_SEND=1).
    let should_send = env::var("HYPERLIQUID_SEND")
        .map(|v| v == "1")
        .unwrap_or(false);
    if !should_send {
        tracing::warn!(
            component = "ws_post",
            "skipping action: set HYPERLIQUID_SEND=1 to send the order"
        );
        return Ok(());
    }

    if best_bid.is_empty() {
        tracing::warn!(
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

    // Get private key from environment for signing
    let private_key_str = env::var("HYPERLIQUID_PK").map_err(
        |_| "HYPERLIQUID_PK environment variable not set. Example: export HYPERLIQUID_PK=0x...",
    )?;
    let private_key = EvmPrivateKey::new(private_key_str)?;
    let signer = HyperliquidEip712Signer::new(private_key);

    // Convert action to JSON for signing
    let action_json = serde_json::to_value(&action)?;

    // Get current nonce (Unix timestamp in milliseconds)
    let nonce = TimeNonce::now_millis();

    // Sign the action
    let sign_request = SignRequest {
        action: action_json,
        action_bytes: None,
        time_nonce: nonce,
        action_type: HyperliquidActionType::UserSigned,
        is_testnet: false,
        vault_address: None,
    };

    let signature_bundle = signer.sign(&sign_request)?;

    // Parse signature into r, s, v components
    // Format is: 0x + r(64 hex) + s(64 hex) + v(2 hex) = 132 chars total
    let sig = signature_bundle.signature;
    if sig.len() != 132 || !sig.starts_with("0x") {
        return Err(format!("Invalid signature format: {}", sig).into());
    }

    let signature = SignatureData {
        r: format!("0x{}", &sig[2..66]),    // Extract r component
        s: format!("0x{}", &sig[66..130]),  // Extract s component
        v: format!("0x{}", &sig[130..132]), // Extract v component
    };

    tracing::info!(component = "ws_post", "action signed successfully");

    let payload = ActionPayload {
        action,
        nonce: nonce.as_millis() as u64,
        signature,
        vault_address: None,
    };

    match client
        .post_action_raw(payload, Duration::from_secs(2))
        .await
    {
        Ok(resp) => tracing::info!(component = "ws_post", ?resp, "action response (success)"),
        Err(e) => {
            tracing::warn!(component = "ws_post", error = %e, "action failed");
        }
    }

    Ok(())
}
