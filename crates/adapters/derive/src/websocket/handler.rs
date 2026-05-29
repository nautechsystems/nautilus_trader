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

//! Inner I/O feed handler for the Derive WebSocket transport.
//!
//! The handler owns the [`WebSocketClient`] exclusively and runs in a dedicated
//! Tokio task. The outer [`super::client::DeriveWebSocketClient`] talks to it
//! via a command channel and consumes a stream of [`DeriveWsMessage`] events.
//!
//! Each outbound JSON-RPC request is registered in a `pending` map keyed by the
//! correlator `id`. When the venue echoes the id on a response frame, the
//! matching oneshot is fulfilled with `result` or the JSON-RPC error.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use ahash::AHashMap;
use nautilus_network::{
    RECONNECTED,
    websocket::{AuthTracker, WebSocketClient},
};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use super::{
    error::DeriveWsError,
    messages::{DeriveWsChannel, DeriveWsFrame, WsSubscribeParams, WsSubscriptionPayload},
};
use crate::http::models::JsonRpcRequest;

/// Outbound commands the outer client sends to the inner handler.
#[derive(Debug)]
pub(super) enum HandlerCommand {
    /// Hand the active [`WebSocketClient`] to the handler.
    SetClient(WebSocketClient),
    /// Send a JSON-RPC request and resolve the oneshot when the venue replies.
    /// `params` is a pre-serialized `Value` so the handler stays agnostic to the
    /// per-method param types (login, subscribe, signed `private/*` bodies).
    Request {
        method: &'static str,
        params: Value,
        response_tx: tokio::sync::oneshot::Sender<Result<Value, DeriveWsError>>,
    },
    /// Gracefully tear down the WebSocket connection.
    Disconnect,
}

/// Events emitted by the handler for the outer client and downstream consumers.
#[derive(Debug, Clone)]
pub enum DeriveWsMessage {
    /// `public/login` succeeded. Consumed by the client's spawn loop to drive
    /// resubscription; not forwarded to data/execution layers.
    Authenticated,
    /// Underlying transport reconnected; outer client triggers re-login and
    /// resubscribes the tracked channels.
    Reconnected,
    /// Channel update pushed by the venue.
    Subscription(WsSubscriptionPayload),
}

/// Inner I/O loop. Lives in a Tokio task spawned by
/// [`super::client::DeriveWebSocketClient::connect`].
pub(super) struct FeedHandler {
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    next_id: Arc<AtomicU64>,
    pending: AHashMap<u64, tokio::sync::oneshot::Sender<Result<Value, DeriveWsError>>>,
    auth_tracker: AuthTracker,
}

impl FeedHandler {
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        next_id: Arc<AtomicU64>,
        auth_tracker: AuthTracker,
    ) -> Self {
        Self {
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            next_id,
            pending: AHashMap::new(),
            auth_tracker,
        }
    }

    /// Drains the next event from the underlying channels, processes it, and
    /// returns the resulting outbound message (if any). Returns `None` when
    /// the handler is shutting down or both channels closed.
    pub(super) async fn next(&mut self) -> Option<DeriveWsMessage> {
        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("Setting WebSocket client in Derive handler");
                            self.client = Some(client);
                        }
                        HandlerCommand::Request { method, params, response_tx } => {
                            self.dispatch_request(method, params, response_tx).await;
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Derive handler received disconnect command");
                            if let Some(ref client) = self.client {
                                client.disconnect().await;
                            }
                            self.signal.store(true, Ordering::SeqCst);
                            return None;
                        }
                    }
                }

                Some(raw) = self.raw_rx.recv() => {
                    match raw {
                        Message::Text(text) => {
                            if text.as_str() == RECONNECTED {
                                log::info!("Derive WebSocket reconnected sentinel received");
                                self.auth_tracker.invalidate();
                                self.fail_pending("WebSocket reconnected before response was received");
                                return Some(DeriveWsMessage::Reconnected);
                            }

                            match DeriveWsFrame::parse(&text) {
                                Ok(DeriveWsFrame::Response { id, result, error }) => {
                                    if let Some(sender) = self.pending.remove(&id) {
                                        let outcome = match (result, error) {
                                            (_, Some(err)) => Err(DeriveWsError::JsonRpc {
                                                code: err.code,
                                                message: err.message,
                                                data: err.data,
                                            }),
                                            (Some(value), None) => Ok(value),
                                            (None, None) => Ok(Value::Null),
                                        };
                                        let _ = sender.send(outcome);
                                    } else {
                                        log::debug!(
                                            "Derive WebSocket response with unknown id={id} dropped",
                                        );
                                    }
                                }
                                Ok(DeriveWsFrame::Subscription(payload)) => {
                                    return Some(DeriveWsMessage::Subscription(payload));
                                }
                                Ok(DeriveWsFrame::Unknown(value)) => {
                                    log::debug!("Derive WebSocket unknown frame: {value}");
                                }
                                Err(e) => {
                                    log::error!(
                                        "Derive WebSocket frame parse error: {e}, text: {text}",
                                    );
                                }
                            }
                        }
                        Message::Ping(data) => {
                            if let Some(ref client) = self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await {
                                log::error!("Derive WebSocket send_pong failed: {e}");
                            }
                        }
                        Message::Close(_) => {
                            log::info!("Derive WebSocket close frame received");
                            return None;
                        }
                        _ => {}
                    }
                }

                else => {
                    log::debug!("Derive handler shutting down: channels closed");
                    return None;
                }
            }
        }
    }

    async fn dispatch_request(
        &mut self,
        method: &'static str,
        params: Value,
        response_tx: tokio::sync::oneshot::Sender<Result<Value, DeriveWsError>>,
    ) {
        let Some(ref client) = self.client else {
            let _ = response_tx.send(Err(DeriveWsError::NotConnected));
            return;
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);
        let payload = match serde_json::to_string(&request) {
            Ok(p) => p,
            Err(e) => {
                let _ = response_tx.send(Err(DeriveWsError::Serde(e)));
                return;
            }
        };
        self.pending.insert(id, response_tx);
        log::debug!("Derive WebSocket sending `{method}` id={id}");
        if let Err(e) = client.send_text(payload, None).await
            && let Some(sender) = self.pending.remove(&id)
        {
            let _ = sender.send(Err(DeriveWsError::transport(e.to_string())));
        }
    }

    fn fail_pending(&mut self, reason: &str) {
        if self.pending.is_empty() {
            return;
        }
        log::debug!(
            "Failing {} pending Derive WebSocket request(s): {reason}",
            self.pending.len(),
        );

        for (_, sender) in self.pending.drain() {
            let _ = sender.send(Err(DeriveWsError::transport(reason.to_string())));
        }
    }
}

