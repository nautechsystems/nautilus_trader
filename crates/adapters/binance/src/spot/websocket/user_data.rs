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

//! Binance Spot User Data Stream WebSocket client.
//!
//! Connects to the Binance WS API (`wss://ws-api.binance.com`) with a separate
//! JSON-only WebSocket connection for receiving real-time execution events.
//! Authentication uses `userDataStream.subscribe.signature` with HMAC-SHA256.
//!
//! Push events arrive as JSON text frames in the format:
//! ```json
//! {"subscriptionId": 0, "event": {"e": "executionReport", ...}}
//! ```
//!
//! This client does NOT modify the existing SBE trading handler — it operates
//! on a completely independent WebSocket connection.

use std::sync::Arc;

use nautilus_core::time::{AtomicTime, get_atomic_clock_realtime};
use nautilus_network::{
    RECONNECTED,
    websocket::{PingHandler, WebSocketClient, WebSocketConfig, channel_message_handler},
};
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::Message;

use super::{
    messages_exec::BinanceSpotUserDataEvent,
    types_exec::{UserDataStreamEvent, UserDataStreamFrame},
};
use crate::common::credential::Credential;

/// Default WebSocket API URL for Binance Spot User Data Stream.
const DEFAULT_WS_API_URL: &str = "wss://ws-api.binance.com:443/ws-api/v3";

/// Testnet WebSocket API URL for Binance Spot User Data Stream.
const TESTNET_WS_API_URL: &str = "wss://ws-api.testnet.binance.vision/ws-api/v3";

/// Binance Spot User Data Stream WebSocket client.
///
/// Maintains a JSON-only WebSocket connection to the Binance WS API for
/// receiving real-time execution events (fills, cancellations, account updates).
/// Uses `userDataStream.subscribe.signature` for authentication with HMAC-SHA256.
///
/// # Architecture
///
/// ```text
/// BinanceSpotUserDataStream
///   └── WebSocketClient (NT)          ← auto-reconnect with exp backoff
///         └── channel_message_handler  ← raw Message → msg_rx channel
///               └── process_task       ← parse JSON → event_tx channel
///                     └── BinanceSpotExecWsFeedHandler (downstream)
/// ```
///
/// On reconnection, the `__RECONNECTED__` sentinel is detected, the
/// `subscribe.signature` request is re-sent, and a `Reconnected` event
/// is forwarded to the handler for pending request cleanup.
#[derive(Debug)]
pub struct BinanceSpotUserDataStream {
    clock: &'static AtomicTime,
    credential: Arc<Credential>,
    url: String,
    event_tx: UnboundedSender<BinanceSpotUserDataEvent>,
    ws_client: Option<Arc<tokio::sync::Mutex<WebSocketClient>>>,
    process_task: Option<tokio::task::JoinHandle<()>>,
}

impl BinanceSpotUserDataStream {
    /// Creates a new [`BinanceSpotUserDataStream`] instance.
    ///
    /// The client is created in a disconnected state. Call [`connect`](Self::connect)
    /// to establish the WebSocket connection and subscribe to the user data stream.
    #[must_use]
    pub fn new(
        credential: Arc<Credential>,
        event_tx: UnboundedSender<BinanceSpotUserDataEvent>,
        url_override: Option<String>,
        is_testnet: bool,
    ) -> Self {
        let url = url_override.unwrap_or_else(|| {
            if is_testnet {
                TESTNET_WS_API_URL.to_string()
            } else {
                DEFAULT_WS_API_URL.to_string()
            }
        });

        Self {
            clock: get_atomic_clock_realtime(),
            credential,
            url,
            event_tx,
            ws_client: None,
            process_task: None,
        }
    }

