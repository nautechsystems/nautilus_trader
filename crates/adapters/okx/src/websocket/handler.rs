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

//! WebSocket message handler for OKX.
//!
//! The handler is a thin I/O boundary between the network layer and the client. It owns the
//! `WebSocketClient`, deserializes raw venue messages into `OKXWsMessage` events, and handles
//! subscription management, authentication, and retry logic.
//!
//! All domain parsing (venue types to Nautilus types) occurs outside the handler:
//! - Data parsing in `PyOKXWebSocketClient` (uses an instruments cache)
//! - Execution parsing in `execution.rs` (uses the system Cache)

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nautilus_model::identifiers::ClientOrderId;
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, TEXT_PING, TEXT_PONG, WebSocketClient},
};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    enums::{OKXSubscriptionEvent, OKXWsChannel, OKXWsOperation},
    error::OKXWsError,
    messages::{
        OKXOrderMsg, OKXSubscription, OKXSubscriptionArg, OKXWebSocketArg, OKXWebSocketError,
        OKXWsFrame, OKXWsMessage,
    },
    subscription::topic_from_websocket_arg,
};
use crate::{
    common::consts::{OKX_FIELD_SMSG, OKX_SUCCESS_CODE, should_retry_error_code},
    websocket::client::OKX_RATE_LIMIT_KEY_SUBSCRIPTION,
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum HandlerCommand {
    /// Set the WebSocketClient for the handler to use.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Send authentication payload to the WebSocket.
    Authenticate { payload: String },
    /// Subscribe to the given channels.
    Subscribe { args: Vec<OKXSubscriptionArg> },
    /// Unsubscribe from the given channels.
    Unsubscribe { args: Vec<OKXSubscriptionArg> },
    /// Send a pre-serialized payload (used for order operations).
    Send {
        payload: String,
        rate_limit_keys: Option<Vec<Ustr>>,
        request_id: Option<String>,
        client_order_id: Option<ClientOrderId>,
        op: Option<OKXWsOperation>,
    },
}

pub(super) struct OKXWsFeedHandler {
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<OKXWsMessage>,
    auth_tracker: AuthTracker,
    subscriptions_state: SubscriptionState,
    retry_manager: RetryManager<OKXWsError>,
    pending_messages: VecDeque<OKXWsMessage>,
}

impl OKXWsFeedHandler {
    /// Creates a new [`OKXWsFeedHandler`] instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<OKXWsMessage>,
        auth_tracker: AuthTracker,
        subscriptions_state: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            out_tx,
            auth_tracker,
            subscriptions_state,
            retry_manager: create_websocket_retry_manager(),
            pending_messages: VecDeque::new(),
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Acquire)
    }

    pub(super) fn send(&self, msg: OKXWsMessage) -> Result<(), ()> {
        self.out_tx.send(msg).map_err(|_| ())
    }

    async fn send_with_retry(
        &self,
        payload: String,
        rate_limit_keys: Option<&[Ustr]>,
    ) -> Result<(), OKXWsError> {
        if let Some(client) = &self.inner {
            let keys_owned: Option<Vec<Ustr>> = rate_limit_keys.map(|k| k.to_vec());
            self.retry_manager
                .execute_with_retry(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        let keys = keys_owned.clone();
                        async move {
                            client
                                .send_text(payload, keys.as_deref())
                                .await
                                .map_err(|e| OKXWsError::ClientError(format!("Send failed: {e}")))
                        }
                    },
                    should_retry_okx_error,
                    create_okx_timeout_error,
                )
                .await
        } else {
            Err(OKXWsError::ClientError(
                "No active WebSocket client".to_string(),
            ))
        }
    }

    pub(super) async fn send_pong(&self) -> anyhow::Result<()> {
        match self.send_with_retry(TEXT_PONG.to_string(), None).await {
            Ok(()) => {
                log::trace!("Sent pong response to OKX text ping");
                Ok(())
            }
            Err(e) => {
                log::warn!("Failed to send pong after retries: error={e}");
                Err(anyhow::anyhow!("Failed to send pong: {e}"))
            }
        }
    }

    pub(super) async fn next(&mut self) -> Option<OKXWsMessage> {
        if let Some(message) = self.pending_messages.pop_front() {
            return Some(message);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("Handler received WebSocket client");
                            self.inner = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Handler disconnecting WebSocket client");
                            self.inner = None;
                            return None;
                        }
                        HandlerCommand::Authenticate { payload } => {
                            if let Err(e) = self.send_with_retry(
                                payload,
                                Some(OKX_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()),
                            ).await {
                                log::error!(
                                    "Failed to send authentication message after retries: error={e}"
                                );
                            }
                        }
                        HandlerCommand::Subscribe { args } => {
                            if let Err(e) = self.handle_subscribe(args).await {
                                log::error!("Failed to handle subscribe command: error={e}");
                            }
                        }
                        HandlerCommand::Unsubscribe { args } => {
                            if let Err(e) = self.handle_unsubscribe(args).await {
                                log::error!("Failed to handle unsubscribe command: error={e}");
                            }
                        }
                        HandlerCommand::Send {
                            payload,
                            rate_limit_keys,
                            request_id,
                            client_order_id,
                            op,
                        } => {
                            if let Err(e) = self.send_with_retry(
                                payload,
                                rate_limit_keys.as_deref(),
                            ).await {
                                log::error!("Failed to send message after retries: error={e}");

                                if let Some(request_id) = request_id {
                                    self.pending_messages.push_back(OKXWsMessage::SendFailed {
                                        request_id,
                                        client_order_id,
                                        op,
                                        error: format!("{e}"),
                                    });
                                }
                            }
                        }
                    }
                }

                () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Acquire) {
                        log::debug!("Stop signal received during idle period");
                        return None;
                    }
                }

                msg = self.raw_rx.recv() => {
                    let event = match msg {
                        Some(msg) => match Self::parse_raw_message(msg) {
                            Some(event) => event,
                            None => continue,
                        },
                        None => {
                            log::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    match event {
                        OKXWsFrame::Ping => {
                            if let Err(e) = self.send_pong().await {
                                log::warn!("Failed to send pong response: error={e}");
                            }
                        }
                        OKXWsFrame::Login {
                            code, msg, conn_id, ..
                        } => {
                            if code == OKX_SUCCESS_CODE {
                                self.auth_tracker.succeed();
                                return Some(OKXWsMessage::Authenticated);
                            }

                            log::error!("WebSocket authentication failed: error={msg}");
                            self.auth_tracker.fail(msg.clone());

                            let error = OKXWebSocketError {
                                code,
                                message: msg,
                                conn_id: Some(conn_id),
                                timestamp: nautilus_core::time::get_atomic_clock_realtime()
                                    .get_time_ns()
                                    .as_u64(),
                            };
                            self.pending_messages.push_back(OKXWsMessage::Error(error));
                        }
                        OKXWsFrame::BookData { arg, action, data } => {
                            return Some(OKXWsMessage::BookData { arg, action, data });
                        }
                        OKXWsFrame::OrderResponse {
                            id, op, code, msg, data,
                        } => {
                            return Some(OKXWsMessage::OrderResponse {
                                id, op, code, msg, data,
                            });
                        }
                        OKXWsFrame::Data { arg, data } => {
                            if let Some(output) = self.route_data_message(arg, data) {
                                return Some(output);
                            }
                        }
                        OKXWsFrame::Error { code, msg } => {
                            let error = OKXWebSocketError {
                                code,
                                message: msg,
                                conn_id: None,
                                timestamp: nautilus_core::time::get_atomic_clock_realtime()
                                    .get_time_ns()
                                    .as_u64(),
                            };
                            return Some(OKXWsMessage::Error(error));
                        }
                        OKXWsFrame::Reconnected => {
                            self.auth_tracker.invalidate();
                            return Some(OKXWsMessage::Reconnected);
                        }
                        OKXWsFrame::Subscription {
                            event, arg, code, msg, ..
                        } => {
                            self.handle_subscription_ack(&event, &arg, code.as_deref(), msg.as_deref());
                        }
                        OKXWsFrame::ChannelConnCount { .. } => {}
                    }
                }

                else => {
                    log::debug!("Handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    fn route_data_message(&self, arg: OKXWebSocketArg, data: Value) -> Option<OKXWsMessage> {
        let OKXWebSocketArg {
            channel, inst_id, ..
        } = arg;

        match channel {
            OKXWsChannel::Account => Some(OKXWsMessage::Account(data)),
            OKXWsChannel::Positions => Some(OKXWsMessage::Positions(data)),
            OKXWsChannel::Orders => {
                parse_array_items(data, "orders", false).map(OKXWsMessage::Orders)
            }
            OKXWsChannel::SprdOrders => {
                parse_array_items(data, "spread orders", false).map(OKXWsMessage::SpreadOrders)
            }
            OKXWsChannel::OrdersAlgo | OKXWsChannel::AlgoAdvance => {
                parse_array_items(data, "algo orders", false).map(OKXWsMessage::AlgoOrders)
            }
            OKXWsChannel::Instruments => {
                parse_array_items(data, "instruments", true).map(OKXWsMessage::Instruments)
            }
            _ => Some(OKXWsMessage::ChannelData {
                channel,
                inst_id,
                data,
            }),
        }
    }

    fn handle_subscription_ack(
        &self,
        event: &OKXSubscriptionEvent,
        arg: &OKXWebSocketArg,
        code: Option<&str>,
        msg: Option<&str>,
    ) {
        let topic = topic_from_websocket_arg(arg);
        let success = code.is_none_or(|c| c == OKX_SUCCESS_CODE);

        match event {
            OKXSubscriptionEvent::Subscribe => {
                if success {
                    self.subscriptions_state.confirm_subscribe(&topic);
                } else {
                    log::warn!(
                        "Subscription failed: topic={topic:?}, error={msg:?}, code={code:?}"
                    );
                    self.subscriptions_state.mark_failure(&topic);
                }
            }
            OKXSubscriptionEvent::Unsubscribe => {
                if success {
                    self.subscriptions_state.confirm_unsubscribe(&topic);
                } else {
                    log::warn!(
                        "Unsubscription failed - restoring subscription: \
                         topic={topic:?}, error={msg:?}, code={code:?}"
                    );
                    self.subscriptions_state.confirm_unsubscribe(&topic);
                    self.subscriptions_state.mark_subscribe(&topic);
                    self.subscriptions_state.confirm_subscribe(&topic);
                }
            }
        }
    }

    async fn handle_subscribe(&self, args: Vec<OKXSubscriptionArg>) -> anyhow::Result<()> {
        for arg in &args {
            log::debug!(
                "Subscribing to channel: channel={:?}, inst_id={:?}",
                arg.channel,
                arg.inst_id
            );
        }

        let message = OKXSubscription {
            op: OKXWsOperation::Subscribe,
            args,
        };

        let json_txt = serde_json::to_string(&message)
            .map_err(|e| anyhow::anyhow!("Failed to serialize subscription: {e}"))?;

        self.send_with_retry(json_txt, Some(OKX_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send subscription after retries: {e}"))?;
        Ok(())
    }

    async fn handle_unsubscribe(&self, args: Vec<OKXSubscriptionArg>) -> anyhow::Result<()> {
        for arg in &args {
            log::debug!(
                "Unsubscribing from channel: channel={:?}, inst_id={:?}",
                arg.channel,
                arg.inst_id
            );
        }

        let message = OKXSubscription {
            op: OKXWsOperation::Unsubscribe,
            args,
        };

        let json_txt = serde_json::to_string(&message)
            .map_err(|e| anyhow::anyhow!("Failed to serialize unsubscription: {e}"))?;

        self.send_with_retry(json_txt, Some(OKX_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send unsubscription after retries: {e}"))?;
        Ok(())
    }

    pub(crate) fn parse_raw_message(
        msg: tokio_tungstenite::tungstenite::Message,
    ) -> Option<OKXWsFrame> {
        match msg {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                if text == TEXT_PONG {
                    log::trace!("Received pong from OKX");
                    return None;
                }

                if text == TEXT_PING {
                    log::trace!("Received ping from OKX (text)");
                    return Some(OKXWsFrame::Ping);
                }

                if text == RECONNECTED {
                    log::debug!("Received WebSocket reconnection signal");
                    return Some(OKXWsFrame::Reconnected);
                }
                log::trace!("Received WebSocket message: {text}");

                match serde_json::from_str(&text) {
                    Ok(ws_event) => match &ws_event {
                        OKXWsFrame::Error { code, msg } => {
                            log::error!("WebSocket error: {code} - {msg}");
                            Some(ws_event)
                        }
                        OKXWsFrame::Login {
                            event,
                            code,
                            msg,
                            conn_id,
                        } => {
                            if code == OKX_SUCCESS_CODE {
                                log::info!("WebSocket authenticated: conn_id={conn_id}");
                            } else {
                                log::error!(
                                    "WebSocket authentication failed: \
                                     event={event}, code={code}, error={msg}"
                                );
                            }
                            Some(ws_event)
                        }
                        OKXWsFrame::Subscription {
                            event,
                            arg,
                            conn_id,
                            ..
                        } => {
                            let channel_str = serde_json::to_string(&arg.channel)
                                .expect("Invalid OKX websocket channel")
                                .trim_matches('"')
                                .to_string();
                            log::debug!("{event}d: channel={channel_str}, conn_id={conn_id}");
                            Some(ws_event)
                        }
                        OKXWsFrame::ChannelConnCount {
                            channel,
                            conn_count,
                            conn_id,
                            ..
                        } => {
                            let channel_str = serde_json::to_string(channel)
                                .expect("Invalid OKX websocket channel")
                                .trim_matches('"')
                                .to_string();
                            log::debug!(
                                "Channel connection status: \
                                 channel={channel_str}, connections={conn_count}, conn_id={conn_id}",
                            );
                            None
                        }
                        OKXWsFrame::Ping => {
                            log::trace!("Ignoring ping event parsed from text payload");
                            None
                        }
                        OKXWsFrame::Data { .. } | OKXWsFrame::BookData { .. } => Some(ws_event),
                        OKXWsFrame::OrderResponse {
                            id, op, code, data, ..
                        } => {
                            if code == OKX_SUCCESS_CODE {
                                log::debug!(
                                    "Order operation successful: id={id:?}, op={op}, code={code}"
                                );

                                if let Some(order_data) = data.first() {
                                    let success_msg = order_data
                                        .get(OKX_FIELD_SMSG)
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("Order operation successful");
                                    log::debug!("Order success details: {success_msg}");
                                }
                            }
                            Some(ws_event)
                        }
                        OKXWsFrame::Reconnected => {
                            log::warn!("Unexpected Reconnected event from deserialization");
                            None
                        }
                    },
                    Err(e) => {
                        log::error!("Failed to parse message: {e}: {text}");
                        None
                    }
                }
            }
            Message::Ping(_payload) => {
                log::trace!("Received binary ping frame from OKX");
                Some(OKXWsFrame::Ping)
            }
            Message::Pong(payload) => {
                log::trace!("Received pong frame from OKX ({} bytes)", payload.len());
                None
            }
            Message::Binary(msg) => {
                log::debug!("Raw binary frame ({} bytes)", msg.len());
                log::trace!("Raw binary: {msg:?}");
                None
            }
            Message::Close(_) => {
                log::debug!("Received close message");
                None
            }
            msg => {
                log::warn!("Unexpected message: {msg}");
                None
            }
        }
    }
}

/// Returns `true` when an OKX WebSocket order message represents a post-only auto-cancel.
pub fn is_post_only_auto_cancel(msg: &OKXOrderMsg) -> bool {
    use crate::common::{consts::OKX_POST_ONLY_CANCEL_SOURCE, enums::OKXOrderStatus};

    if msg.state != OKXOrderStatus::Canceled {
        return false;
    }

    let cancel_source_matches = matches!(
        msg.cancel_source.as_deref(),
        Some(source) if source == OKX_POST_ONLY_CANCEL_SOURCE
    );

    let reason_matches = matches!(
        msg.cancel_source_reason.as_deref(),
        Some(reason) if reason.contains("POST_ONLY")
    );

    if !(cancel_source_matches || reason_matches) {
        return false;
    }

    msg.acc_fill_sz
        .as_ref()
        .is_none_or(|filled| filled == "0" || filled.is_empty())
}

// Per-item deserialization so one malformed entry does not drop the batch.
fn parse_array_items<T: serde::de::DeserializeOwned>(
    data: Value,
    label: &str,
    warn_on_parse_error: bool,
) -> Option<Vec<T>> {
    let Value::Array(items) = data else {
        if warn_on_parse_error {
            log::warn!("Expected {label} payload to be a JSON array");
        } else {
            log::error!("Expected {label} payload to be a JSON array");
        }
        return None;
    };

    let mut parsed = Vec::with_capacity(items.len());
    for (idx, item) in items.into_iter().enumerate() {
        match serde_json::from_value::<T>(item) {
            Ok(value) => parsed.push(value),
            Err(e) => {
                if warn_on_parse_error {
                    log::warn!("Failed to parse {label} item at index {idx}: {e}");
                } else {
                    log::error!("Failed to parse {label} item at index {idx}: {e}");
                }
            }
        }
    }

    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

#[inline]
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

// Specific phrases rather than bare "connection"/"network", which appear
// in permanent errors too (e.g. "no active WebSocket client connection").
const RETRYABLE_CLIENT_ERROR_PHRASES: &[&str] = &[
    "timeout",
    "timed out",
    "connection reset",
    "connection refused",
    "connection closed",
    "connection aborted",
    "broken pipe",
    "network unreachable",
    "network is unreachable",
    "no route to host",
];

fn should_retry_okx_error(error: &OKXWsError) -> bool {
    match error {
        OKXWsError::OkxError { error_code, .. } => should_retry_error_code(error_code),
        OKXWsError::TungsteniteError(_) => true,
        OKXWsError::ClientError(msg) => RETRYABLE_CLIENT_ERROR_PHRASES
            .iter()
            .any(|phrase| contains_ignore_ascii_case(msg, phrase)),
        OKXWsError::AuthenticationError(_)
        | OKXWsError::JsonError(_)
        | OKXWsError::ParsingError(_) => false,
    }
}

fn create_okx_timeout_error(msg: String) -> OKXWsError {
    OKXWsError::ClientError(msg)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use nautilus_network::websocket::{AuthTracker, SubscriptionState};
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::common::{consts::OKX_WS_TOPIC_DELIMITER, testing::load_test_json};

    fn create_handler() -> OKXWsFeedHandler {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();

        OKXWsFeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
        )
    }

    #[rstest]
    #[case("Connection reset by peer", true)]
    #[case("send timeout after 30s", true)]
    #[case("Connection closed unexpectedly", true)]
    #[case("Broken pipe", true)]
    #[case("Network unreachable", true)]
    #[case("No active WebSocket client connection", false)]
    #[case("network protocol upgrade required", false)]
    #[case("invalid frame format", false)]
    fn test_should_retry_client_error(#[case] msg: &str, #[case] expected: bool) {
        let err = OKXWsError::ClientError(msg.to_string());
        assert_eq!(should_retry_okx_error(&err), expected);
    }

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct ParseArrayItem {
        value: i64,
    }

    #[rstest]
    fn test_parse_array_items_keeps_good_items_when_one_fails() {
        let data = json!([
            {"value": 1},
            {"value": "not a number"},
            {"value": 3},
        ]);

        let parsed: Vec<ParseArrayItem> =
            parse_array_items(data, "test", false).expect("non-empty");
        assert_eq!(
            parsed,
            vec![ParseArrayItem { value: 1 }, ParseArrayItem { value: 3 }],
        );
    }

    #[rstest]
    fn test_parse_array_items_returns_none_when_payload_not_array() {
        let data = json!({"not": "an array"});
        let parsed: Option<Vec<ParseArrayItem>> = parse_array_items(data, "test", false);
        assert!(parsed.is_none());
    }

    #[rstest]
    fn test_parse_array_items_returns_none_when_all_items_fail() {
        let data = json!([{"value": "bad"}]);
        let parsed: Option<Vec<ParseArrayItem>> = parse_array_items(data, "test", false);
        assert!(parsed.is_none());
    }

    #[rstest]
    fn test_route_instruments_keeps_valid_items_when_one_item_fails() {
        let handler = create_handler();
        let mut frame: Value =
            serde_json::from_str(&load_test_json("ws_instruments.json")).expect("valid fixture");
        let data = frame
            .get_mut("data")
            .and_then(Value::as_array_mut)
            .expect("data array");
        let mut invalid_item = data[0].clone();
        invalid_item["tickSz"] = json!(7);
        data.insert(0, invalid_item);

        let arg: OKXWebSocketArg = serde_json::from_value(frame["arg"].clone()).expect("valid arg");
        let msg = handler
            .route_data_message(arg, frame["data"].clone())
            .expect("instruments message");

        match msg {
            OKXWsMessage::Instruments(instruments) => {
                assert_eq!(instruments.len(), 1);
                assert_eq!(instruments[0].inst_id.as_str(), "BTC-USDT-SWAP");
            }
            other => panic!("Expected Instruments, was {other:?}"),
        }
    }
}
