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

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use nautilus_core::string::secret::SecretString;
use nautilus_live::task::{SharedTaskSlot, TaskJoinOutcome};
use nautilus_network::{
    RECONNECTED,
    websocket::{AuthTracker, WebSocketClient},
};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use super::{
    client::UNAUTHENTICATED_CONNECTION_EPOCH,
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
    /// `params` is pre-serialized JSON so the handler stays agnostic to the
    /// per-method param types (login, subscribe, signed `private/*` bodies).
    Request {
        method: &'static str,
        params: SecretString,
        connection_epoch: Option<u64>,
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
    /// Re-login or subscription replay exhausted its retry budget.
    SessionRecoveryFailed(String),
    /// Channel update pushed by the venue.
    Subscription(WsSubscriptionPayload),
}

#[derive(Debug)]
struct SendCommand {
    id: u64,
    token: u64,
    connection_epoch: u64,
    payload: SecretString,
}

#[derive(Debug)]
struct SendFailure {
    id: u64,
    token: u64,
    reason: String,
}

#[derive(Debug)]
struct PendingRequest {
    token: u64,
    response_tx: tokio::sync::oneshot::Sender<Result<Value, DeriveWsError>>,
}

/// Inner I/O loop. Lives in a Tokio task spawned by
/// [`super::client::DeriveWebSocketClient::connect`].
pub(super) struct FeedHandler {
    signal: Arc<AtomicBool>,
    client: Option<Arc<WebSocketClient>>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    cmd_closed: bool,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    raw_closed: bool,
    next_id: Arc<AtomicU64>,
    next_send_token: u64,
    pending: AHashMap<u64, PendingRequest>,
    send_tx: Option<tokio::sync::mpsc::UnboundedSender<SendCommand>>,
    send_failure_rx: tokio::sync::mpsc::UnboundedReceiver<SendFailure>,
    send_task: Arc<SharedTaskSlot<()>>,
    auth_tracker: AuthTracker,
    authenticated_epoch: Arc<AtomicU64>,
}

impl FeedHandler {
    pub(super) fn new_with_send_task(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        next_id: Arc<AtomicU64>,
        auth_tracker: AuthTracker,
        authenticated_epoch: Arc<AtomicU64>,
        send_task: Arc<SharedTaskSlot<()>>,
    ) -> Self {
        let (_, send_failure_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            signal,
            client: None,
            cmd_rx,
            cmd_closed: false,
            raw_rx,
            raw_closed: false,
            next_id,
            next_send_token: 0,
            pending: AHashMap::new(),
            send_tx: None,
            send_failure_rx,
            send_task,
            auth_tracker,
            authenticated_epoch,
        }
    }

    /// Drains the next event from the underlying channels, processes it, and
    /// returns the resulting outbound message (if any). Returns `None` when
    /// the handler is shutting down or both channels closed.
    pub(super) async fn next(&mut self) -> Option<DeriveWsMessage> {
        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv(), if !self.cmd_closed => {
                    match cmd {
                        None => {
                            self.cmd_closed = true;

                            if self.raw_closed {
                                self.shutdown_send_path(
                                    "WebSocket handler stopped before response was received",
                                )
                                .await;
                                return None;
                            }
                        }
                        Some(HandlerCommand::SetClient(client)) => {
                            log::debug!("Setting WebSocket client in Derive handler");
                            let client = Arc::new(client);
                            self.stop_send_worker().await;
                            self.start_send_worker(Arc::clone(&client));
                            self.client = Some(client);
                        }
                        Some(HandlerCommand::Request {
                            method,
                            params,
                            connection_epoch,
                            response_tx,
                        }) => {
                            self.dispatch_request(method, params, connection_epoch, response_tx);
                        }
                        Some(HandlerCommand::Disconnect) => {
                            log::debug!("Derive handler received disconnect command");
                            self.shutdown_send_path(
                                "WebSocket disconnected before response was received",
                            )
                            .await;

                            if let Some(ref client) = self.client {
                                client.disconnect().await;
                            }
                            self.signal.store(true, Ordering::SeqCst);
                            return None;
                        }
                    }
                }

                raw = self.raw_rx.recv(), if !self.raw_closed => {
                    match raw {
                        None => {
                            self.raw_closed = true;

                            if self.cmd_closed {
                                self.shutdown_send_path(
                                    "WebSocket handler stopped before response was received",
                                )
                                .await;
                                return None;
                            }
                        }
                        Some(Message::Text(text)) => {
                            if text.as_str() == RECONNECTED {
                                log::info!("Derive WebSocket reconnected sentinel received");
                                self.auth_tracker.invalidate();
                                self.authenticated_epoch.store(
                                    UNAUTHENTICATED_CONNECTION_EPOCH,
                                    Ordering::Release,
                                );
                                self.restart_send_worker(
                                    "WebSocket reconnected before response was received",
                                )
                                .await;
                                return Some(DeriveWsMessage::Reconnected);
                            }

                            match DeriveWsFrame::parse(&text) {
                                Ok(DeriveWsFrame::Response { id, result, error }) => {
                                    if let Some(pending) = self.pending.remove(&id) {
                                        let outcome = match (result, error) {
                                            (_, Some(err)) => Err(DeriveWsError::JsonRpc {
                                                code: err.code,
                                                message: err.message,
                                                data: err.data,
                                            }),
                                            (Some(value), None) => Ok(value),
                                            (None, None) => Ok(Value::Null),
                                        };
                                        let _ = pending.response_tx.send(outcome);
                                    } else {
                                        log::debug!(
                                            "Derive WebSocket response with unknown id={id} dropped",
                                        );
                                    }
                                }
                                Ok(DeriveWsFrame::Subscription(payload)) => {
                                    return Some(DeriveWsMessage::Subscription(payload));
                                }
                                Ok(DeriveWsFrame::UncorrelatedError(error)) => {
                                    self.fail_uncorrelated_error(error);
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
                        Some(Message::Ping(data)) => {
                            if let Some(ref client) = self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await {
                                log::error!("Derive WebSocket send_pong failed: {e}");
                            }
                        }
                        Some(Message::Close(_)) => {
                            log::debug!("Derive WebSocket close frame received");
                            self.shutdown_send_path(
                                "WebSocket closed before response was received",
                            )
                            .await;
                            return None;
                        }
                        Some(_) => {}
                    }
                }

                Some(failure) = self.send_failure_rx.recv() => {
                    self.handle_send_failure(failure);
                }

                else => {
                    log::debug!("Derive handler shutting down: channels closed");
                    self.shutdown_send_path(
                        "WebSocket handler stopped before response was received",
                    )
                    .await;
                    return None;
                }
            }
        }
    }

    fn dispatch_request(
        &mut self,
        method: &'static str,
        params: SecretString,
        connection_epoch: Option<u64>,
        response_tx: tokio::sync::oneshot::Sender<Result<Value, DeriveWsError>>,
    ) {
        let Some(client) = self.client.as_ref() else {
            let _ = response_tx.send(Err(DeriveWsError::NotConnected));
            return;
        };
        let current_epoch = client.connection_epoch();
        let connection_epoch = if method.starts_with("private/") {
            let authenticated_epoch = self.authenticated_epoch.load(Ordering::Acquire);
            if !client.connection_mode().is_active() || authenticated_epoch != current_epoch {
                let _ = response_tx.send(Err(DeriveWsError::Authentication {
                    operation: method.to_string(),
                    reason: "WebSocket session is not authenticated".to_string(),
                }));
                return;
            }
            authenticated_epoch
        } else {
            connection_epoch.unwrap_or(current_epoch)
        };

        if !client.connection_mode().is_active() || connection_epoch != current_epoch {
            let _ = response_tx.send(Err(DeriveWsError::transport(format!(
                "connection changed before `{method}` was sent",
            ))));
            return;
        }

        self.enqueue_request(method, params, connection_epoch, response_tx);
    }

    fn enqueue_request(
        &mut self,
        method: &'static str,
        params_json: SecretString,
        connection_epoch: u64,
        response_tx: tokio::sync::oneshot::Sender<Result<Value, DeriveWsError>>,
    ) {
        let Some(send_tx) = self.send_tx.clone() else {
            let _ = response_tx.send(Err(DeriveWsError::NotConnected));
            return;
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let token = self.next_send_token;
        self.next_send_token = self.next_send_token.wrapping_add(1);
        let params: Value = match serde_json::from_str(params_json.expose_secret()) {
            Ok(params) => params,
            Err(e) => {
                let _ = response_tx.send(Err(DeriveWsError::Serde(e)));
                return;
            }
        };
        drop(params_json);
        let request = JsonRpcRequest::new(id, method, params);
        let payload = match serde_json::to_string(&request) {
            Ok(payload) => SecretString::from(payload),
            Err(e) => {
                let _ = response_tx.send(Err(DeriveWsError::Serde(e)));
                return;
            }
        };
        self.pending
            .insert(id, PendingRequest { token, response_tx });
        log::debug!("Derive WebSocket sending `{method}` id={id}");
        if let Err(e) = send_tx.send(SendCommand {
            id,
            token,
            connection_epoch,
            payload,
        }) && self
            .pending
            .get(&id)
            .is_some_and(|pending| pending.token == token)
            && let Some(pending) = self.pending.remove(&id)
        {
            let _ = pending
                .response_tx
                .send(Err(DeriveWsError::transport(format!(
                    "failed to queue WebSocket request: {e}",
                ))));
        }
    }

    fn start_send_worker(&mut self, client: Arc<WebSocketClient>) {
        if !self.send_task.is_empty() {
            log::error!("Cannot start Derive WebSocket send worker while the prior task is owned");
            return;
        }

        let (send_tx, send_rx) = tokio::sync::mpsc::unbounded_channel();
        let (failure_tx, failure_rx) = tokio::sync::mpsc::unbounded_channel();
        self.send_tx = Some(send_tx);
        self.send_failure_rx = failure_rx;

        if let Err(e) = self
            .send_task
            .spawn(run_send_worker(client, send_rx, failure_tx))
        {
            self.send_tx.take();
            log::error!("Failed to start Derive WebSocket send worker: {e}");
        }
    }

    async fn stop_send_worker(&mut self) {
        self.send_tx.take();

        if let Some(outcome) = self
            .send_task
            .finish(Duration::from_secs(1), Duration::from_secs(2))
            .await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                TaskJoinOutcome::Failed(e) => {
                    log::error!("Derive WebSocket send worker failed: {e}");
                }
                TaskJoinOutcome::Incomplete => {
                    log::error!("Derive WebSocket send worker did not stop after abort");
                }
            }
        }
    }

    async fn restart_send_worker(&mut self, reason: &str) {
        self.stop_send_worker().await;
        self.fail_pending(reason);

        if let Some(client) = self.client.clone() {
            self.start_send_worker(client);
        }
    }

    async fn shutdown_send_path(&mut self, reason: &str) {
        self.stop_send_worker().await;
        self.fail_pending(reason);
    }

    fn handle_send_failure(&mut self, failure: SendFailure) {
        if !self
            .pending
            .get(&failure.id)
            .is_some_and(|pending| pending.token == failure.token)
        {
            return;
        }

        if let Some(pending) = self.pending.remove(&failure.id) {
            let _ = pending
                .response_tx
                .send(Err(DeriveWsError::transport(failure.reason)));
        }
    }

    fn fail_uncorrelated_error(&mut self, error: crate::http::models::JsonRpcError) {
        if self.pending.len() != 1 {
            log::warn!(
                "Derive WebSocket uncorrelated JSON-RPC error with {} pending requests: code={}, message={}",
                self.pending.len(),
                error.code,
                error.message,
            );
            return;
        }

        if let Some((_, pending)) = self.pending.drain().next() {
            let _ = pending.response_tx.send(Err(DeriveWsError::JsonRpc {
                code: error.code,
                message: error.message,
                data: error.data,
            }));
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

        for (_, pending) in self.pending.drain() {
            let _ = pending
                .response_tx
                .send(Err(DeriveWsError::transport(reason.to_string())));
        }
    }
}

impl Drop for FeedHandler {
    fn drop(&mut self) {
        self.send_tx.take();
        self.send_task.abort();
    }
}

async fn run_send_worker(
    client: Arc<WebSocketClient>,
    mut send_rx: tokio::sync::mpsc::UnboundedReceiver<SendCommand>,
    failure_tx: tokio::sync::mpsc::UnboundedSender<SendFailure>,
) {
    while let Some(command) = send_rx.recv().await {
        if let Err(e) = client
            .send_text_on_connection(
                command.payload.expose_secret().to_owned(),
                None,
                command.connection_epoch,
            )
            .await
            && failure_tx
                .send(SendFailure {
                    id: command.id,
                    token: command.token,
                    reason: e.to_string(),
                })
                .is_err()
        {
            return;
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
    use nautilus_core::string::secret::zeroize_json_value;
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    fn unauthenticated_epoch() -> Arc<AtomicU64> {
        Arc::new(AtomicU64::new(u64::MAX))
    }

    fn secret_json(mut value: Value) -> SecretString {
        let serialized = SecretString::from(value.to_string());
        zeroize_json_value(&mut value);
        serialized
    }

    #[rstest]
    fn test_queued_request_debug_redacts_payload() {
        let secret = "signature-secret";
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
        let command = HandlerCommand::Request {
            method: "public/login",
            params: SecretString::from(format!(r#"{{"signature":"{secret}"}}"#)),
            connection_epoch: None,
            response_tx,
        };
        let send = SendCommand {
            id: 1,
            token: 1,
            connection_epoch: 1,
            payload: SecretString::from(format!(r#"{{"signature":"{secret}"}}"#)),
        };

        let debug = format!("{command:?} {send:?}");

        assert!(!debug.contains(secret));
    }

    fn feed_handler(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        next_id: Arc<AtomicU64>,
        auth_tracker: AuthTracker,
        authenticated_epoch: Arc<AtomicU64>,
    ) -> FeedHandler {
        FeedHandler::new_with_send_task(
            signal,
            cmd_rx,
            raw_rx,
            next_id,
            auth_tracker,
            authenticated_epoch,
            Arc::new(SharedTaskSlot::new()),
        )
    }

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
        let mut handler = feed_handler(
            signal,
            cmd_rx,
            raw_rx,
            next_id,
            auth_tracker,
            unauthenticated_epoch(),
        );

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let params =
            secret_json(serde_json::to_value(WsSubscribeParams { channels: vec![] }).unwrap());
        handler.dispatch_request("public/login", params, None, response_tx);

        let outcome = response_rx.await.expect("oneshot resolved");
        match outcome {
            Err(DeriveWsError::NotConnected) => {}
            other => panic!("expected NotConnected, was {other:?}"),
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_dispatch_registers_pending_requests_before_ordered_queueing() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let next_id = Arc::new(AtomicU64::new(1));
        let auth_tracker = AuthTracker::new();
        let mut handler = feed_handler(
            signal,
            cmd_rx,
            raw_rx,
            next_id,
            auth_tracker,
            unauthenticated_epoch(),
        );
        let (send_tx, mut send_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.send_tx = Some(send_tx);

        let (first_tx, _first_rx) = tokio::sync::oneshot::channel();
        let (second_tx, _second_rx) = tokio::sync::oneshot::channel();
        handler.enqueue_request("first", secret_json(json!({"sequence": 1})), 0, first_tx);
        handler.enqueue_request("second", secret_json(json!({"sequence": 2})), 0, second_tx);

        let first = send_rx.recv().await.expect("first queued send");
        let second = send_rx.recv().await.expect("second queued send");
        let first_payload: Value = serde_json::from_str(first.payload.expose_secret()).unwrap();
        let second_payload: Value = serde_json::from_str(second.payload.expose_secret()).unwrap();

        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
        assert_eq!(first_payload["method"], "first");
        assert_eq!(second_payload["method"], "second");
        assert_eq!(handler.pending.get(&first.id).unwrap().token, first.token);
        assert_eq!(handler.pending.get(&second.id).unwrap().token, second.token);
    }

    #[rstest]
    #[tokio::test]
    async fn test_late_send_failure_does_not_remove_reused_request_id() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let next_id = Arc::new(AtomicU64::new(1));
        let auth_tracker = AuthTracker::new();
        let mut handler = feed_handler(
            signal,
            cmd_rx,
            raw_rx,
            Arc::clone(&next_id),
            auth_tracker,
            unauthenticated_epoch(),
        );
        let (send_tx, mut send_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.send_tx = Some(send_tx);

        let (old_tx, old_rx) = tokio::sync::oneshot::channel();
        handler.enqueue_request("first", secret_json(json!({})), 0, old_tx);
        let old_send = send_rx.recv().await.expect("old queued send");
        let old_pending = handler.pending.remove(&old_send.id).unwrap();
        old_pending.response_tx.send(Ok(Value::Null)).unwrap();
        old_rx.await.unwrap().unwrap();

        next_id.store(old_send.id, Ordering::Relaxed);
        let (new_tx, new_rx) = tokio::sync::oneshot::channel();
        handler.enqueue_request("second", secret_json(json!({})), 0, new_tx);
        let new_send = send_rx.recv().await.expect("new queued send");
        handler.handle_send_failure(SendFailure {
            id: old_send.id,
            token: old_send.token,
            reason: "late failure".to_string(),
        });

        assert_eq!(old_send.id, new_send.id);
        assert_ne!(old_send.token, new_send.token);
        assert_eq!(
            handler.pending.get(&new_send.id).unwrap().token,
            new_send.token,
        );

        handler.handle_send_failure(SendFailure {
            id: new_send.id,
            token: new_send.token,
            reason: "current failure".to_string(),
        });
        let error = new_rx.await.unwrap().expect_err("current request failed");
        assert!(error.to_string().contains("current failure"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_shutdown_aborts_send_worker_and_drains_pending_requests() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let next_id = Arc::new(AtomicU64::new(1));
        let auth_tracker = AuthTracker::new();
        let mut handler = feed_handler(
            signal,
            cmd_rx,
            raw_rx,
            next_id,
            auth_tracker,
            unauthenticated_epoch(),
        );
        let (send_tx, _send_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.send_tx = Some(send_tx);

        let task = tokio::spawn(std::future::pending::<()>());
        let abort_handle = task.abort_handle();
        handler.send_task.insert(task);

        let (first_tx, first_rx) = tokio::sync::oneshot::channel();
        let (second_tx, second_rx) = tokio::sync::oneshot::channel();
        handler.enqueue_request("first", secret_json(json!({})), 0, first_tx);
        handler.enqueue_request("second", secret_json(json!({})), 0, second_tx);
        handler.shutdown_send_path("disconnect requested").await;
        tokio::task::yield_now().await;

        let first_error = first_rx.await.unwrap().expect_err("first request failed");
        let second_error = second_rx.await.unwrap().expect_err("second request failed");
        assert!(abort_handle.is_finished());
        assert!(handler.send_tx.is_none());
        assert!(handler.send_task.is_empty());
        assert!(handler.pending.is_empty());
        assert!(first_error.to_string().contains("disconnect requested"));
        assert!(second_error.to_string().contains("disconnect requested"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_next_stops_when_input_channels_close_with_failure_channel_open() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let next_id = Arc::new(AtomicU64::new(1));
        let auth_tracker = AuthTracker::new();
        let mut handler = feed_handler(
            signal,
            cmd_rx,
            raw_rx,
            next_id,
            auth_tracker,
            unauthenticated_epoch(),
        );
        let (_failure_tx, failure_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.send_failure_rx = failure_rx;
        drop(cmd_tx);
        drop(raw_tx);

        let outcome = tokio::time::timeout(std::time::Duration::from_millis(100), handler.next())
            .await
            .expect("handler stopped after both input channels closed");

        assert!(outcome.is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn test_uncorrelated_error_fails_only_pending_request() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let next_id = Arc::new(AtomicU64::new(1));
        let auth_tracker = AuthTracker::new();
        let mut handler = feed_handler(
            signal,
            cmd_rx,
            raw_rx,
            next_id,
            auth_tracker,
            unauthenticated_epoch(),
        );
        let (send_tx, _send_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.send_tx = Some(send_tx);
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        handler.enqueue_request("request", secret_json(json!({})), 0, response_tx);

        handler.fail_uncorrelated_error(crate::http::models::JsonRpcError {
            code: -32700,
            message: "Parse error".to_string(),
            data: Some(json!("invalid JSON")),
        });
        let error = response_rx.await.unwrap().expect_err("request failed");

        match error {
            DeriveWsError::JsonRpc {
                code,
                message,
                data,
            } => {
                assert_eq!(code, -32700);
                assert_eq!(message, "Parse error");
                assert_eq!(data, Some(json!("invalid JSON")));
            }
            other => panic!("expected JsonRpc, was {other:?}"),
        }
        assert!(handler.pending.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_uncorrelated_error_does_not_guess_between_pending_requests() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let next_id = Arc::new(AtomicU64::new(1));
        let auth_tracker = AuthTracker::new();
        let mut handler = feed_handler(
            signal,
            cmd_rx,
            raw_rx,
            next_id,
            auth_tracker,
            unauthenticated_epoch(),
        );
        let (send_tx, _send_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.send_tx = Some(send_tx);
        let (first_tx, first_rx) = tokio::sync::oneshot::channel();
        let (second_tx, second_rx) = tokio::sync::oneshot::channel();
        handler.enqueue_request("first", secret_json(json!({})), 0, first_tx);
        handler.enqueue_request("second", secret_json(json!({})), 0, second_tx);

        handler.fail_uncorrelated_error(crate::http::models::JsonRpcError {
            code: -32600,
            message: "Invalid Request".to_string(),
            data: None,
        });

        assert_eq!(handler.pending.len(), 2);
        handler.fail_pending("test cleanup");
        assert!(first_rx.await.unwrap().is_err());
        assert!(second_rx.await.unwrap().is_err());
    }
}
