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
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nautilus_common::live::dst::time;
use nautilus_core::{AtomicTime, string::secret::REDACTED};
pub use nautilus_live::book::snapshot::SnapshotGate;
use nautilus_model::identifiers::ClientOrderId;
use nautilus_network::{
    RECONNECTED,
    error::SendError,
    retry::{RetryError, RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, TEXT_PING, TEXT_PONG, WebSocketClient},
};
use serde_json::{Map, Value};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    enums::{OKXSubscriptionEvent, OKXWsChannel, OKXWsOperation},
    error::OKXWsError,
    messages::{
        OKXOrderMsg, OKXSubscription, OKXSubscriptionArg, OKXWebSocketArg, OKXWebSocketError,
        OKXWsFrame, OKXWsMessage,
    },
    subscription::{topic_from_subscription_arg, topic_from_websocket_arg},
};
use crate::{
    common::{
        consts::{OKX_FIELD_SMSG, OKX_SUCCESS_CODE, should_retry_error_code},
        enums::{OKXOrderStatus, OKXOrderType},
        parse::prefer_rpi_response_fields,
    },
    websocket::client::OKX_RATE_LIMIT_KEY_SUBSCRIPTION,
};

/// Commands sent from the outer client to the inner message handler.
pub enum HandlerCommand {
    /// Set the `WebSocketClient` for the handler to use.
    SetClient(Arc<WebSocketClient>),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Subscribe to the given channels.
    Subscribe { args: Vec<OKXSubscriptionArg> },
    /// Subscribes to a book and reports completion of the transport send.
    SubscribeBook {
        subscription: OKXSubscriptionArg,
        cancel: CancellationToken,
        gate: SnapshotGate,
        completion: tokio::sync::oneshot::Sender<Result<(), OKXWsError>>,
    },
    /// Unsubscribe from the given channels.
    Unsubscribe { args: Vec<OKXSubscriptionArg> },
    /// Replaces a subscription without removing reconnect intent, unless canceled.
    Resubscribe {
        subscription: OKXSubscriptionArg,
        cancel: CancellationToken,
        gate: SnapshotGate,
        completion: tokio::sync::oneshot::Sender<Result<(), OKXWsError>>,
    },
    /// Send a pre-serialized payload (used for order operations).
    Send {
        payload: String,
        rate_limit_keys: Option<Vec<Ustr>>,
        request_id: Option<String>,
        client_order_ids: Vec<ClientOrderId>,
        op: Option<OKXWsOperation>,
    },
}

impl Debug for HandlerCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetClient(_) => f.write_str("SetClient"),
            Self::Disconnect => f.write_str("Disconnect"),
            Self::Subscribe { args } => f
                .debug_struct(stringify!(Subscribe))
                .field("args", args)
                .finish(),
            Self::SubscribeBook { subscription, .. } => {
                f.debug_tuple("SubscribeBook").field(subscription).finish()
            }
            Self::Unsubscribe { args } => f
                .debug_struct(stringify!(Unsubscribe))
                .field("args", args)
                .finish(),
            Self::Resubscribe { subscription, .. } => {
                f.debug_tuple("Resubscribe").field(subscription).finish()
            }
            Self::Send {
                rate_limit_keys,
                request_id,
                client_order_ids,
                op,
                ..
            } => f
                .debug_struct(stringify!(Send))
                .field("payload", &REDACTED)
                .field("rate_limit_keys", rate_limit_keys)
                .field("request_id", request_id)
                .field("client_order_ids", client_order_ids)
                .field("op", op)
                .finish(),
        }
    }
}

pub(super) struct OKXWsFeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    inner: Option<Arc<WebSocketClient>>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<OKXWsMessage>,
    auth_tracker: AuthTracker,
    requires_auth: bool,
    subscriptions_state: SubscriptionState,
    retry_manager: RetryManager<OKXWsError>,
    pending_messages: VecDeque<OKXWsMessage>,
    pending_sends: VecDeque<PendingSend>,
}

