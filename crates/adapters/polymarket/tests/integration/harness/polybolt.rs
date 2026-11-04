// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! PolyBolt mock venue shared by protocol integration tests and focused unit tests.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use axum::{
    Router,
    extract::{
        State,
        ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use futures_util::StreamExt;
use nautilus_common::testing::wait_until_async;
use nautilus_core::Params;
use nautilus_model::data::DataType;
use parking_lot::Mutex;
use serde_json::{Value, json};

pub(crate) const CRYPTO_UPDATE_FIXTURE: &str =
    include_str!("../../../test_data/polybolt_crypto_price_update.json");
pub(crate) const CRYPTO_SNAPSHOT_FIXTURE: &str =
    include_str!("../../../test_data/polybolt_crypto_price_snapshot.json");
pub(crate) const TWAP_UPDATE_FIXTURE: &str =
    include_str!("../../../test_data/polybolt_crypto_twap_update.json");
pub(crate) const EQUITY_UPDATE_FIXTURE: &str =
    include_str!("../../../test_data/polybolt_equity_price_update.json");
pub(crate) const EQUITY_SNAPSHOT_FIXTURE: &str =
    include_str!("../../../test_data/polybolt_equity_price_snapshot.json");
pub(crate) const AUTHED_FIXTURE: &str = include_str!("../../../test_data/polybolt_authed.json");

#[derive(Clone, Default)]
pub(crate) struct TestServerState {
    pub(crate) received_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
    pub(crate) connection_count: Arc<tokio::sync::Mutex<usize>>,
    pub(crate) control_txs:
        Arc<tokio::sync::Mutex<Vec<tokio::sync::mpsc::UnboundedSender<TestServerCommand>>>>,
    pub(crate) reject_auth: Arc<AtomicBool>,
    pub(crate) hold_auth: Arc<AtomicBool>,
    pub(crate) unavailable_auth_remaining: Arc<AtomicUsize>,
    pub(crate) auth_attempts_once: Arc<AtomicBool>,
    pub(crate) auth_counts: Arc<Mutex<Vec<usize>>>,
    pub(crate) auth_count: Arc<AtomicUsize>,
    pub(crate) snapshots: Arc<AtomicBool>,
    pub(crate) snapshot_frames: Arc<AtomicUsize>,
    pub(crate) snapshot_points: Arc<AtomicUsize>,
    pub(crate) subscription_times: Arc<tokio::sync::Mutex<Vec<std::time::Instant>>>,
    pub(crate) snapshot_times: Arc<Mutex<Vec<std::time::Instant>>>,
    pub(crate) sent_frames: Arc<Mutex<Vec<Value>>>,
}

#[derive(Debug)]
pub(crate) enum TestServerCommand {
    SendText(String),
    Close,
    CloseWithCode(u16),
}

impl TestServerState {
    pub(crate) async fn clear_received_payloads(&self) {
        self.received_payloads.lock().await.clear();
    }

    pub(crate) async fn connection_count(&self) -> usize {
        *self.connection_count.lock().await
    }

    pub(crate) async fn payloads(&self) -> Vec<Value> {
        self.received_payloads.lock().await.clone()
    }

    pub(crate) async fn send_text_to_all(&self, text: String) {
        let senders = self.control_txs.lock().await.clone();
        for tx in senders {
            let _ = tx.send(TestServerCommand::SendText(text.clone()));
        }
    }

    pub(crate) async fn close_all_connections(&self) {
        let senders = self.control_txs.lock().await.clone();
        for tx in senders {
            let _ = tx.send(TestServerCommand::Close);
        }
    }

    pub(crate) async fn close_all_connections_with_code(&self, code: u16) {
        let senders = self.control_txs.lock().await.clone();
        for tx in senders {
            let _ = tx.send(TestServerCommand::CloseWithCode(code));
        }
    }
}

async fn handle_polybolt_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_polybolt_socket(socket, state))
}

