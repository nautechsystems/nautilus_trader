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
        OKXAlgoOrderMsg, OKXOrderMsg, OKXSubscription, OKXSubscriptionArg, OKXWebSocketArg,
        OKXWebSocketError, OKXWsFrame, OKXWsMessage,
    },
    subscription::topic_from_websocket_arg,
};
use crate::{
    common::{
        consts::{OKX_FIELD_SCODE, OKX_FIELD_SMSG, OKX_SUCCESS_CODE, should_retry_error_code},
        models::OKXInstrument,
    },
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
    pub fn new(
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
            OKXWsChannel::Orders => match serde_json::from_value::<Vec<OKXOrderMsg>>(data) {
                Ok(orders) => Some(OKXWsMessage::Orders(orders)),
                Err(e) => {
                    log::error!("Failed to parse orders data: {e}");
                    None
                }
            },
            OKXWsChannel::OrdersAlgo | OKXWsChannel::AlgoAdvance => {
                match serde_json::from_value::<Vec<OKXAlgoOrderMsg>>(data) {
                    Ok(orders) => Some(OKXWsMessage::AlgoOrders(orders)),
                    Err(e) => {
                        log::error!("Failed to parse algo orders data: {e}");
                        None
                    }
                }
            }
            OKXWsChannel::Instruments => match serde_json::from_value::<Vec<OKXInstrument>>(data) {
                Ok(instruments) => Some(OKXWsMessage::Instruments(instruments)),
                Err(e) => {
                    log::error!("Failed to parse instruments data: {e}");
                    None
                }
            },
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
                log::debug!("Raw binary: {msg:?}");
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

/// Returns `true` when an OKX WebSocket error payload represents a post-only rejection.
pub fn is_post_only_rejection(code: &str, data: &[Value]) -> bool {
    use crate::common::consts::OKX_POST_ONLY_ERROR_CODE;

    if code == OKX_POST_ONLY_ERROR_CODE {
        return true;
    }

    for entry in data {
        if let Some(s_code) = entry.get(OKX_FIELD_SCODE).and_then(|value| value.as_str())
            && s_code == OKX_POST_ONLY_ERROR_CODE
        {
            return true;
        }

        if let Some(inner_code) = entry.get("code").and_then(|value| value.as_str())
            && inner_code == OKX_POST_ONLY_ERROR_CODE
        {
            return true;
        }
    }

    false
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

#[inline]
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn should_retry_okx_error(error: &OKXWsError) -> bool {
    match error {
        OKXWsError::OkxError { error_code, .. } => should_retry_error_code(error_code),
        OKXWsError::TungsteniteError(_) => true,
        OKXWsError::ClientError(msg) => {
            contains_ignore_ascii_case(msg, "timeout")
                || contains_ignore_ascii_case(msg, "timed out")
                || contains_ignore_ascii_case(msg, "connection")
                || contains_ignore_ascii_case(msg, "network")
        }
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
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_is_post_only_rejection_detects_by_code() {
        assert!(is_post_only_rejection("51019", &[]));
    }

    #[rstest]
    fn test_is_post_only_rejection_detects_by_inner_code() {
        let data = vec![json!({ "sCode": "51019" })];
        assert!(is_post_only_rejection("50000", &data));
    }

    #[rstest]
    fn test_is_post_only_rejection_false_for_unrelated_error() {
        let data = vec![json!({ "sMsg": "Insufficient balance" })];
        assert!(!is_post_only_rejection("50000", &data));
    }
}