impl OKXWsFeedHandler {
    /// Creates a new [`OKXWsFeedHandler`] instance.
    #[expect(
        clippy::too_many_arguments,
        reason = "the client supplies its authentication requirement alongside the shared tracker"
    )]
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<OKXWsMessage>,
        auth_tracker: AuthTracker,
        requires_auth: bool,
        subscriptions_state: SubscriptionState,
        clock: &'static AtomicTime,
    ) -> Self {
        Self {
            clock,
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            out_tx,
            auth_tracker,
            requires_auth,
            subscriptions_state,
            retry_manager: create_websocket_retry_manager(),
            pending_messages: VecDeque::new(),
            pending_sends: VecDeque::new(),
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
            let keys_owned: Option<Vec<Ustr>> = rate_limit_keys.map(<[Ustr]>::to_vec);
            self.retry_manager
                .invocation(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        let keys = keys_owned.clone();
                        async move {
                            client
                                .send_text(payload, keys.as_deref())
                                .await
                                .map_err(OKXWsError::TransportSend)
                        }
                    },
                    should_retry_replay_safe_error,
                    create_okx_retry_error,
                )
                .execute()
                .await
        } else {
            Err(OKXWsError::NoActiveClient)
        }
    }

    async fn send_on_connection(
        &self,
        payload: String,
        rate_limit_keys: Option<&[Ustr]>,
    ) -> Result<(), OKXWsError> {
        let client = self.inner.as_ref().ok_or(OKXWsError::NoActiveClient)?;
        // Capture ownership before checking reconnect so recovery cannot select a replacement
        let connection_epoch = client.connection_epoch();
        if client.connection_mode().is_reconnect() {
            return Err(OKXWsError::TransportSend(SendError::ConnectionChanged));
        }

        client
            .send_text_on_connection(payload, rate_limit_keys, connection_epoch)
            .await
            .map_err(OKXWsError::TransportSend)
    }

    pub(super) async fn send_pong(&self) -> anyhow::Result<()> {
        match self.send_on_connection(TEXT_PONG.to_string(), None).await {
            Ok(()) => {
                log::trace!("Sent pong response to OKX text ping");
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("Failed to send pong: {e}")),
        }
    }

    pub(super) async fn next(&mut self) -> Option<OKXWsMessage> {
        if let Some(message) = self.pending_messages.pop_front() {
            return Some(message);
        }

        let mut poll_raw_next = false;

        loop {
            if self.signal.load(Ordering::Acquire) {
                log::debug!("Stop signal received");
                return None;
            }

            tokio::select! {
                biased;
                () = std::future::ready(()), if !poll_raw_next && self.can_send_pending() => {
                    self.send_pending().await;
                    poll_raw_next = true;
                }
                Some(cmd) = self.cmd_rx.recv(), if !poll_raw_next => {
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
                        HandlerCommand::Subscribe { args } => {
                            if let Err(e) = self.handle_subscribe(args).await {
                                log::error!("Failed to handle subscribe command: error={e}");
                            }
                        }
                        HandlerCommand::SubscribeBook { subscription, cancel, gate, completion } => {
                            let result = tokio::select! {
                                biased;
                                () = cancel.cancelled() => continue,
                                result = async {
                                    let client = self.inner.as_ref().ok_or(OKXWsError::NoActiveClient)?;
                                    self.send_book_subscription(
                                        OKXWsOperation::Subscribe,
                                        subscription,
                                        client.connection_epoch(),
                                    ).await
                                } => result,
                            };

                            if result.is_ok() {
                                gate.open();
                            }
                            let _ = completion.send(result);
                        }
                        HandlerCommand::Unsubscribe { args } => {
                            if let Err(e) = self.handle_unsubscribe(args).await {
                                log::error!("Failed to handle unsubscribe command: error={e}");
                            }
                        }
                        HandlerCommand::Resubscribe { subscription, cancel, gate, completion } => {
                            let result = tokio::select! {
                                biased;
                                () = cancel.cancelled() => continue,
                                result = async {
                                    self.subscriptions_state.mark_failure(&topic_from_subscription_arg(&subscription));
                                    let client = self.inner.as_ref().ok_or(OKXWsError::NoActiveClient)?;
                                    let epoch = client.connection_epoch();
                                    self.send_book_subscription(
                                        OKXWsOperation::Unsubscribe,
                                        subscription.clone(),
                                        epoch,
                                    ).await?;
                                    self.send_book_subscription(
                                        OKXWsOperation::Subscribe,
                                        subscription,
                                        epoch,
                                    ).await
                                } => result,
                            };

                            if result.is_ok() {
                                gate.open();
                            }
                            let _ = completion.send(result);
                        }
                        HandlerCommand::Send {
                            payload,
                            rate_limit_keys,
                            request_id,
                            client_order_ids,
                            op,
                        } => {
                            let pending = PendingSend {
                                payload,
                                rate_limit_keys,
                                request_id,
                                client_order_ids,
                                op,
                                attempted: false,
                            };

                            if self.inner.is_none() {
                                self.fail_send(pending, OKXWsError::NoActiveClient);
                            } else if self.pending_sends.len() >= PENDING_SENDS_CAPACITY {
                                self.fail_send(pending, OKXWsError::TransportSend(SendError::BufferFull));
                            } else {
                                log::debug!(
                                    "Retained WebSocket command: request_id={:?}, op={:?}, mode={:?}",
                                    pending.request_id,
                                    pending.op,
                                    self.inner.as_ref().map(|client| client.connection_mode()),
                                );
                                self.pending_sends.push_back(pending);
                            }
                        }
                    }

                    poll_raw_next = true;
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

                            self.auth_tracker.fail(msg.clone());

                            let error = OKXWebSocketError {
                                code,
                                message: msg,
                                conn_id: Some(conn_id),
                                timestamp: self.clock.get_time_ns().as_u64(),
                            };
                            self.pending_messages.push_back(OKXWsMessage::Error(error));
                        }
                        OKXWsFrame::BookData { arg, action, data } => {
                            return Some(OKXWsMessage::BookData { arg, action, data });
                        }
                        OKXWsFrame::RpiBookData { arg, action, data } => {
                            return Some(OKXWsMessage::RpiBookData { arg, action, data });
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
                        OKXWsFrame::Error { arg, code, msg } => {
                            let arg = arg.or_else(|| subscription_arg_from_error_message(&msg));
                            if let Some(arg) = arg
                                && self.handle_subscription_error(&arg, &code, &msg)
                            {
                                return Some(OKXWsMessage::SubscriptionFailed {
                                    channel: arg.channel,
                                    inst_id: arg.inst_id,
                                    code,
                                    msg,
                                });
                            }

                            let error = OKXWebSocketError {
                                code,
                                message: msg,
                                conn_id: None,
                                timestamp: self.clock.get_time_ns().as_u64(),
                            };
                            return Some(OKXWsMessage::Error(error));
                        }
                        OKXWsFrame::Reconnected => {
                            return Some(OKXWsMessage::Reconnected);
                        }
                        OKXWsFrame::Subscription {
                            event, arg, code, msg,
                            ..
                        } => {
                            let rejected = self
                                .handle_subscription_ack(&event, &arg, code.as_deref(), msg.as_deref());

                            if rejected {
                                return Some(OKXWsMessage::SubscriptionFailed {
                                    channel: arg.channel,
                                    inst_id: arg.inst_id,
                                    code: code.unwrap_or_default(),
                                    msg: msg.unwrap_or_default(),
                                });
                            }
                        }
                        OKXWsFrame::ChannelConnCount { .. } => {}
                    }
                }

                () = std::future::ready(()), if poll_raw_next => {
                    poll_raw_next = false;
                }

                // Polled last so a ready message never registers and drops a timer
                () = time::sleep(time::Duration::from_millis(100)) => {
                    // Wake the loop to poll the stop signal while both channels are idle
                }

                else => {
                    log::debug!("Handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    fn can_send_pending(&self) -> bool {
        !self.pending_sends.is_empty()
            && (!self.requires_auth || self.auth_tracker.is_authenticated())
            && self.inner.as_ref().is_some_and(|client| client.is_active())
    }

    async fn send_pending(&mut self) {
        let client = self.inner.as_ref().expect("pending send requires a client");
        let epoch = client.connection_epoch();

        if self.requires_auth && !self.auth_tracker.is_authenticated() {
            return;
        }

        let pending = self.pending_sends.front_mut().expect("pending send exists");
        pending.attempted = true;
        let result = client
            .send_text_on_connection(
                pending.payload.clone(),
                pending.rate_limit_keys.as_deref(),
                epoch,
            )
            .await;

        if matches!(result, Err(SendError::ConnectionChanged)) {
            pending.attempted = false;
            self.auth_tracker.invalidate();
            return;
        }

        if matches!(result, Err(SendError::Timeout)) {
            self.auth_tracker.invalidate();
        }

        let pending = self.pending_sends.pop_front().expect("pending send exists");
        if let Err(e) = result {
            self.fail_send(pending, OKXWsError::TransportSend(e));
        }
    }

    fn fail_send(&self, pending: PendingSend, error: OKXWsError) {
        if let Some(request_id) = pending.request_id {
            let message = OKXWsMessage::SendFailed {
                request_id,
                client_order_ids: pending.client_order_ids,
                op: pending.op,
                error,
            };

            if self.send(message).is_err() {
                if !self.is_stopped() {
                    log::error!("Failed to forward WebSocket send failure: receiver dropped");
                }

                self.signal.store(true, Ordering::Release);
            }
        }
    }

    fn route_data_message(&self, arg: OKXWebSocketArg, mut data: Value) -> Option<OKXWsMessage> {
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
            OKXWsChannel::LiquidationWarning => {
                parse_array_items(data, "liquidation warnings", false)
                    .map(OKXWsMessage::LiquidationWarnings)
            }
            OKXWsChannel::Instruments => {
                prefer_rpi_response_fields(&mut data);
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
    ) -> bool {
        let topic = topic_from_websocket_arg(arg);
        let success = code.is_none_or(|c| c == OKX_SUCCESS_CODE);

        match event {
            OKXSubscriptionEvent::Subscribe => {
                if success {
                    self.subscriptions_state.confirm_subscribe(&topic);
                    false
                } else {
                    log::warn!(
                        "Subscription failed: topic={topic:?}, error={msg:?}, code={code:?}"
                    );
                    self.subscriptions_state.mark_failure(&topic);
                    true
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
                false
            }
        }
    }

    fn handle_subscription_error(&self, arg: &OKXWebSocketArg, code: &str, msg: &str) -> bool {
        let topic = topic_from_websocket_arg(arg);
        let event = if self
            .subscriptions_state
            .pending_unsubscribe_topics()
            .iter()
            .any(|pending| pending == &topic)
        {
            OKXSubscriptionEvent::Unsubscribe
        } else if self
            .subscriptions_state
            .pending_subscribe_topics()
            .iter()
            .any(|pending| pending == &topic)
            || (arg.channel.is_book()
                && self
                    .subscriptions_state
                    .all_topics()
                    .iter()
                    .any(|tracked| tracked == &topic))
        {
            OKXSubscriptionEvent::Subscribe
        } else {
            return false;
        };

        self.handle_subscription_ack(&event, arg, Some(code), Some(msg))
    }

    async fn send_book_subscription(
        &self,
        op: OKXWsOperation,
        subscription: OKXSubscriptionArg,
        connection_epoch: u64,
    ) -> Result<(), OKXWsError> {
        let client = self.inner.as_ref().ok_or(OKXWsError::NoActiveClient)?;
        let message = OKXSubscription {
            op,
            args: vec![subscription],
        };
        let payload =
            serde_json::to_string(&message).map_err(|e| OKXWsError::ClientError(e.to_string()))?;

        client
            .send_text_on_connection(
                payload,
                Some(OKX_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()),
                connection_epoch,
            )
            .await
            .map_err(OKXWsError::TransportSend)
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
                        OKXWsFrame::Error { arg, code, msg } => {
                            let level = error_frame_level(arg.as_ref(), code, msg);
                            log::log!(level, "WebSocket error: {code} - {msg}");
                            Some(ws_event)
                        }
                        OKXWsFrame::Login {
                            event,
                            code,
                            msg,
                            conn_id,
                        } => {
                            if code == OKX_SUCCESS_CODE {
                                log::debug!("WebSocket authenticated: conn_id={conn_id}");
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
                        OKXWsFrame::Data { .. }
                        | OKXWsFrame::BookData { .. }
                        | OKXWsFrame::RpiBookData { .. } => Some(ws_event),
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

// Book recovery retries a rejected book subscription and reports its own failure
fn error_frame_level(arg: Option<&OKXWebSocketArg>, code: &str, msg: &str) -> log::Level {
    let book = match arg {
        Some(arg) => arg.channel.is_book(),
        None => subscription_arg_from_error_message(msg).is_some_and(|arg| arg.channel.is_book()),
    };

    if should_retry_error_code(code) || book {
        log::Level::Warn
    } else {
        log::Level::Error
    }
}

fn subscription_arg_from_error_message(msg: &str) -> Option<OKXWebSocketArg> {
    let descriptor = msg
        .strip_prefix("Wrong URL or channel:")?
        .split_whitespace()
        .next()?;
    let mut fields = descriptor.split(',');
    let channel = fields.next()?;
    let mut arg = Map::new();
    arg.insert("channel".to_string(), Value::String(channel.to_string()));

    for field in fields {
        let (key, value) = field.split_once(':')?;
        if !matches!(key, "instId" | "sprdId" | "instType" | "instFamily") {
            return None;
        }
        arg.insert(key.to_string(), Value::String(value.to_string()));
    }

    serde_json::from_value(Value::Object(arg)).ok()
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

/// Returns `true` when an RPI order update is canceled without any fill.
pub fn is_unfilled_rpi_cancel(msg: &OKXOrderMsg) -> bool {
    msg.ord_type == OKXOrderType::Rpi
        && msg.state == OKXOrderStatus::Canceled
        && msg
            .acc_fill_sz
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

const PENDING_SENDS_CAPACITY: usize = 1_024;

struct PendingSend {
    payload: String,
    rate_limit_keys: Option<Vec<Ustr>>,
    request_id: Option<String>,
    client_order_ids: Vec<ClientOrderId>,
    op: Option<OKXWsOperation>,
    attempted: bool,
}

impl Drop for OKXWsFeedHandler {
    fn drop(&mut self) {
        self.cmd_rx.close();

        while let Some(pending) = self.pending_sends.pop_front() {
            let error = if pending.attempted {
                OKXWsError::SendFailed(
                    "handler stopped before the write outcome was known".to_string(),
                )
            } else {
                OKXWsError::TransportSend(SendError::Closed)
            };

            self.fail_send(pending, error);
        }

        while let Ok(command) = self.cmd_rx.try_recv() {
            if let HandlerCommand::Send {
                payload,
                rate_limit_keys,
                request_id,
                client_order_ids,
                op,
            } = command
            {
                self.fail_send(
                    PendingSend {
                        payload,
                        rate_limit_keys,
                        request_id,
                        client_order_ids,
                        op,
                        attempted: false,
                    },
                    OKXWsError::TransportSend(SendError::Closed),
                );
            }
        }
    }
}

fn should_retry_replay_safe_error(error: &OKXWsError) -> bool {
    match error {
        OKXWsError::OkxError { error_code, .. } => should_retry_error_code(error_code),
        OKXWsError::TransportSend(SendError::Timeout | SendError::ConnectionChanged)
        | OKXWsError::TungsteniteError(_)
        | OKXWsError::OperationTimeout { .. } => true,
        OKXWsError::AuthenticationError(_)
        | OKXWsError::JsonError(_)
        | OKXWsError::ParsingError(_)
        | OKXWsError::ClientError(_)
        | OKXWsError::NoActiveClient
        | OKXWsError::HandlerUnavailable(_)
        | OKXWsError::TransportSend(
            SendError::InvalidInput(_)
            | SendError::BufferFull
            | SendError::Closed
            | SendError::WriteTimeout
            | SendError::BrokenPipe(_),
        )
        | OKXWsError::SendFailed(_) => false,
    }
}

fn create_okx_retry_error(error: RetryError) -> OKXWsError {
    match error {
        RetryError::OperationTimeout { timeout_ms } => OKXWsError::OperationTimeout { timeout_ms },
        RetryError::InvalidConfiguration { message } => OKXWsError::ClientError(message),
        RetryError::Canceled => {
            OKXWsError::SendFailed("Adapter disconnecting or shutting down".to_string())
        }
        error @ RetryError::ElapsedBudgetExceeded { .. } => {
            OKXWsError::SendFailed(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU32,
        sync::{Arc, atomic::AtomicBool},
        time::Duration,
    };

    use futures_util::{SinkExt, StreamExt};
    use nautilus_core::{collections::AtomicMap, time::get_atomic_clock_realtime};
    use nautilus_model::identifiers::InstrumentId;
    use nautilus_network::{
        mode::{ConnectionMode, ReconnectRequestOutcome},
        ratelimiter::{RateLimiter, quota::Quota},
        websocket::{
            AuthTracker, SubscriptionState, TransportBackend, WebSocketConfig,
            channel_message_handler,
        },
    };
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::{
        book::{BookChannelScope, BookSequenceOutcome, sync::BookSyncTracker},
        common::{
            consts::OKX_WS_TOPIC_DELIMITER,
            enums::{OKXBookAction, OKXBookChannel, OKXRpiPermission},
            testing::load_test_json,
        },
    };

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
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        )
    }

    #[rstest]
    fn test_command_debug_redacts_payloads() {
        let payload = "authentication-secret";

        let send = HandlerCommand::Send {
            payload: payload.to_string(),
            rate_limit_keys: None,
            request_id: None,
            client_order_ids: Vec::new(),
            op: None,
        };

        let debug = format!("{send:?}");

        assert!(debug.contains(REDACTED));
        assert!(!debug.contains(payload));
    }

    #[rstest]
    #[case::submit(OKXWsOperation::Order)]
    #[case::cancel(OKXWsOperation::CancelOrder)]
    #[case::amend(OKXWsOperation::AmendOrder)]
    #[tokio::test]
    async fn send_failure_returns_without_inbound_frame(#[case] op: OKXWsOperation) {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = OKXWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );
        let client_order_id = ClientOrderId::from("O-disconnected");
        cmd_tx
            .send(HandlerCommand::Send {
                payload: String::new(),
                rate_limit_keys: None,
                request_id: Some("disconnected-request".to_string()),
                client_order_ids: vec![client_order_id],
                op: Some(op.clone()),
            })
            .unwrap();

        let message = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::select! {
                message = out_rx.recv() => message,
                message = handler.next() => panic!("Unexpected handler return: {message:?}"),
            }
        })
        .await
        .expect("send failure must not wait for an inbound frame");

        let Some(OKXWsMessage::SendFailed {
            request_id,
            client_order_ids,
            op: failed_op,
            error,
        }) = message
        else {
            panic!("Expected send failure, received {message:?}");
        };

        assert_eq!(request_id, "disconnected-request");
        assert_eq!(client_order_ids, vec![client_order_id]);
        assert_eq!(failed_op, Some(op));
        assert!(matches!(error, OKXWsError::NoActiveClient));
    }

    #[tokio::test]
    async fn send_failures_preserve_raw_message_fairness() {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = OKXWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );

        for request_id in ["first-cancel", "second-cancel"] {
            cmd_tx
                .send(HandlerCommand::Send {
                    payload: String::new(),
                    rate_limit_keys: None,
                    request_id: Some(request_id.to_string()),
                    client_order_ids: vec![ClientOrderId::from("O-disconnected")],
                    op: Some(OKXWsOperation::CancelOrder),
                })
                .unwrap();
        }

        raw_tx
            .send(Message::Text(RECONNECTED.to_string().into()))
            .unwrap();

        raw_tx
            .send(Message::Text(RECONNECTED.to_string().into()))
            .unwrap();

        let messages = tokio::time::timeout(Duration::from_secs(1), async {
            for _ in 0..2 {
                let message = handler.next().await.expect("raw message");
                handler.send(message).unwrap();
            }

            [
                out_rx.recv().await,
                out_rx.recv().await,
                out_rx.recv().await,
                out_rx.recv().await,
            ]
        })
        .await
        .expect("failures and reconnect must arrive without more input");

        assert!(
            matches!(&messages[0], Some(OKXWsMessage::SendFailed { request_id, .. })
            if request_id == "first-cancel")
        );
        assert!(matches!(messages[1], Some(OKXWsMessage::Reconnected)));
        assert!(
            matches!(&messages[2], Some(OKXWsMessage::SendFailed { request_id, .. })
            if request_id == "second-cancel")
        );
        assert!(matches!(messages[3], Some(OKXWsMessage::Reconnected)));
        assert!(handler.pending_messages.is_empty());
        assert!(handler.cmd_rx.is_empty());
    }

    #[tokio::test]
    async fn test_next_polls_raw_after_one_ready_command() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handler = OKXWsFeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );

        for _ in 0..3 {
            cmd_tx
                .send(HandlerCommand::Subscribe { args: Vec::new() })
                .unwrap();
        }
        raw_tx
            .send(Message::Text(RECONNECTED.to_string().into()))
            .unwrap();

        let message = handler.next().await;

        assert!(matches!(message, Some(OKXWsMessage::Reconnected)));
        assert_eq!(handler.cmd_rx.len(), 2);
    }

    #[rstest]
    #[case::sent(false)]
    #[case::canceled(true)]
    #[tokio::test]
    async fn initial_book_completion_waits_for_send(#[case] canceled: bool) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());
        let (wire_tx, mut wire_rx) = tokio::sync::mpsc::unbounded_channel();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            while let Some(Ok(frame)) = socket.next().await {
                wire_tx.send(frame).unwrap();
            }
        });

        let (message_handler, raw_rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(WebSocketConfig::builder().url(url).build().unwrap())
            .message_handler(message_handler)
            .default_quota(Quota::with_period(Duration::from_secs(10)).unwrap())
            .connect()
            .await
            .unwrap();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = OKXWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );
        handler.inner = Some(Arc::new(client));
        cmd_tx
            .send(HandlerCommand::Subscribe { args: Vec::new() })
            .unwrap();
        let cancel = CancellationToken::new();
        let gate = SnapshotGate::default();
        gate.lock().close();
        let (completion, mut result) = tokio::sync::oneshot::channel();
        cmd_tx
            .send(HandlerCommand::SubscribeBook {
                subscription: OKXSubscriptionArg {
                    channel: OKXWsChannel::Books,
                    inst_id: Some(Ustr::from("BTC-USDT")),
                    inst_type: None,
                    inst_family: None,
                },
                cancel: cancel.clone(),
                gate: gate.clone(),
                completion,
            })
            .unwrap();

        let running = tokio::spawn(async move { handler.next().await });
        let first = tokio::time::timeout(Duration::from_secs(3), wire_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let first: Value = serde_json::from_str(first.to_text().unwrap()).unwrap();
        assert_eq!(first, serde_json::json!({"op": "subscribe", "args": []}));
        assert!(matches!(
            result.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        if canceled {
            cancel.cancel();
        }

        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(10)).await;
        tokio::time::resume();
        let result = tokio::time::timeout(Duration::from_secs(3), result)
            .await
            .unwrap();

        if canceled {
            assert!(gate.lock().is_closed());
            assert!(result.is_err());
            assert!(
                tokio::time::timeout(Duration::from_millis(100), wire_rx.recv())
                    .await
                    .is_err()
            );
        } else {
            result.unwrap().unwrap();
            assert!(!gate.lock().is_closed());
            let frame = tokio::time::timeout(Duration::from_secs(3), wire_rx.recv())
                .await
                .unwrap()
                .unwrap();
            let frame: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            assert_eq!(
                frame,
                serde_json::json!({"op": "subscribe", "args": [{"channel": "books", "instId": "BTC-USDT"}]})
            );
        }

        running.abort();
        server.abort();
    }

    #[tokio::test]
    async fn book_subscription_rejects_changed_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());
        let (wire_tx, mut wire_rx) = tokio::sync::mpsc::unbounded_channel();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            while let Some(Ok(frame)) = socket.next().await {
                wire_tx.send(frame).unwrap();
            }
        });

        let (message_handler, raw_rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(WebSocketConfig::builder().url(url).build().unwrap())
            .message_handler(message_handler)
            .connect()
            .await
            .unwrap();
        let wrong_epoch = client.connection_epoch() + 1;
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handler = OKXWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );
        handler.inner = Some(Arc::new(client));

        let result = handler
            .send_book_subscription(
                OKXWsOperation::Subscribe,
                OKXSubscriptionArg {
                    channel: OKXWsChannel::Books,
                    inst_id: Some(Ustr::from("BTC-USDT")),
                    inst_type: None,
                    inst_family: None,
                },
                wrong_epoch,
            )
            .await;

        assert!(matches!(
            result,
            Err(OKXWsError::TransportSend(SendError::ConnectionChanged))
        ));
        assert!(matches!(
            wire_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        handler.inner.as_ref().unwrap().disconnect().await;
        server.abort();
    }

    #[tokio::test]
    async fn recovery_cancel_between_sends_keeps_snapshot_gate_closed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());
        let (received, first_frame) = tokio::sync::oneshot::channel();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let frame = socket.next().await.unwrap().unwrap();
            received.send(frame).unwrap();

            while socket.next().await.is_some() {}
        });

        let (message_handler, raw_rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(WebSocketConfig::builder().url(url).build().unwrap())
            .message_handler(message_handler)
            .default_quota(Quota::per_hour(NonZeroU32::new(1).unwrap()))
            .connect()
            .await
            .unwrap();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = OKXWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );
        handler.inner = Some(Arc::new(client));
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        tracker.record_subscription(instrument_id, time::Instant::now(), SnapshotGate::default());
        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        assert!(recovery.begin_replacement());
        let cancel = recovery.cancellation.child_token();
        let (completion, result) = tokio::sync::oneshot::channel();
        cmd_tx
            .send(HandlerCommand::Resubscribe {
                subscription: OKXSubscriptionArg {
                    channel: OKXWsChannel::Books,
                    inst_id: Some(Ustr::from("BTC-USDT")),
                    inst_type: None,
                    inst_family: None,
                },
                cancel: cancel.clone(),
                gate: recovery.gate.clone(),
                completion,
            })
            .unwrap();

        let running = tokio::spawn(async move { handler.next().await });
        let frame = tokio::time::timeout(Duration::from_secs(3), first_frame)
            .await
            .unwrap()
            .unwrap();
        let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        assert_eq!(request["op"], "unsubscribe");
        assert_eq!(
            tracker.validate_sequence(instrument_id, true, &[(Some(-1), 42)], time::Instant::now()),
            BookSequenceOutcome::Suppress
        );
        cancel.cancel();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), result)
                .await
                .unwrap()
                .is_err()
        );

        assert!(recovery.gate.lock().is_closed());
        assert_eq!(
            tracker.validate_sequence(instrument_id, true, &[(Some(-1), 43)], time::Instant::now()),
            BookSequenceOutcome::Suppress
        );
        cmd_tx.send(HandlerCommand::Disconnect).unwrap();
        tokio::time::timeout(Duration::from_secs(3), running)
            .await
            .unwrap()
            .unwrap();
        server.abort();
    }

    #[rstest]
    #[case::disabled_deadline(Duration::ZERO)]
    #[case::snapshot_deadline(Duration::from_secs(3))]
    #[tokio::test]
    async fn reconnect_replay_survives_inflight_recovery(#[case] snapshot_timeout: Duration) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());
        let (wire_tx, mut wire_rx) = tokio::sync::mpsc::unbounded_channel();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();

            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let request: Value = serde_json::from_str(&text).unwrap();
                wire_tx
                    .send(request["op"].as_str().unwrap().to_owned())
                    .unwrap();

                if request["op"] == "subscribe" {
                    for (fixture, sequence, previous) in [
                        ("ws_books_snapshot.json", 100, -1),
                        ("ws_books_update.json", 101, 100),
                    ] {
                        let mut frame: Value =
                            serde_json::from_str(&load_test_json(fixture)).unwrap();
                        frame["data"][0]["seqId"] = json!(sequence);
                        frame["data"][0]["prevSeqId"] = json!(previous);
                        socket
                            .send(Message::Text(frame.to_string().into()))
                            .await
                            .unwrap();
                    }
                }
            }
        });

        // Replay and unsubscribe consume the burst; the replacement subscribe cannot send
        let limiter = Arc::new(RateLimiter::new_with_quota(
            Some(
                Quota::with_period(Duration::from_secs(10))
                    .unwrap()
                    .allow_burst(NonZeroU32::new(2).unwrap()),
            ),
            Vec::new(),
        ));
        let (message_handler, raw_rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(WebSocketConfig::builder().url(url).build().unwrap())
            .message_handler(message_handler)
            .rate_limiter(Arc::clone(&limiter))
            .connect()
            .await
            .unwrap();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = OKXWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );
        handler.inner = Some(Arc::new(client));
        let (messages_tx, mut messages_rx) = tokio::sync::mpsc::unbounded_channel();

        let running = tokio::spawn(async move {
            while let Some(message) = handler.next().await {
                messages_tx.send(message).unwrap();
            }
        });

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let channels = AtomicMap::new();
        channels.insert(instrument_id, OKXBookChannel::Book);
        let tracker = BookSyncTracker::default();
        tracker.record_subscription(instrument_id, time::Instant::now(), SnapshotGate::default());
        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        assert!(recovery.begin_replacement());

        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Books,
            inst_id: Some(Ustr::from("BTC-USDT")),
            inst_type: None,
            inst_family: None,
        };

        cmd_tx
            .send(HandlerCommand::Subscribe {
                args: vec![arg.clone()],
            })
            .unwrap();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), wire_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            "subscribe"
        );

        for action in [OKXBookAction::Snapshot, OKXBookAction::Update] {
            let message = tokio::time::timeout(Duration::from_secs(3), messages_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(
                matches!(message, OKXWsMessage::BookData { action: received, .. } if received == action)
            );
        }

        let (completion, result) = tokio::sync::oneshot::channel();
        cmd_tx
            .send(HandlerCommand::Resubscribe {
                subscription: arg.clone(),
                cancel: recovery.cancellation.child_token(),
                gate: recovery.gate.clone(),
                completion,
            })
            .unwrap();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), wire_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            "unsubscribe"
        );

        // Paused time keeps the subscribe blocked until the reset completes
        tokio::time::pause();
        tracker.reset_sequences(&channels, BookChannelScope::Public);

        if !snapshot_timeout.is_zero() {
            tracker.seed_pending_snapshots(
                &channels,
                BookChannelScope::Public,
                snapshot_timeout,
                time::Instant::now(),
            );
        }

        tokio::time::advance(Duration::from_secs(10)).await;
        tokio::time::resume();
        let _ = tokio::time::timeout(Duration::from_secs(3), result)
            .await
            .expect("recovery command completes after reconnect");

        let resumed = tokio::time::timeout(Duration::from_secs(3), async {
            for (action, sequence) in [(OKXBookAction::Snapshot, 100), (OKXBookAction::Update, 101)]
            {
                let Some(OKXWsMessage::BookData {
                    action: received,
                    data,
                    ..
                }) = messages_rx.recv().await
                else {
                    panic!("expected book data after reconnect");
                };

                assert_eq!(received, action);
                assert_eq!(data[0].seq_id, sequence);
                assert_eq!(
                    tracker.validate_sequence(
                        instrument_id,
                        received == OKXBookAction::Snapshot,
                        &[(data[0].prev_seq_id, data[0].seq_id)],
                        time::Instant::now(),
                    ),
                    BookSequenceOutcome::Accept,
                );
            }
        })
        .await;

        // A successful manual subscribe isolates a failure to automatic replay
        if resumed.is_err() {
            cmd_tx
                .send(HandlerCommand::Subscribe { args: vec![arg] })
                .unwrap();

            for action in [OKXBookAction::Snapshot, OKXBookAction::Update] {
                let message = tokio::time::timeout(Duration::from_secs(3), messages_rx.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(
                    matches!(message, OKXWsMessage::BookData { action: received, .. } if received == action)
                );
            }
        }

        cmd_tx.send(HandlerCommand::Disconnect).unwrap();
        tokio::time::timeout(Duration::from_secs(3), running)
            .await
            .unwrap()
            .unwrap();
        server.abort();

        assert!(
            resumed.is_ok(),
            "reconnect must preserve an in-flight recovery and resume book output"
        );
    }

    #[rstest]
    fn test_should_retry_typed_transport_and_timeout_errors() {
        assert!(!should_retry_replay_safe_error(&OKXWsError::TransportSend(
            SendError::BufferFull
        )));
        assert!(should_retry_replay_safe_error(&OKXWsError::TransportSend(
            SendError::Timeout
        )));
        assert!(should_retry_replay_safe_error(&OKXWsError::TransportSend(
            SendError::ConnectionChanged
        )));
        assert!(!should_retry_replay_safe_error(&OKXWsError::TransportSend(
            SendError::WriteTimeout
        )));
        assert!(!should_retry_replay_safe_error(&OKXWsError::TransportSend(
            SendError::BrokenPipe("connection reset".to_string())
        )));
        assert!(should_retry_replay_safe_error(
            &OKXWsError::OperationTimeout { timeout_ms: 1_000 }
        ));
        assert!(!should_retry_replay_safe_error(&OKXWsError::NoActiveClient));
        assert!(!should_retry_replay_safe_error(
            &OKXWsError::HandlerUnavailable("closed".to_string())
        ));
    }

    #[rstest]
    fn test_retryability_uses_websocket_error_type_not_message() {
        let message = "connection reset".to_string();
        let temporary = OKXWsError::OkxError {
            error_code: "50011".to_string(),
            message: message.clone(),
        };
        let permanent = OKXWsError::ClientError(message.clone());
        let ambiguous = OKXWsError::SendFailed(message);

        assert!(should_retry_replay_safe_error(&temporary));
        assert!(!should_retry_replay_safe_error(&permanent));
        assert!(!should_retry_replay_safe_error(&ambiguous));
    }

    #[rstest]
    fn test_subscription_error_restores_failed_unsubscribe() {
        let handler = create_handler();
        let arg = OKXWebSocketArg {
            channel: OKXWsChannel::Books,
            inst_id: Some(Ustr::from("BTC-USD")),
            inst_type: None,
            inst_family: None,
            bar: None,
        };
        let topic = topic_from_websocket_arg(&arg);
        handler.subscriptions_state.mark_subscribe(&topic);
        handler.subscriptions_state.confirm_subscribe(&topic);
        handler.subscriptions_state.mark_unsubscribe(&topic);

        let rejected_subscription =
            handler.handle_subscription_error(&arg, "60019", "Unsubscription failed");

        assert!(!rejected_subscription);
        assert_eq!(handler.subscriptions_state.all_topics(), vec![topic]);
        assert!(
            handler
                .subscriptions_state
                .pending_subscribe_topics()
                .is_empty()
        );
        assert!(
            handler
                .subscriptions_state
                .pending_unsubscribe_topics()
                .is_empty()
        );
    }

    #[rstest]
    fn test_subscription_arg_from_error_message_matches_mainnet_shape() {
        let msg = "Wrong URL or channel:books,instId:BTC-USDT-SWAP doesn't exist. Please use the \
                   correct URL, channel and parameters referring to API document.";

        let arg = subscription_arg_from_error_message(msg).unwrap();

        assert_eq!(arg.channel, OKXWsChannel::Books);
        assert_eq!(arg.inst_id, Some(Ustr::from("BTC-USDT-SWAP")));
        assert_eq!(arg.inst_type, None);
        assert_eq!(arg.inst_family, None);
        assert_eq!(arg.bar, None);
    }

    #[rstest]
    #[case::book_arg(
        Some(OKXWsChannel::Books),
        "60018",
        "Subscription failed",
        log::Level::Warn
    )]
    #[case::book_from_message(
        None,
        "60018",
        "Wrong URL or channel:books,instId:BTC-USDT-SWAP doesn't exist.",
        log::Level::Warn
    )]
    #[case::retryable(
        Some(OKXWsChannel::Tickers),
        "50001",
        "Service unavailable",
        log::Level::Warn
    )]
    #[case::non_book_arg(
        Some(OKXWsChannel::Tickers),
        "60018",
        "Subscription failed",
        log::Level::Error
    )]
    #[case::non_book_from_message(
        None,
        "60018",
        "Wrong URL or channel:tickers,instId:BTC-USDT-SWAP doesn't exist.",
        log::Level::Error
    )]
    fn test_error_frame_level(
        #[case] channel: Option<OKXWsChannel>,
        #[case] code: &str,
        #[case] msg: &str,
        #[case] expected: log::Level,
    ) {
        let arg = channel.map(|channel| OKXWebSocketArg {
            channel,
            inst_id: Some(Ustr::from("BTC-USDT-SWAP")),
            inst_type: None,
            inst_family: None,
            bar: None,
        });

        let level = error_frame_level(arg.as_ref(), code, msg);

        assert_eq!(level, expected);
    }

    #[rstest]
    fn test_subscription_error_ignores_non_pending_topic() {
        let handler = create_handler();
        let arg = OKXWebSocketArg {
            channel: OKXWsChannel::Books,
            inst_id: Some(Ustr::from("BTC-USDT-SWAP")),
            inst_type: None,
            inst_family: None,
            bar: None,
        };

        let rejected_subscription =
            handler.handle_subscription_error(&arg, "60018", "Subscription failed");

        assert!(!rejected_subscription);
        assert!(handler.subscriptions_state.all_topics().is_empty());
        assert!(
            handler
                .subscriptions_state
                .pending_subscribe_topics()
                .is_empty()
        );
        assert!(
            handler
                .subscriptions_state
                .pending_unsubscribe_topics()
                .is_empty()
        );
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
                assert_eq!(instruments[0].inst_id, "BTC-USDT-SWAP");
            }
            other => panic!("Expected Instruments, was {other:?}"),
        }
    }

    #[rstest]
    fn test_route_instruments_prefers_rpi_over_legacy_alias() {
        let handler = create_handler();
        let mut frame: Value =
            serde_json::from_str(&load_test_json("ws_instruments.json")).expect("valid fixture");
        let instrument = &mut frame["data"][0];
        instrument["rpi"] = json!("2");
        instrument["elp"] = json!("1");

        let arg: OKXWebSocketArg = serde_json::from_value(frame["arg"].clone()).expect("valid arg");
        let msg = handler
            .route_data_message(arg, frame["data"].clone())
            .expect("instruments message");

        match msg {
            OKXWsMessage::Instruments(instruments) => {
                assert_eq!(instruments.len(), 1);
                assert_eq!(instruments[0].rpi, Some(OKXRpiPermission::Permitted));
            }
            other => panic!("Expected Instruments, was {other:?}"),
        }
    }

    #[rstest]
    fn test_route_liquidation_warnings() {
        let handler = create_handler();
        let frame: Value = serde_json::from_str(&load_test_json("ws_liquidation_warning.json"))
            .expect("valid fixture");

        let arg: OKXWebSocketArg = serde_json::from_value(frame["arg"].clone()).expect("valid arg");
        let msg = handler
            .route_data_message(arg, frame["data"].clone())
            .expect("liquidation warning message");

        match msg {
            OKXWsMessage::LiquidationWarnings(warnings) => {
                assert_eq!(warnings.len(), 1);
                assert_eq!(warnings[0].inst_id, "BTC-USDT-SWAP");
                assert_eq!(warnings[0].mgn_ratio, "0.62");
            }
            other => panic!("Expected LiquidationWarnings, was {other:?}"),
        }
    }

    #[tokio::test]
    async fn bound_send_rejects_reconnecting_epoch_before_waiting() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let mut frames = Vec::new();
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                frames.push(text.to_string());
            }

            frames
        });

        let (message_handler, raw_rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(WebSocketConfig::builder().url(url).build().unwrap())
            .message_handler(message_handler)
            .connect()
            .await
            .unwrap();
        let mode = client.connection_mode_atomic();
        let epoch = client.connection_epoch();
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = OKXWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );
        handler.inner = Some(Arc::new(client));
        handler
            .send_on_connection("active-control".to_string(), None)
            .await
            .unwrap();

        tokio::time::pause();
        mode.store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);

        let (ready, result) = {
            let mut send =
                Box::pin(handler.send_on_connection("cancel-in-reconnect".to_string(), None));

            match futures_util::poll!(send.as_mut()) {
                std::task::Poll::Ready(result) => (true, result),
                std::task::Poll::Pending => {
                    mode.store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);
                    tokio::time::advance(Duration::from_millis(100)).await;
                    (false, send.await)
                }
            }
        };

        mode.store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);
        tokio::time::resume();
        let final_epoch = handler.inner.as_ref().unwrap().connection_epoch();
        handler.inner.as_ref().unwrap().disconnect().await;
        let frames = tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("peer did not close")
            .unwrap();

        assert_eq!(frames, vec!["active-control"]);
        assert_eq!(final_epoch, epoch);
        assert!(
            ready,
            "reconnecting send must fail without waiting for recovery"
        );
        assert!(
            matches!(
                result,
                Err(OKXWsError::TransportSend(SendError::ConnectionChanged))
            ),
            "expected rejection for a reconnecting epoch, received {result:?}"
        );
    }

    #[tokio::test]
    async fn pending_sends_preserve_exact_bytes_and_fifo() {
        let mut peer = handler_peer().await;
        let payloads = [
            serde_json::json!({"id":"17","op":"cancel-order","args":[{"clOrdId":"OQUEUED17","ordId":"70017","instIdCode":1_000_000_101}]}).to_string(),
            serde_json::json!({"id":"29","op":"amend-order","args":[{"clOrdId":"OQUEUED29","newSz":"0.013","newPx":"49731.2","instIdCode":1_000_000_102}]}).to_string(),
        ];
        peer.handler.auth_tracker.invalidate();

        for (payload, index, op) in [
            (&payloads[0], 17, OKXWsOperation::CancelOrder),
            (&payloads[1], 29, OKXWsOperation::AmendOrder),
        ] {
            let mut pending = order_send(payload.clone(), index);
            pending.op = Some(op);
            peer.handler.pending_sends.push_back(pending);
        }

        assert!(!peer.handler.can_send_pending());
        assert!(peer.frames.try_recv().is_err());
        tokio::time::pause();
        let mut next = Box::pin(peer.handler.next());
        assert!(futures_util::poll!(next.as_mut()).is_pending());
        tokio::time::advance(Duration::from_secs(11)).await;
        assert!(futures_util::poll!(next.as_mut()).is_pending());
        drop(next);
        tokio::time::resume();
        assert_eq!(peer.handler.pending_sends.len(), payloads.len());
        assert!(peer.messages.try_recv().is_err());

        peer.handler.auth_tracker.succeed();
        peer.handler.send_pending().await;
        peer.handler.send_pending().await;
        let frames = [
            peer.frames.recv().await.unwrap(),
            peer.frames.recv().await.unwrap(),
        ];
        assert_eq!(frames, payloads);
        assert!(peer.handler.pending_sends.is_empty());
        assert!(peer.messages.try_recv().is_err());
        peer.handler.inner.as_ref().unwrap().disconnect().await;
        peer.server.abort();
    }

    #[tokio::test]
    async fn pending_send_preserves_fifo_after_connection_change() {
        let mut peer = handler_peer().await;
        let keys = [Ustr::from(SEND_RATE_KEY)];
        let ws_client = peer.handler.inner.as_ref().unwrap();
        let reconnect = ws_client.reconnect_handle();
        ws_client
            .send_text_on_connection("consume-quota".to_string(), Some(&keys), 0)
            .await
            .unwrap();
        assert_eq!(peer.frames.recv().await.unwrap(), "consume-quota");
        let payloads = ["request-17", "request-29"];
        for (payload, index) in [(payloads[0], 17), (payloads[1], 29)] {
            let mut pending = order_send(payload.to_string(), index);
            pending.rate_limit_keys = Some(keys.to_vec());
            peer.handler.pending_sends.push_back(pending);
        }

        let mut send = Box::pin(peer.handler.send_pending());
        assert!(futures_util::poll!(send.as_mut()).is_pending());
        assert_eq!(
            reconnect.request_reconnect(),
            ReconnectRequestOutcome::Accepted
        );
        tokio::time::timeout(Duration::from_secs(5), send)
            .await
            .unwrap();

        assert_eq!(peer.handler.pending_sends.len(), payloads.len());
        assert!(!peer.handler.pending_sends.front().unwrap().attempted);
        assert!(!peer.handler.auth_tracker.is_authenticated());
        assert!(!peer.handler.can_send_pending());
        assert!(peer.frames.try_recv().is_err());
        assert!(peer.messages.try_recv().is_err());

        peer.handler.auth_tracker.succeed();
        peer.handler.send_pending().await;
        peer.handler.send_pending().await;
        let frames = [
            peer.frames.recv().await.unwrap(),
            peer.frames.recv().await.unwrap(),
        ];
        assert_eq!(frames, payloads);
        assert!(peer.handler.pending_sends.is_empty());
        assert!(peer.messages.try_recv().is_err());
        peer.handler.inner.as_ref().unwrap().disconnect().await;
        peer.server.abort();
    }

    #[tokio::test]
    async fn pending_send_rechecks_authentication_after_reconnect() {
        let mut peer = handler_peer().await;
        let payload = "queued-order";
        peer.handler
            .pending_sends
            .push_back(order_send(payload.to_string(), 17));
        assert!(peer.handler.can_send_pending());

        let client = peer.handler.inner.as_ref().unwrap();
        let epoch = client.connection_epoch();
        assert_eq!(
            client.reconnect_handle().request_reconnect(),
            ReconnectRequestOutcome::Accepted
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while client.connection_epoch() == epoch || !client.is_active() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        assert!(!peer.handler.auth_tracker.is_authenticated());

        peer.handler.send_pending().await;

        assert_eq!(peer.handler.pending_sends.len(), 1);
        assert!(!peer.handler.pending_sends.front().unwrap().attempted);
        assert!(peer.frames.try_recv().is_err());
        assert!(peer.messages.try_recv().is_err());

        peer.handler.auth_tracker.succeed();
        peer.handler.send_pending().await;

        assert_eq!(peer.frames.recv().await.unwrap(), payload);
        assert!(peer.handler.pending_sends.is_empty());
        assert!(peer.messages.try_recv().is_err());
        peer.handler.inner.as_ref().unwrap().disconnect().await;
        peer.server.abort();
    }

    #[tokio::test]
    async fn pending_send_timeout_fails_only_head() {
        let mut peer = handler_peer().await;
        for index in [17, 29] {
            peer.handler
                .pending_sends
                .push_back(order_send(format!("request-{index}"), index));
        }

        peer.server.abort();
        peer.handler
            .inner
            .as_ref()
            .unwrap()
            .connection_mode_atomic()
            .store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
        tokio::time::pause();
        let mut send = Box::pin(peer.handler.send_pending());
        assert!(futures_util::poll!(send.as_mut()).is_pending());
        tokio::time::advance(Duration::from_secs(11)).await;
        send.await;
        tokio::time::resume();

        let failure = peer.messages.try_recv().unwrap();
        assert!(matches!(failure, OKXWsMessage::SendFailed {
            request_id, client_order_ids, op: Some(OKXWsOperation::CancelOrder),
            error: OKXWsError::TransportSend(SendError::Timeout),
        } if request_id == "17" && client_order_ids == vec![ClientOrderId::from("OQUEUED17")]));
        assert_eq!(peer.handler.pending_sends.len(), 1);
        assert_eq!(
            peer.handler
                .pending_sends
                .front()
                .unwrap()
                .request_id
                .as_deref(),
            Some("29")
        );
        assert!(!peer.handler.auth_tracker.is_authenticated());
        assert!(!peer.handler.can_send_pending());
        assert!(peer.messages.try_recv().is_err());
        peer.handler.inner.as_ref().unwrap().disconnect().await;
    }

    #[tokio::test]
    async fn pending_send_capacity_and_drop_report_each_command() {
        let mut peer = handler_peer().await;
        peer.handler.auth_tracker.invalidate();
        for index in 0..PENDING_SENDS_CAPACITY {
            peer.handler
                .pending_sends
                .push_back(order_send(format!("request-{index}"), index));
        }

        peer.commands
            .send(HandlerCommand::Send {
                payload: "overflow-request".to_string(),
                rate_limit_keys: None,
                request_id: Some("overflow".to_string()),
                client_order_ids: vec![ClientOrderId::from("OOVERFLOW")],
                op: Some(OKXWsOperation::AmendOrder),
            })
            .unwrap();

        let mut next = Box::pin(peer.handler.next());
        assert!(futures_util::poll!(next.as_mut()).is_pending());
        drop(next);
        let overflow = peer.messages.try_recv().unwrap();
        assert!(
            matches!(overflow, OKXWsMessage::SendFailed { request_id, client_order_ids, op: Some(OKXWsOperation::AmendOrder), error: OKXWsError::TransportSend(SendError::BufferFull) }
            if request_id == "overflow" && client_order_ids == vec![ClientOrderId::from("OOVERFLOW")])
        );
        peer.handler
            .inner
            .as_ref()
            .unwrap()
            .connection_mode_atomic()
            .store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
        peer.handler.auth_tracker.succeed();
        let mut attempted = Box::pin(peer.handler.send_pending());
        assert!(futures_util::poll!(attempted.as_mut()).is_pending());
        drop(attempted);
        peer.commands
            .send(HandlerCommand::Send {
                payload: "unread-request".to_string(),
                rate_limit_keys: None,
                request_id: Some("unread".to_string()),
                client_order_ids: vec![ClientOrderId::from("OUNREAD")],
                op: Some(OKXWsOperation::Order),
            })
            .unwrap();

        peer.handler.inner.as_ref().unwrap().disconnect().await;
        drop(peer.handler);
        for index in 0..PENDING_SENDS_CAPACITY {
            let message = peer.messages.try_recv().unwrap();

            let OKXWsMessage::SendFailed {
                request_id,
                client_order_ids,
                op,
                error,
            } = message
            else {
                panic!("expected send failure")
            };

            assert_eq!(request_id, index.to_string());
            assert_eq!(
                client_order_ids,
                vec![ClientOrderId::from(format!("OQUEUED{index}").as_str())]
            );
            assert_eq!(op, Some(OKXWsOperation::CancelOrder));

            if index == 0 {
                assert!(matches!(error, OKXWsError::SendFailed(_)));
            } else {
                assert!(matches!(
                    error,
                    OKXWsError::TransportSend(SendError::Closed)
                ));
            }
        }

        let unread = peer.messages.try_recv().unwrap();
        assert!(
            matches!(unread, OKXWsMessage::SendFailed { request_id, client_order_ids, op: Some(OKXWsOperation::Order), error: OKXWsError::TransportSend(SendError::Closed) }
            if request_id == "unread" && client_order_ids == vec![ClientOrderId::from("OUNREAD")])
        );
        assert!(peer.messages.try_recv().is_err());
        assert!(peer.frames.try_recv().is_err());
        peer.server.abort();
    }

    #[tokio::test]
    async fn reconnect_preserves_buffered_order_events() {
        let mut peer = handler_peer().await;
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        peer.handler.raw_rx = raw_rx;
        let fixture: Value =
            serde_json::from_str(&load_test_json("ws_reconnect_queued_frames.json")).unwrap();
        let orders = load_test_json("ws_orders.json");
        let orders_frame: Value = serde_json::from_str(&orders).unwrap();
        let expected_orders: Vec<OKXOrderMsg> =
            serde_json::from_value(orders_frame["data"].clone()).unwrap();

        raw_tx.send(Message::Text(orders.into())).unwrap();
        raw_tx
            .send(Message::Text(fixture["response"].to_string().into()))
            .unwrap();

        let ws_client = peer.handler.inner.as_ref().unwrap();
        assert!(ws_client.request_reconnect());
        tokio::time::timeout(Duration::from_secs(5), async {
            while ws_client.connection_epoch() != 1 || !ws_client.is_active() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        let orders = tokio::time::timeout(Duration::from_secs(1), peer.handler.next())
            .await
            .unwrap();

        let Some(OKXWsMessage::Orders(orders)) = orders else {
            panic!("expected buffered order update, received {orders:?}");
        };

        let response = tokio::time::timeout(Duration::from_secs(1), peer.handler.next())
            .await
            .unwrap();

        let Some(OKXWsMessage::OrderResponse {
            id,
            op,
            code,
            msg,
            data,
        }) = response
        else {
            panic!("expected buffered order response, received {response:?}");
        };

        assert_eq!(
            serde_json::to_value(orders).unwrap(),
            serde_json::to_value(expected_orders).unwrap()
        );
        assert_eq!(id.as_deref(), Some("queued-response-17"));
        assert_eq!(op, OKXWsOperation::Order);
        assert_eq!(code, "0");
        assert_eq!(msg, "");
        assert_eq!(data, *fixture["response"]["data"].as_array().unwrap());
        assert!(peer.handler.pending_messages.is_empty());
        assert!(peer.frames.try_recv().is_err());
        peer.handler.inner.as_ref().unwrap().disconnect().await;
        peer.server.abort();
    }

    #[tokio::test]
    async fn failed_login_stops_pending_sends() {
        let mut peer = handler_peer().await;
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        peer.handler.raw_rx = raw_rx;
        let fixture: Value =
            serde_json::from_str(&load_test_json("ws_reconnect_queued_frames.json")).unwrap();
        raw_tx
            .send(Message::Text(fixture["login_failure"].to_string().into()))
            .unwrap();
        raw_tx
            .send(Message::Text(load_test_json("ws_orders.json").into()))
            .unwrap();
        assert!(matches!(
            peer.handler.next().await,
            Some(OKXWsMessage::Orders(_))
        ));
        peer.handler
            .pending_sends
            .push_back(order_send("request-17".to_string(), 17));
        let error = peer.handler.next().await;

        assert!(
            matches!(error, Some(OKXWsMessage::Error(error)) if error.code == "60009" && error.message == "Login failed")
        );
        assert!(!peer.handler.auth_tracker.is_authenticated());
        assert!(!peer.handler.can_send_pending());
        assert!(peer.frames.try_recv().is_err());
        peer.handler.inner.as_ref().unwrap().disconnect().await;
        peer.server.abort();
    }

    #[tokio::test]
    async fn uncertain_write_is_not_replayed_after_recovery() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());
        let (frame_tx, mut frames) = tokio::sync::mpsc::unbounded_channel();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let mut header = [0u8; 1];
            tokio::io::AsyncReadExt::read_exact(socket.get_mut(), &mut header)
                .await
                .unwrap();
            assert_eq!(header[0] & 0x0f, 1);
            drop(socket);
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            while let Some(Ok(frame)) = socket.next().await {
                if let Message::Text(text) = frame {
                    frame_tx.send(text.to_string()).unwrap();
                }
            }
        });

        let (message_handler, raw_rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(
                WebSocketConfig::builder()
                    .url(url)
                    .backend(TransportBackend::Tungstenite)
                    .build()
                    .unwrap(),
            )
            .message_handler(message_handler)
            .connect()
            .await
            .unwrap();
        let (_commands, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, mut messages) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = OKXWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            false,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );
        handler.inner = Some(Arc::new(client));
        handler.auth_tracker.succeed();
        handler
            .pending_sends
            .push_back(order_send("x".repeat(64 * 1024 * 1024), 17));
        handler.send_pending().await;
        let failure = messages.try_recv().unwrap();

        let OKXWsMessage::SendFailed {
            request_id,
            client_order_ids,
            op,
            error,
        } = failure
        else {
            panic!("expected send failure");
        };

        assert_eq!(request_id, "17");
        assert_eq!(client_order_ids, vec![ClientOrderId::from("OQUEUED17")]);
        assert_eq!(op, Some(OKXWsOperation::CancelOrder));
        assert!(matches!(
            error,
            OKXWsError::TransportSend(SendError::BrokenPipe(_) | SendError::WriteTimeout)
        ));
        assert!(handler.pending_sends.is_empty());
        let client = handler.inner.as_ref().unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while client.connection_epoch() == 0 || !client.is_active() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        handler
            .send_on_connection("recovered-control".to_string(), None)
            .await
            .unwrap();
        assert_eq!(frames.recv().await.unwrap(), "recovered-control");
        assert!(frames.try_recv().is_err());
        client.disconnect().await;
        server.abort();
    }

    #[tokio::test]
    async fn closed_output_stops_handler_without_inbound_frames() {
        let mut peer = handler_peer().await;
        peer.messages.close();
        let client = peer.handler.inner.take().unwrap();
        peer.commands
            .send(HandlerCommand::Send {
                payload: "unwritten-request-17".to_string(),
                rate_limit_keys: None,
                request_id: Some("17".to_string()),
                client_order_ids: vec![ClientOrderId::from("OQUEUED17")],
                op: Some(OKXWsOperation::CancelOrder),
            })
            .unwrap();

        let result = tokio::time::timeout(Duration::from_secs(1), peer.handler.next())
            .await
            .unwrap();
        assert!(result.is_none());
        assert!(peer.handler.is_stopped());
        assert!(peer.frames.try_recv().is_err());
        client.disconnect().await;
        peer.server.abort();
    }

    const SEND_RATE_KEY: &str = "owned-send";

    struct HandlerPeer {
        handler: OKXWsFeedHandler,
        commands: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
        frames: tokio::sync::mpsc::UnboundedReceiver<String>,
        messages: tokio::sync::mpsc::UnboundedReceiver<OKXWsMessage>,
        server: tokio::task::JoinHandle<()>,
    }

    async fn handler_peer() -> HandlerPeer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());
        let (frame_tx, frames) = tokio::sync::mpsc::unbounded_channel();

        let server = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();

            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let frame_tx = frame_tx.clone();

                connections.spawn(async move {
                    let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
                    while let Some(Ok(frame)) = socket.next().await {
                        if let Message::Text(text) = frame {
                            frame_tx.send(text.to_string()).unwrap();
                        }
                    }
                });
            }
        });

        let (message_handler, raw_rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(WebSocketConfig::builder().url(url).build().unwrap())
            .message_handler(message_handler)
            .keyed_quotas(vec![(
                SEND_RATE_KEY.to_string(),
                Quota::per_second(NonZeroU32::new(1).unwrap()).unwrap(),
            )])
            .connect()
            .await
            .unwrap();
        let (commands, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, messages) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = OKXWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            AuthTracker::new(),
            true,
            SubscriptionState::new(OKX_WS_TOPIC_DELIMITER),
            get_atomic_clock_realtime(),
        );
        client.set_auth_tracker(handler.auth_tracker.clone(), false);
        handler.inner = Some(Arc::new(client));
        handler.auth_tracker.succeed();

        HandlerPeer {
            handler,
            commands,
            frames,
            messages,
            server,
        }
    }

    fn order_send(payload: String, index: usize) -> PendingSend {
        PendingSend {
            payload,
            rate_limit_keys: None,
            request_id: Some(index.to_string()),
            client_order_ids: vec![ClientOrderId::from(format!("OQUEUED{index}").as_str())],
            op: Some(OKXWsOperation::CancelOrder),
            attempted: false,
        }
    }
}
