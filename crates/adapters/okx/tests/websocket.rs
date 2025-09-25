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

//! Integration tests for the OKX WebSocket client using a mock Axum server.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use futures_util::{StreamExt, pin_mut};
use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{AccountId, InstrumentId};
use nautilus_okx::{
    common::parse::parse_instrument_any, http::client::OKXResponse,
    websocket::client::OKXWebSocketClient,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct WsState {
    login_count: std::sync::Arc<Mutex<usize>>,
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_value(filename: &str) -> Value {
    let path = manifest_path().join("test_data").join(filename);
    let content = std::fs::read_to_string(path).expect("failed to read test data");
    serde_json::from_str(&content).expect("failed to parse test data")
}

fn load_instruments() -> Vec<nautilus_model::instruments::InstrumentAny> {
    let raw = load_value("http_get_instruments_spot.json");
    let response: OKXResponse<nautilus_okx::common::models::OKXInstrument> =
        serde_json::from_value(raw).expect("invalid instrument response");
    let ts_init = UnixNanos::default();
    response
        .data
        .iter()
        .filter_map(|inst| parse_instrument_any(inst, ts_init).ok().flatten())
        .collect()
}

async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<WsState>) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: WsState) {
    let trades_payload = load_value("ws_trades.json");

    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(text) => {
                let payload: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(_) => continue,
                };

                if payload.get("op") == Some(&json!("login")) {
                    let mut counter = state.login_count.lock().await;
                    *counter += 1;

                    let response = json!({
                        "event": "login",
                        "code": "0",
                        "msg": "",
                        "connId": "test-conn",
                    });
                    if socket
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }

                if payload.get("op") == Some(&json!("subscribe")) {
                    if let Some(args) = payload.get("args").and_then(|v| v.as_array()) {
                        if let Some(first) = args.first() {
                            let ack = json!({
                                "event": "subscribe",
                                "arg": first,
                                "connId": "test-conn",
                            });
                            if socket
                                .send(Message::Text(ack.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                            if socket
                                .send(Message::Text(trades_payload.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    continue;
                }
            }
            Message::Ping(data) => {
                if socket.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn start_ws_server() -> SocketAddr {
    let router = Router::new()
        .route("/ws", get(handle_ws_upgrade))
        .with_state(WsState::default());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind ws listener");
    let addr = listener.local_addr().expect("missing local addr");

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("websocket server failed");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn test_websocket_trades_subscription_flow() {
    let addr = start_ws_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let mut client = OKXWebSocketClient::new(
        Some(ws_url),
        Some("api_key".to_string()),
        Some("api_secret".to_string()),
        Some("passphrase".to_string()),
        Some(AccountId::from("OKX-001")),
        Some(30),
    )
    .expect("failed to construct client");

    let instruments = load_instruments();
    client.initialize_instruments_cache(instruments);

    client.connect().await.expect("failed to connect");
    client
        .wait_until_active(1.0)
        .await
        .expect("connection inactive");

    let instrument_id = InstrumentId::from("BTC-USD.OKX");

    client
        .subscribe_trades(instrument_id, false)
        .await
        .expect("subscribe failed");

    let stream = client.stream();
    pin_mut!(stream);
    let message = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("no websocket message received")
        .expect("stream ended unexpectedly");

    match message {
        nautilus_okx::websocket::messages::NautilusWsMessage::Data(data) => {
            assert!(!data.is_empty(), "expected trade data");
        }
        other => panic!("unexpected websocket message: {other:?}"),
    }

    client.close().await.expect("failed to close client");
}