async fn handle_polybolt_socket(mut socket: WebSocket, state: TestServerState) {
    *state.connection_count.lock().await += 1;

    let connection = {
        let mut counts = state.auth_counts.lock();
        counts.push(0);
        counts.len() - 1
    };

    let (control_tx, mut control_rx) = tokio::sync::mpsc::unbounded_channel();
    state.control_txs.lock().await.push(control_tx);
    let mut sequences = AHashMap::<&str, u64>::new();

    loop {
        let result = tokio::select! {
            command = control_rx.recv() => {
                match command {
                    Some(TestServerCommand::SendText(text)) => {
                        if send_text_frame(&mut socket, &state, &mut sequences, text).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    Some(TestServerCommand::Close) => {
                        let _ = socket.send(AxumWsMessage::Close(None)).await;
                        break;
                    }
                    Some(TestServerCommand::CloseWithCode(code)) => {
                        let frame = axum::extract::ws::CloseFrame {
                            code,
                            reason: "test".into(),
                        };
                        let _ = socket.send(AxumWsMessage::Close(Some(frame))).await;
                        break;
                    }
                    None => break,
                }
            }
            result = socket.next() => result
        };

        let Some(result) = result else { break };
        let Ok(message) = result else { break };

        match message {
            AxumWsMessage::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                state
                    .received_payloads
                    .lock()
                    .await
                    .push(recorded_payload(&payload));

                let replies = if payload["op"] == "auth" {
                    state.auth_counts.lock()[connection] += 1;
                    auth_replies(&state, &payload)
                } else {
                    state
                        .subscription_times
                        .lock()
                        .await
                        .push(std::time::Instant::now());
                    subscription_replies(&state, &payload, &mut sequences)
                };

                for reply in replies {
                    if send_text_frame(&mut socket, &state, &mut sequences, reply.to_string())
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
            AxumWsMessage::Ping(data) => {
                if socket.send(AxumWsMessage::Pong(data)).await.is_err() {
                    break;
                }
            }
            AxumWsMessage::Close(_) => break,
            _ => {}
        }
    }
}

async fn send_text_frame(
    socket: &mut WebSocket,
    state: &TestServerState,
    sequences: &mut AHashMap<&'static str, u64>,
    text: String,
) -> Result<(), axum::Error> {
    let frame = serde_json::from_str::<Value>(&text).ok();
    if let Some(frame) = &frame
        && let Some(seq) = frame["seq"].as_u64()
        && let Some(channel) = frame["channel"].as_str()
        && let Some(channel) = ["price.crypto", "price.equity"]
            .into_iter()
            .find(|name| *name == channel)
    {
        sequences.insert(channel, seq);
    }

    socket.send(AxumWsMessage::Text(text.into())).await?;

    if let Some(frame) = frame
        && frame["payload"].is_object()
    {
        state.sent_frames.lock().push(frame);
    }

    Ok(())
}

fn subscription_replies(
    state: &TestServerState,
    payload: &Value,
    sequences: &mut AHashMap<&str, u64>,
) -> Vec<Value> {
    let mut replies = mock_replies(payload);
    if payload["op"] != "subscribe" || !state.snapshots.load(Ordering::Relaxed) {
        return replies;
    }

    for subscription in payload["subscriptions"].as_array().unwrap() {
        let channel = subscription["channel"].as_str().unwrap();

        let (channel, fixture) = if channel == "price.crypto" {
            ("price.crypto", CRYPTO_SNAPSHOT_FIXTURE)
        } else {
            ("price.equity", EQUITY_SNAPSHOT_FIXTURE)
        };

        let mut snapshot = fixture_json(fixture);
        snapshot["payload"]["symbol"] = subscription["filter"]["symbol"].clone();
        let seq = sequences.entry(channel).or_default();
        *seq += 1;
        snapshot["seq"] = json!(*seq);
        for (index, point) in snapshot["payload"]["data"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            point["timestamp"] = json!(price_timestamp_ms(*seq, index));
        }

        snapshot["dropped"] = json!(0);
        state.snapshot_frames.fetch_add(1, Ordering::Relaxed);
        state.snapshot_points.fetch_add(
            snapshot["payload"]["data"].as_array().unwrap().len(),
            Ordering::Relaxed,
        );
        state.snapshot_times.lock().push(std::time::Instant::now());
        replies.push(snapshot);
    }

    replies
}

fn auth_replies(state: &TestServerState, payload: &Value) -> Vec<Value> {
    state.auth_count.fetch_add(1, Ordering::Relaxed);
    if state.reject_auth.load(Ordering::Relaxed) {
        vec![json!({"op": "error", "code": "auth_invalid", "rid": payload["rid"]})]
    } else if state.hold_auth.load(Ordering::Relaxed) {
        Vec::new()
    } else if state
        .unavailable_auth_remaining
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
            remaining.checked_sub(1)
        })
        .is_ok()
    {
        vec![json!({"op": "error", "code": "auth_unavailable", "rid": payload["rid"]})]
    } else if state.auth_attempts_once.swap(false, Ordering::Relaxed) {
        vec![json!({"op": "error", "code": "auth_attempts", "rid": payload["rid"]})]
    } else {
        mock_replies(payload)
    }
}