/// Builds `subscribe` params from a single channel topic.
#[must_use]
pub(super) fn subscribe_params(channel: DeriveWsChannel) -> WsSubscribeParams {
    WsSubscribeParams {
        channels: vec![channel],
    }
}

/// Convenience wrapper that produces the `subscribe` params for the
/// `ticker_slim.{instrument_name}.{interval}` channel.
#[must_use]
pub(super) fn ticker_subscribe_params(instrument_name: &str, interval: &str) -> WsSubscribeParams {
    subscribe_params(DeriveWsChannel::ticker_slim(instrument_name, interval))
}

/// Convenience wrapper that produces the `subscribe` params for the
/// `orderbook.{instrument_name}.{group}.{depth}` channel.
#[must_use]
pub(super) fn orderbook_subscribe_params(
    instrument_name: &str,
    group: &str,
    depth: &str,
) -> WsSubscribeParams {
    subscribe_params(DeriveWsChannel::orderbook(instrument_name, group, depth))
}

/// Convenience wrapper that produces the `subscribe` params for the
/// `trades.{instrument_type}.{currency}` channel.
#[must_use]
pub(super) fn trades_subscribe_params(instrument_type: &str, currency: &str) -> WsSubscribeParams {
    subscribe_params(DeriveWsChannel::trades(instrument_type, currency))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_subscribe_params_carries_single_channel() {
        let params = subscribe_params(DeriveWsChannel::ticker_slim("ETH-PERP", "1000"));
        assert_eq!(
            params.channels,
            vec![DeriveWsChannel::ticker_slim("ETH-PERP", "1000")],
        );
    }

    #[rstest]
    fn test_ticker_subscribe_params_formats_topic() {
        let params = ticker_subscribe_params("ETH-PERP", "1000");
        assert_eq!(
            params.channels,
            vec![DeriveWsChannel::ticker_slim("ETH-PERP", "1000")],
        );
    }

    #[rstest]
    fn test_orderbook_subscribe_params_formats_topic() {
        let params = orderbook_subscribe_params("ETH-PERP", "1", "10");
        assert_eq!(
            params.channels,
            vec![DeriveWsChannel::orderbook("ETH-PERP", "1", "10")],
        );
    }

    #[rstest]
    fn test_trades_subscribe_params_formats_topic() {
        let params = trades_subscribe_params("perp", "ETH");
        assert_eq!(
            params.channels,
            vec![DeriveWsChannel::trades("perp", "ETH")],
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_dispatch_request_without_client_returns_not_connected() {
        // Requests issued before SetClient must fail fast rather than hang.
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let next_id = Arc::new(AtomicU64::new(1));
        let auth_tracker = AuthTracker::new();
        let mut handler = FeedHandler::new(signal, cmd_rx, raw_rx, next_id, auth_tracker);

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let params = serde_json::to_value(WsSubscribeParams { channels: vec![] }).unwrap();
        handler
            .dispatch_request("public/login", params, response_tx)
            .await;

        let outcome = response_rx.await.expect("oneshot resolved");
        match outcome {
            Err(DeriveWsError::NotConnected) => {}
            other => panic!("expected NotConnected, was {other:?}"),
        }
    }
}