    /// Connects to the WebSocket server and subscribes to the user data stream.
    ///
    /// Establishes a JSON-only WebSocket connection with automatic reconnection
    /// (exponential backoff: 500ms→5s, jitter 250ms, unlimited attempts).
    /// After connecting, sends `userDataStream.subscribe.signature` with
    /// HMAC-SHA256 authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - WebSocket connection fails
    /// - Subscribe message cannot be sent
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let (handler, msg_rx) = channel_message_handler();
        let ping_handler: PingHandler = Arc::new(move |_| {});

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: Some(20),
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
        };

        let ws_client = WebSocketClient::connect(
            config,
            Some(handler),
            Some(ping_handler),
            None,
            vec![],
            None,
        )
        .await
        .map_err(|e| anyhow::anyhow!("UDS WebSocket connection failed: {e}"))?;

        let ws_client = Arc::new(tokio::sync::Mutex::new(ws_client));
        self.ws_client = Some(ws_client.clone());

        // Send initial subscribe.signature request
        Self::send_subscribe_msg(&ws_client, &self.credential, self.clock, 1).await?;

        // Spawn the message processing task
        self.spawn_process_task(msg_rx, ws_client);

        log::info!("User data stream connected: url={}", self.url);
        Ok(())
    }

    /// Disconnects from the WebSocket server and cancels processing tasks.
    pub async fn disconnect(&mut self) {
        if let Some(task) = self.process_task.take() {
            task.abort();
            let _ = task.await;
        }

        if let Some(ref ws) = self.ws_client {
            let client = ws.lock().await;
            client.disconnect().await;
        }

        self.ws_client = None;
        log::info!("User data stream disconnected");
    }

    /// Builds and sends a `userDataStream.subscribe.signature` request.
    ///
    /// Constructs an HMAC-SHA256 signed subscription request and sends it
    /// as a JSON text frame on the WebSocket connection.
    async fn send_subscribe_msg(
        ws_client: &Arc<tokio::sync::Mutex<WebSocketClient>>,
        credential: &Arc<Credential>,
        clock: &'static AtomicTime,
        request_id: u64,
    ) -> anyhow::Result<()> {
        let id = format!("uds-sub-{request_id}");
        let timestamp = clock.get_time_ms();
        let api_key = credential.api_key();

        let sign_payload = format!("apiKey={api_key}&timestamp={timestamp}");
        let signature = credential.sign(&sign_payload);

        let subscribe_msg = serde_json::json!({
            "id": id,
            "method": "userDataStream.subscribe.signature",
            "params": {
                "apiKey": api_key,
                "timestamp": timestamp,
                "signature": signature,
            }
        });

        let msg_text = serde_json::to_string(&subscribe_msg)
            .map_err(|e| anyhow::anyhow!("Failed to serialize subscribe message: {e}"))?;

        let client = ws_client.lock().await;
        client
            .send_text(msg_text, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe message: {e}"))?;

        log::info!("Sent userDataStream.subscribe.signature (id={id})");
        Ok(())
    }

    /// Spawns the background task that processes raw WebSocket messages.
    ///
    /// Reads from the raw message channel, detects reconnection sentinels,
    /// re-subscribes via the shared WebSocket client, parses JSON push events,
    /// and forwards them to the exec handler via the event channel.
    fn spawn_process_task(
        &mut self,
        mut msg_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        ws_client: Arc<tokio::sync::Mutex<WebSocketClient>>,
    ) {
        let event_tx = self.event_tx.clone();
        let credential = self.credential.clone();
        let clock = self.clock;
        let url = self.url.clone();
        let mut request_id_counter: u64 = 1;

        let task = tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                let text = match msg {
                    Message::Text(t) => t.to_string(),
                    Message::Binary(b) => {
                        log::debug!(
                            "Unexpected binary frame on UDS connection ({} bytes), skipping",
                            b.len()
                        );
                        continue;
                    }
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                    Message::Close(_) => {
                        log::info!("UDS WebSocket received close frame");
                        break;
                    }
                };

                // Check for reconnection sentinel
                if text == RECONNECTED {
                    log::info!("UDS WebSocket reconnected, re-subscribing...");

                    // Forward reconnected event to handler (drain pending)
                    if event_tx.send(BinanceSpotUserDataEvent::Reconnected).is_err() {
                        log::error!("Event channel closed, stopping UDS process task");
                        break;
                    }

                    // Re-subscribe with retry (3 attempts, 1s delay between retries)
                    request_id_counter += 1;
                    let mut subscribed = false;
                    for attempt in 1..=3u32 {
                        match Self::send_subscribe_msg(
                            &ws_client,
                            &credential,
                            clock,
                            request_id_counter,
                        )
                        .await
                        {
                            Ok(()) => {
                                subscribed = true;
                                break;
                            }
                            Err(e) => {
                                log::error!(
                                    "Re-subscribe attempt {attempt}/3 failed: {e}"
                                );
                                if attempt < 3 {
                                    tokio::time::sleep(
                                        std::time::Duration::from_secs(1),
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                    if !subscribed {
                        log::error!(
                            "All re-subscribe attempts failed after reconnect. \
                             UDS will not receive execution events until next reconnect."
                        );
                    }

                    continue;
                }

                // Parse the JSON message
                Self::dispatch_json_message(&text, &event_tx);
            }

            log::info!("UDS process task ended (url={url})");
        });

        self.process_task = Some(task);
    }

    /// Parses a JSON text frame and dispatches it to the event channel.
    ///
    /// Handles three message formats:
    /// 1. UDS push events: `{"subscriptionId": N, "event": {...}}`
    /// 2. Subscribe response: `{"id": "...", "status": 200, ...}`
    /// 3. Unknown: logged and skipped
    ///
    /// Push events are deserialized in a single pass using the tagged
    /// [`UserDataStreamEvent`] enum (keyed on the `"e"` field), avoiding
    /// the overhead of intermediate `serde_json::Value` allocation.
    fn dispatch_json_message(
        text: &str,
        event_tx: &UnboundedSender<BinanceSpotUserDataEvent>,
    ) {
        // Try to parse as UDS push event frame (single-pass tagged deserialization)
        if let Ok(frame) = serde_json::from_str::<UserDataStreamFrame>(text) {
            match frame.event {
                UserDataStreamEvent::ExecutionReport(report) => {
                    log::debug!(
                        "UDS executionReport: symbol={}, type={:?}, client_order_id={}",
                        report.symbol,
                        report.execution_type,
                        report.client_order_id,
                    );
                    if event_tx
                        .send(BinanceSpotUserDataEvent::ExecutionReport(report))
                        .is_err()
                    {
                        log::error!("Event channel closed");
                    }
                }
                UserDataStreamEvent::AccountPosition(position) => {
                    log::debug!(
                        "UDS outboundAccountPosition: {} balance(s)",
                        position.balances.len(),
                    );
                    if event_tx
                        .send(BinanceSpotUserDataEvent::AccountPosition(position))
                        .is_err()
                    {
                        log::error!("Event channel closed");
                    }
                }
                UserDataStreamEvent::BalanceUpdate(_) => {
                    log::debug!("UDS balanceUpdate received (log only)");
                }
                UserDataStreamEvent::Unknown => {
                    log::debug!("UDS unknown event type");
                }
            }

            return;
        }

        // Try to parse as subscribe response
        if let Ok(response) = serde_json::from_str::<serde_json::Value>(text)
            && let Some(status) = response.get("status").and_then(|v| v.as_u64())
        {
            if status == 200 {
                let id = response
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                log::info!("UDS subscribe response: status=200, id={id}");
            } else {
                log::warn!("UDS subscribe response: status={status}, body={text}");
            }
            return;
        }

        // Unknown message format
        log::debug!(
            "UDS unknown message format (len={}): {}",
            text.len(),
            &text[..text.len().min(200)]
        );
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;
    use crate::spot::websocket::messages_exec::BinanceSpotUserDataEvent;

    /// Creates a test event channel for capturing dispatched events.
    fn test_channel() -> (
        UnboundedSender<BinanceSpotUserDataEvent>,
        mpsc::UnboundedReceiver<BinanceSpotUserDataEvent>,
    ) {
        mpsc::unbounded_channel()
    }

    #[test]
    fn dispatch_execution_report_trade_fill() {
        let (tx, mut rx) = test_channel();

        let json = r#"{"subscriptionId":0,"event":{"e":"executionReport","E":1772494860000,"s":"ETHUSDC","c":"TEST-001","S":"BUY","o":"LIMIT","f":"GTC","q":"0.01000000","p":"2045.50000000","P":"0.00000000","F":"0.00000000","g":-1,"C":"","x":"TRADE","X":"FILLED","r":"NONE","i":9399999776,"l":"0.01000000","z":"0.01000000","L":"2045.50000000","n":"0.00001234","N":"ETH","T":1772494860000,"t":123456789,"w":false,"m":true,"O":1772494856997,"Z":"20.45500000","Y":"20.45500000","Q":"0.00000000","W":1772494856997,"V":"EXPIRE_MAKER"}}"#;

        BinanceSpotUserDataStream::dispatch_json_message(json, &tx);

        let event = rx.try_recv().expect("Expected event");
        match event {
            BinanceSpotUserDataEvent::ExecutionReport(report) => {
                assert_eq!(report.symbol, "ETHUSDC");
                assert_eq!(report.client_order_id, "TEST-001");
                assert_eq!(
                    report.execution_type,
                    crate::spot::websocket::types_exec::BinanceSpotExecutionType::Trade
                );
                assert_eq!(report.trade_id, 123_456_789);
            }
            _ => panic!("Expected ExecutionReport"),
        }
    }

    #[test]
    fn dispatch_execution_report_canceled() {
        let (tx, mut rx) = test_channel();

        let json = r#"{"subscriptionId":0,"event":{"e":"executionReport","E":1772494873278,"s":"ETHUSDC","c":"PAyFKkUBxfnY0fqEogcln5","S":"BUY","o":"LIMIT","f":"GTC","q":"0.01000000","p":"1500.00000000","P":"0.00000000","F":"0.00000000","g":-1,"C":"UDS-TEST-1772494856","x":"CANCELED","X":"CANCELED","r":"NONE","i":9399999776,"l":"0.00000000","z":"0.00000000","L":"0.00000000","n":"0","N":null,"T":1772494873278,"t":-1,"w":false,"m":false,"O":1772494856997,"Z":"0.00000000","Y":"0.00000000","Q":"0.00000000","V":"EXPIRE_MAKER"}}"#;

        BinanceSpotUserDataStream::dispatch_json_message(json, &tx);

        let event = rx.try_recv().expect("Expected event");
        match event {
            BinanceSpotUserDataEvent::ExecutionReport(report) => {
                assert_eq!(
                    report.execution_type,
                    crate::spot::websocket::types_exec::BinanceSpotExecutionType::Canceled
                );
            }
            _ => panic!("Expected ExecutionReport"),
        }
    }

    #[test]
    fn dispatch_account_position_update() {
        let (tx, mut rx) = test_channel();

        let json = r#"{"subscriptionId":0,"event":{"e":"outboundAccountPosition","E":1772494856997,"u":1772494856997,"B":[{"a":"ETH","f":"0.14741228","l":"0.00000000"},{"a":"USDC","f":"886.63366221","l":"15.00000000"}]}}"#;

        BinanceSpotUserDataStream::dispatch_json_message(json, &tx);

        let event = rx.try_recv().expect("Expected event");
        match event {
            BinanceSpotUserDataEvent::AccountPosition(pos) => {
                assert_eq!(pos.balances.len(), 2);
                assert_eq!(pos.balances[0].asset, "ETH");
                assert_eq!(pos.balances[1].asset, "USDC");
            }
            _ => panic!("Expected AccountPosition"),
        }
    }

    #[test]
    fn dispatch_subscribe_response_success() {
        let (tx, mut rx) = test_channel();

        let json = r#"{"id":"uds-sub-1","status":200,"result":{"subscriptionId":0},"rateLimits":[{"rateLimitType":"REQUEST_WEIGHT","interval":"MINUTE","intervalNum":1,"limit":6000,"count":3}]}"#;

        BinanceSpotUserDataStream::dispatch_json_message(json, &tx);

        assert!(
            rx.try_recv().is_err(),
            "Subscribe response should not emit event"
        );
    }

    #[test]
    fn dispatch_balance_update_log_only() {
        let (tx, mut rx) = test_channel();

        let json = r#"{"subscriptionId":0,"event":{"e":"balanceUpdate","E":1772494860000,"a":"USDC","d":"100.00000000","T":1772494860000}}"#;

        BinanceSpotUserDataStream::dispatch_json_message(json, &tx);

        assert!(
            rx.try_recv().is_err(),
            "balanceUpdate should not emit event"
        );
    }

    #[test]
    fn dispatch_unknown_event_type_no_panic() {
        let (tx, mut rx) = test_channel();

        let json = r#"{"subscriptionId":0,"event":{"e":"listStatus","E":1772494860000}}"#;

        BinanceSpotUserDataStream::dispatch_json_message(json, &tx);

        assert!(
            rx.try_recv().is_err(),
            "Unknown event type should not emit event"
        );
    }

    #[test]
    fn dispatch_malformed_json_no_panic() {
        let (tx, mut rx) = test_channel();

        let json = "this is not valid json";

        BinanceSpotUserDataStream::dispatch_json_message(json, &tx);

        assert!(
            rx.try_recv().is_err(),
            "Malformed JSON should not emit event"
        );
    }

    #[test]
    fn dispatch_subscribe_error_response() {
        let (tx, mut rx) = test_channel();

        let json = r#"{"id":"uds-sub-1","status":400,"error":{"code":-1022,"msg":"Signature for this request is not valid."}}"#;

        BinanceSpotUserDataStream::dispatch_json_message(json, &tx);

        assert!(
            rx.try_recv().is_err(),
            "Error response should not emit event"
        );
    }
}
