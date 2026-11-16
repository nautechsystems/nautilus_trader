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

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use futures_util::{SinkExt, StreamExt};
use nautilus_model::identifiers::InstrumentId;
use serde_json::Value;
use tokio::{io::AsyncWriteExt, net::TcpListener};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::{KalshiDataClientConfig, http_request::read_headers, keys};

pub(super) struct Peer {
    pub(super) config: KalshiDataClientConfig,
    pub(super) revision: Arc<AtomicUsize>,
    pub(super) market: Arc<Mutex<Option<Value>>>,
    pub(super) requests: Arc<AtomicUsize>,
    pub(super) request_paths: Arc<Mutex<Vec<String>>>,
    pub(super) confirm: Arc<AtomicBool>,
    pub(super) early: Arc<AtomicBool>,
    pub(super) actions: tokio::sync::mpsc::UnboundedSender<(String, Value)>,
    pub(super) commands: tokio::sync::mpsc::UnboundedReceiver<Value>,
    pub(super) release: Arc<tokio::sync::Notify>,
    requested: Arc<tokio::sync::Notify>,
    pub(super) response_gate: Arc<tokio::sync::Semaphore>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Peer {
    pub(super) async fn new() -> Self {
        let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let websocket = TcpListener::bind("127.0.0.1:0").await.unwrap();

        let config = KalshiDataClientConfig {
            api_key_id: Some("data-test-key".into()),
            private_key: Some(keys::ed25519_pkcs8().into()),
            base_url_http: Some(
                format!("http://{}/trade-api/v2", http.local_addr().unwrap()).into(),
            ),
            base_url_ws: Some(
                format!("ws://{}/trade-api/ws/v2", websocket.local_addr().unwrap()).into(),
            ),
            ws_timeout_secs: 2,
            http_timeout_secs: 2,
            ..Default::default()
        };

        let revision = Arc::new(AtomicUsize::new(0));
        let market = Arc::new(Mutex::new(None::<Value>));
        let definition = Arc::clone(&market);
        let requests = Arc::new(AtomicUsize::new(0));
        let request_paths = Arc::new(Mutex::new(Vec::new()));
        let paths = Arc::clone(&request_paths);
        let version = Arc::clone(&revision);
        let count = Arc::clone(&requests);
        let release = Arc::new(tokio::sync::Notify::new());
        let response_release = Arc::clone(&release);
        let requested = Arc::new(tokio::sync::Notify::new());
        let request_observed = Arc::clone(&requested);

        let response_gate = Arc::new(tokio::sync::Semaphore::new(
            tokio::sync::Semaphore::MAX_PERMITS,
        ));

        let responses = Arc::clone(&response_gate);

        let http_task = tokio::spawn(async move {
            let mut pending = tokio::task::JoinSet::new();

            loop {
                tokio::select! {
                    connection = http.accept() => {
                        let (mut stream, _) = connection.unwrap();
                        let paths = Arc::clone(&paths);
                        let definition = Arc::clone(&definition);
                        let version = Arc::clone(&version);
                        let count = Arc::clone(&count);
                        let response_release = Arc::clone(&response_release);
                        let request_observed = Arc::clone(&request_observed);
                        let responses = Arc::clone(&responses);

                        pending.spawn(async move {
                            let request = read_headers(&mut stream).await;
                            let held = response_release.notified();
                            tokio::pin!(held);
                            held.as_mut().enable();
                            let block = version.load(Ordering::SeqCst) == 100;
                            count.fetch_add(1, Ordering::SeqCst);
                            request_observed.notify_one();

                            if block {
                                held.await;
                            }
                            responses.acquire().await.unwrap().forget();

                            let path = request
                                .lines()
                                .next()
                                .unwrap()
                                .split_whitespace()
                                .nth(1)
                                .unwrap();
                            paths.lock().unwrap().push(path.to_owned());
                            let fixtures: Value =
                                serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
                            let mut market = definition.lock().unwrap().clone().unwrap_or_else(|| fixtures["definitions"]["live"]["market"].clone());
                            market["yes_sub_title"] = format!("revision-{}", version.load(Ordering::SeqCst)).into();
                            let mut event = fixtures["definitions"]["live"]["event"].clone();
                            if version.load(Ordering::SeqCst) == 99 {
                                event["collateral_return_type"] = "DIRECNET".into();
                            }

                            let body = if path.contains("/orderbook?") {
                                let fixture: Value =
                                    serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();
                                serde_json::json!({"orderbook_fp":fixture["rest"]})
                            } else if path.contains("/markets?") {
                                let mut second = market.clone();
                                second["ticker"] = "MARKET-B".into();
                                serde_json::json!({"markets": [market, second], "cursor": ""})
                            } else if path.contains("/markets/") {
                                market["ticker"] = path.rsplit('/').next().unwrap().into();
                                serde_json::json!({"market": market})
                            } else if path.contains("/events/") {
                                serde_json::json!({"event": event, "markets": []})
                            } else if path.contains("/series/") {
                                fixtures["series"].clone()
                            } else {
                                panic!("unexpected path {path}");
                            };

                            let body = body.to_string();
                            let _ = stream
                                .write_all(
                                    format!(
                                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                        body.len()
                                    )
                                    .as_bytes(),
                                )
                                .await;
                        });
                    },
                    Some(result) = pending.join_next(), if !pending.is_empty() => { result.unwrap(); },
                }
            }
        });

        let (actions, mut control) = tokio::sync::mpsc::unbounded_channel::<(String, Value)>();
        let (commands_tx, commands) = tokio::sync::mpsc::unbounded_channel();
        let confirm = Arc::new(AtomicBool::new(true));
        let confirm_subscriptions = Arc::clone(&confirm);
        let early = Arc::new(AtomicBool::new(false));
        let early_frame = Arc::clone(&early);

        let ws_task = tokio::spawn(async move {
            loop {
                let (stream, _) = websocket.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                let mut subscriptions = HashMap::new();
                let mut next_sid = 1_u64;
                let mut sequences = HashMap::<u64, u64>::new();

                loop {
                    tokio::select! {
                        connection = websocket.accept() => {
                            let (stream, _) = connection.unwrap();
                            ws_client = accept_async(stream).await.unwrap();
                            subscriptions.clear();
                            next_sid = 1;
                            sequences.clear();
                        },
                        frame = ws_client.next() => {
                            let Some(Ok(Message::Text(frame))) = frame else {
                                break;
                            };
                            let command: Value = serde_json::from_str(&frame).unwrap();
                            commands_tx.send(command.clone()).unwrap();

                            if !confirm_subscriptions.load(Ordering::SeqCst) {
                                continue;
                            }
                            let response = match command["cmd"].as_str().unwrap() {
                                "subscribe" => {
                                    let channel = command["params"]["channels"][0].as_str().unwrap();
                                    let ticker = command["params"]["market_tickers"][0].as_str().unwrap_or("");
                                    let sid = next_sid;
                                    next_sid += 1;
                                    subscriptions.insert(format!("{channel}:{ticker}"), sid);
                                    serde_json::json!({"type":"subscribed", "id":command["id"], "msg":{"channel":channel,"sid":sid}})
                                }
                                "unsubscribe" => {
                                    let sid = command["params"]["sids"][0].as_u64().unwrap();
                                    let seq = sequences.entry(sid).or_default();
                                    *seq += 1;
                                    serde_json::json!({"type":"unsubscribed","id":command["id"],"sid":sid,"seq":seq})
                                }
                                "update_subscription" => {
                                    let sid = command["params"]["sid"].as_u64().unwrap();
                                    let fixtures: Value =
                                        serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();
                                    for ticker in command["params"]["market_tickers"].as_array().unwrap() {
                                        let seq = sequences.entry(sid).or_default();
                                        *seq += 1;
                                        let mut payload = fixtures["snapshot"].clone();
                                        payload["market_ticker"] = ticker.clone();
                                        let response = serde_json::json!({
                                            "type": "orderbook_snapshot",
                                            "sid": sid,
                                            "seq": seq,
                                            "id": command["id"],
                                            "msg": payload
                                        });
                                        ws_client
                                            .send(Message::text(response.to_string()))
                                            .await
                                            .unwrap();
                                    }
                                    continue;
                                }
                                name => panic!("unexpected command {name}"),
                            };

                            if ws_client
                                .send(Message::Text(response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }

                            if command["cmd"] == "subscribe" && command["params"]["channels"][0] == "orderbook_delta" {
                                let sid = response["msg"]["sid"].as_u64().unwrap();
                                let fixture: Value =
                                    serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();
                                for ticker in command["params"]["market_tickers"].as_array().unwrap() {
                                    let seq = sequences.entry(sid).or_default();
                                    *seq += 1;
                                    let mut payload = fixture["snapshot"].clone();
                                    payload["market_ticker"] = ticker.clone();
                                    let frame = serde_json::json!({"type":"orderbook_snapshot","sid":sid,"seq":seq,"msg":payload});
                                    ws_client
                                        .send(Message::text(frame.to_string()))
                                        .await
                                        .unwrap();
                                }
                            }

                            if command["cmd"] == "subscribe" && command["params"]["channels"][0] != "market_lifecycle_v2" && early_frame.swap(false, Ordering::SeqCst) {
                                let fixtures: Value =
                                    serde_json::from_str(include_str!("../../test_data/market_data.json")).unwrap();
                                let channel = command["params"]["channels"][0].as_str().unwrap();
                                let mut payload = fixtures[channel].clone();
                                payload["market_ticker"] = command["params"]["market_tickers"][0].clone();
                                let frame = serde_json::json!({"type":channel,"sid":response["msg"]["sid"],"seq":1,"msg":payload});
                                ws_client
                                    .send(Message::Text(frame.to_string().into()))
                                    .await
                                    .unwrap();
                            }

                        },
                        action = control.recv() => {
                            let Some((channel, payload)) = action else {
                                return;
                            };

                            if channel == "close" {
                                let _ = ws_client.close(None).await;
                                break;
                            }

                            if channel == "raw" {
                                if let (Some(sid), Some(seq)) = (payload["sid"].as_u64(), payload["seq"].as_u64()) {
                                    let previous = sequences.entry(sid).or_default();
                                    *previous = (*previous).max(seq);
                                }

                                if ws_client
                                    .send(Message::text(payload.to_string()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                            let ticker = payload["market_ticker"].as_str().unwrap();
                            let key = format!("{channel}:{ticker}");
                            let sid = subscriptions
                                .get(&key)
                                .or_else(|| {
                                    subscriptions
                                        .iter()
                                        .find_map(|(key, sid)| key.starts_with(&format!("{channel}:")).then_some(sid))
                                })
                                .copied()
                                .unwrap();
                            let frame = serde_json::json!({"type":channel,"sid":sid,"seq":1,"msg":payload});
                            if ws_client
                                .send(Message::Text(frame.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }

                        }
                    }
                }

                let _ = commands_tx.send(serde_json::json!({"cmd":"closed"}));
            }
        });

        Self {
            config,
            revision,
            market,
            requests,
            request_paths,
            confirm,
            early,
            actions,
            commands,
            release,
            requested,
            response_gate,
            tasks: vec![http_task, ws_task],
        }
    }

    pub(super) fn emit(&self, channel: &str, id: InstrumentId, malformed: bool) {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/market_data.json")).unwrap();
        let mut value = fixtures[channel].clone();
        value["market_ticker"] = id.symbol.as_str().into();
        if malformed {
            value["count_fp"] = "bad-number".into();
        }

        self.actions.send((channel.into(), value)).unwrap();
    }

    pub(super) async fn wait_requests(&self, count: usize) {
        loop {
            let requested = self.requested.notified();
            tokio::pin!(requested);
            requested.as_mut().enable();

            if self.requests.load(Ordering::SeqCst) >= count {
                return;
            }

            requested.await;
        }
    }

    pub(super) async fn command(&mut self) -> Value {
        loop {
            let command = self.commands.recv().await.unwrap();
            if command["cmd"] != "closed" {
                return command;
            }
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