pub(crate) async fn start_polybolt_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind PolyBolt test server");
    let addr = listener
        .local_addr()
        .expect("missing PolyBolt test server address");
    let router = Router::new()
        .route("/polybolt", get(handle_polybolt_upgrade))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("PolyBolt test server failed");
    });

    addr
}

pub(crate) fn recorded_payload(payload: &Value) -> Value {
    if payload["op"] == "auth" {
        json!({"op": "auth"})
    } else {
        payload.clone()
    }
}

pub(crate) fn mock_replies(payload: &Value) -> Vec<Value> {
    match payload["op"].as_str() {
        Some("auth") => vec![json!({"op": "authed", "rid": payload["rid"]})],
        Some(op @ ("subscribe" | "unsubscribe")) => payload["subscriptions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|subscription| {
                json!({
                    "op": format!("{op}d"),
                    "channel": subscription["channel"],
                    "rid": payload["rid"],
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn crypto_data_type(symbol: &str) -> DataType {
    let metadata = Params::new([("symbol".to_string(), json!(symbol))].into());
    DataType::new("PolymarketCryptoPrice", Some(metadata), None)
}

pub(crate) fn equity_data_type(symbol: &str) -> DataType {
    let metadata = Params::new([("symbol".to_string(), json!(symbol))].into());
    DataType::new("PolymarketEquityPrice", Some(metadata), None)
}

pub(crate) fn crypto_twap_data_type(symbol: &str, window_seconds: u64) -> DataType {
    let metadata = Params::new(
        [
            ("symbol".to_string(), json!(symbol)),
            ("window_seconds".to_string(), json!(window_seconds)),
        ]
        .into(),
    );
    DataType::new("PolymarketCryptoTwap", Some(metadata), None)
}

pub(crate) fn price_timestamp_ms(sequence: u64, point: usize) -> u64 {
    1791355667000 + sequence * 10 + point as u64
}

pub(crate) fn fixture_json(fixture: &str) -> Value {
    serde_json::from_str(fixture).expect("parse fixture")
}

pub(crate) fn subscription_pairs(payload: &Value) -> Vec<(String, String)> {
    let mut pairs = payload["subscriptions"]
        .as_array()
        .expect("subscriptions array")
        .iter()
        .map(|subscription| {
            (
                subscription["channel"].as_str().unwrap().to_string(),
                subscription["filter"]["symbol"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();

    pairs.sort();
    pairs
}

pub(crate) fn ops(payloads: &[Value]) -> Vec<&str> {
    payloads
        .iter()
        .map(|payload| payload["op"].as_str().unwrap_or("?"))
        .collect()
}

pub(crate) async fn wait_for_op(state: &TestServerState, op: &'static str) {
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .received_payloads
                    .lock()
                    .await
                    .iter()
                    .any(|payload| payload["op"] == op)
            }
        },
        Duration::from_secs(5),
    )
    .await;
}
