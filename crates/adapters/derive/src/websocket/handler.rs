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
use nautilus_common::live::dst::task;
use nautilus_core::string::secret::{SecretString, zeroize_json_value};
use nautilus_live::task::{SharedTaskSlot, TaskJoinOutcome};
use nautilus_network::{
    RECONNECTED,
    error::SendError,
    websocket::{AuthTracker, WebSocketClient},
};
use serde_json::{Value, value::RawValue};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::{
    client::UNAUTHENTICATED_CONNECTION_EPOCH,
    error::DeriveWsError,
    messages::{DeriveWsChannel, ParsedFrame, WsSubscribeParams, WsSubscriptionPayload},
};
use crate::{
    book::sync::{BookSyncTracker, BookWrite},
    http::models::JsonRpcRequest,
};

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
        cancellation: Option<CancellationToken>,
        response_tx: tokio::sync::oneshot::Sender<Result<Box<RawValue>, DeriveWsError>>,
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
    /// Book update admitted at ingress with its subscription generation.
    BookSubscription {
        /// Channel update pushed by the venue.
        payload: WsSubscriptionPayload,
        /// Cancelled when replacement, unsubscribe, or reconnect obsoletes the frame.
        generation: CancellationToken,
    },
}

#[derive(Debug)]
struct SendCommand {
    id: u64,
    token: u64,
    connection_epoch: u64,
    payload: SecretString,
    cancellation: Option<CancellationToken>,
    book_generations: Vec<CancellationToken>,
}

#[derive(Debug)]
struct SendFailure {
    id: u64,
    token: u64,
    error: SendError,
}

#[derive(Debug)]
struct PendingRequest {
    token: u64,
    cancellation: Option<CancellationToken>,
    book_writes: Vec<(String, BookWrite)>,
    response_tx: tokio::sync::oneshot::Sender<Result<Box<RawValue>, DeriveWsError>>,
}

/// Inner I/O loop. Lives in a Tokio task spawned by
/// [`super::client::DeriveWebSocketClient::connect`].
pub(super) struct FeedHandler {
    signal: Arc<AtomicBool>,
    book_sync: Option<(BookSyncTracker, Duration)>,
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
            book_sync: None,
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

    pub(super) fn with_book_sync(mut self, book_sync: Option<(BookSyncTracker, Duration)>) -> Self {
        self.book_sync = book_sync;
        self
    }

    /// Drains the next event from the underlying channels, processes it, and
    /// returns the resulting outbound message (if any). Returns `None` when
    /// the handler is shutting down or both channels closed.
    pub(super) async fn next(&mut self) -> Option<DeriveWsMessage> {
        let mut command_ready = true;

        loop {
            tokio::select! {
                biased;
                Some(failure) = self.send_failure_rx.recv() => {
                    self.handle_send_failure(&failure);
                }
                cmd = self.cmd_rx.recv(), if !self.cmd_closed && command_ready => {
                    command_ready = false;

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
                            cancellation,
                            response_tx,
                        }) => {
                            self.dispatch_request(method, params, connection_epoch, cancellation, response_tx);
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
                    command_ready = true;

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

                                if let Some((tracker, timeout)) = &self.book_sync {
                                    tracker.reset_on_reconnect(*timeout);
                                }
                                return Some(DeriveWsMessage::Reconnected);
                            }

                            match ParsedFrame::parse(&text) {
                                Ok(ParsedFrame::Response { id, result, error }) => {
                                    if let Some(pending) = self.pending.remove(&id) {
                                        let outcome = match (result, error) {
                                            (_, Some(err)) => Err(DeriveWsError::JsonRpc {
                                                code: err.code,
                                                message: err.message,
                                                data: err.data,
                                            }),
                                            (Some(value), None) => Ok(value),
                                            (None, None) => Ok(RawValue::NULL.to_owned()),
                                        };

                                        if let Ok(value) = &outcome
                                            && let Some((tracker, _)) = &self.book_sync {
                                            open_book_gates(&pending, value, tracker);
                                        }
                                        let _ = pending.response_tx.send(outcome);
                                    } else {
                                        log::debug!(
                                            "Derive WebSocket response with unknown id={id} dropped",
                                        );
                                    }
                                }
                                Ok(ParsedFrame::Subscription(payload)) => {
                                    if let Some((tracker, _)) = &self.book_sync
                                        && payload.channel.as_str().starts_with("orderbook.") {
                                        let Some(write) = tracker.write(payload.channel.as_str()) else {
                                            continue;
                                        };

                                        if write.generation.is_cancelled() || write.gate.lock().is_closed() {
                                            continue;
                                        }
                                        return Some(DeriveWsMessage::BookSubscription {
                                            payload,
                                            generation: write.generation,
                                        });
                                    }
                                    return Some(DeriveWsMessage::Subscription(payload));
                                }
                                Ok(ParsedFrame::UncorrelatedError(error)) => {
                                    self.fail_uncorrelated_error(error);
                                }
                                Ok(ParsedFrame::Unknown(_)) => {
                                    log::debug!("Derive WebSocket unknown frame, bytes={}", text.len());
                                }
                                Err(e) => {
                                    log::error!(
                                        "Derive WebSocket frame parse error: {:?}, line={}, column={}, bytes={}",
                                        e.classify(),
                                        e.line(),
                                        e.column(),
                                        text.len(),
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

                () = task::yield_now(), if !command_ready => command_ready = true,

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
        cancellation: Option<CancellationToken>,
        response_tx: tokio::sync::oneshot::Sender<Result<Box<RawValue>, DeriveWsError>>,
    ) {
        if cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            let _ = response_tx.send(Err(DeriveWsError::NotConnected));
            return;
        }

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
            let _ = response_tx.send(Err(DeriveWsError::NotConnected));
            return;
        }

        self.enqueue_request(method, params, connection_epoch, cancellation, response_tx);
    }

    fn enqueue_request(
        &mut self,
        method: &'static str,
        params_json: SecretString,
        connection_epoch: u64,
        cancellation: Option<CancellationToken>,
        response_tx: tokio::sync::oneshot::Sender<Result<Box<RawValue>, DeriveWsError>>,
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

        let book_writes = self
            .book_sync
            .as_ref()
            .map_or_else(Vec::new, |(tracker, _)| {
                params
                    .get("channels")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter_map(|channel| {
                        tracker
                            .write(channel)
                            .map(|write| (channel.to_owned(), write))
                    })
                    .collect::<Vec<_>>()
            });

        let book_generations = if cancellation.is_some() {
            book_writes
                .iter()
                .map(|(_, write)| write.generation.clone())
                .collect()
        } else {
            Vec::new()
        };

        let book_writes = if method == "subscribe" {
            book_writes
        } else {
            Vec::new()
        };

        let mut request = JsonRpcRequest::new(id, method, params);
        let serialized = serde_json::to_string(&request);
        zeroize_json_value(&mut request.params);

        let payload = match serialized {
            Ok(payload) => SecretString::from(payload),
            Err(e) => {
                let _ = response_tx.send(Err(DeriveWsError::Serde(e)));
                return;
            }
        };

        self.pending.insert(
            id,
            PendingRequest {
                token,
                cancellation: cancellation.clone(),
                book_writes,
                response_tx,
            },
        );

        log::debug!("Derive WebSocket sending `{method}` id={id}");

        if send_tx
            .send(SendCommand {
                id,
                token,
                connection_epoch,
                payload,
                cancellation,
                book_generations,
            })
            .is_err()
            && self
                .pending
                .get(&id)
                .is_some_and(|pending| pending.token == token)
            && let Some(pending) = self.pending.remove(&id)
        {
            let _ = pending.response_tx.send(Err(DeriveWsError::NotConnected));
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

    fn handle_send_failure(&mut self, failure: &SendFailure) {
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
                .send(Err(send_error_to_derive(&failure.error)));
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
        let result = tokio::select! {
            biased;
            () = async {
                if let Some(cancel) = &command.cancellation {
                    cancel.cancelled().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => Err(SendError::Closed),
            result = async {
                if command.book_generations.iter().any(CancellationToken::is_cancelled) {
                    return Err(SendError::Closed);
                }
                client.send_text_on_connection(
                    command.payload.expose_secret().to_owned(),
                    None,
                    command.connection_epoch,
                ).await
            } => result,
        };

        if let Err(e) = result
            && failure_tx
                .send(SendFailure {
                    id: command.id,
                    token: command.token,
                    error: e,
                })
                .is_err()
        {
            return;
        }
    }
}

fn open_book_gates(pending: &PendingRequest, value: &RawValue, tracker: &BookSyncTracker) {
    let is_pending = || {
        !pending.response_tx.is_closed()
            && !pending
                .cancellation
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
    };

    if pending.book_writes.is_empty() || !is_pending() {
        return;
    }

    let Ok(result) = serde_json::from_str::<super::messages::WsSubscribeResult>(value.get()) else {
        return;
    };

    for (channel, write) in &pending.book_writes {
        let topic = DeriveWsChannel::from(channel.clone());

        if result
            .status
            .get(&topic)
            .is_some_and(|status| matches!(status.as_str(), "ok" | "already subscribed"))
            && result.channels.contains(&topic)
        {
            tracker.confirm_subscription(write, is_pending);
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

fn send_error_to_derive(error: &SendError) -> DeriveWsError {
    match error {
        SendError::InvalidInput(_)
        | SendError::BufferFull
        | SendError::Closed
        | SendError::Timeout
        | SendError::ConnectionChanged => {
            log::warn!("Derive WebSocket request was not sent: {error}");
            DeriveWsError::NotConnected
        }
        SendError::BrokenPipe(_) | SendError::WriteTimeout => {
            DeriveWsError::transport(error.to_string())
        }
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
    use futures_util::StreamExt;
    use nautilus_model::identifiers::InstrumentId;
    use nautilus_network::websocket::WebSocketConfig;
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
    #[case(SendError::InvalidInput("invalid".to_string()), false)]
    #[case(SendError::BufferFull, false)]
    #[case(SendError::Closed, false)]
    #[case(SendError::Timeout, false)]
    #[case(SendError::ConnectionChanged, false)]
    #[case(SendError::BrokenPipe("lost connection".to_string()), true)]
    #[case(SendError::WriteTimeout, true)]
    fn test_send_error_preserves_write_ambiguity(
        #[case] error: SendError,
        #[case] ambiguous: bool,
    ) {
        let reason = error.to_string();
        let error = send_error_to_derive(&error);
        assert_eq!(
            crate::common::retry::is_write_outcome_ambiguous_ws(&error),
            ambiguous
        );

        if ambiguous {
            let DeriveWsError::Transport(message) = error else {
                panic!("expected ambiguous transport error: {error:?}");
            };

            assert_eq!(message, reason);
        } else {
            assert!(matches!(error, DeriveWsError::NotConnected));
        }
    }

    #[rstest]
    fn test_queued_request_debug_redacts_payload() {
        let secret = "signature-secret";
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel();

        let command = HandlerCommand::Request {
            method: "public/login",
            params: SecretString::from(format!(r#"{{"signature":"{secret}"}}"#)),
            connection_epoch: None,
            cancellation: None,
            response_tx,
        };

        let send = SendCommand {
            cancellation: None,
            book_generations: Vec::new(),
            id: 1,
            token: 1,
            connection_epoch: 1,
            payload: SecretString::from(format!(r#"{{"signature":"{secret}"}}"#)),
        };

        let debug = format!("{command:?} {send:?}");

        assert!(!debug.contains(secret));
    }

    #[rstest]
    #[case::episode(true)]
    #[case::generation(false)]
    #[tokio::test]
    async fn cancelled_book_writes_never_reach_socket(#[case] episode: bool) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let mut messages = Vec::new();

            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Text(text) => messages.push(text.to_string()),
                    Message::Close(_) => break,
                    _ => {}
                }
            }

            messages
        });

        let config = WebSocketConfig::builder()
            .url(format!("ws://{address}"))
            .build()
            .unwrap();

        let client = Arc::new(
            WebSocketClient::builder()
                .config(config)
                .message_handler(Arc::new(|_| {}))
                .connect()
                .await
                .unwrap(),
        );
        let cancel = CancellationToken::new();
        let generation = CancellationToken::new();

        if episode {
            cancel.cancel();
        } else {
            generation.cancel();
        }

        let (send_tx, send_rx) = tokio::sync::mpsc::unbounded_channel();
        let (failure_tx, mut failure_rx) = tokio::sync::mpsc::unbounded_channel();
        send_tx
            .send(SendCommand {
                id: 17,
                token: 29,
                connection_epoch: client.connection_epoch(),
                payload: SecretString::from("cancelled"),
                cancellation: Some(cancel),
                book_generations: vec![generation],
            })
            .unwrap();

        send_tx
            .send(SendCommand {
                id: 31,
                token: 43,
                connection_epoch: client.connection_epoch(),
                payload: SecretString::from("current"),
                cancellation: None,
                book_generations: Vec::new(),
            })
            .unwrap();

        drop(send_tx);

        run_send_worker(Arc::clone(&client), send_rx, failure_tx).await;
        let failure = failure_rx.try_recv().unwrap();
        client.disconnect().await;
        let messages = tokio::time::timeout(std::time::Duration::from_secs(3), peer)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(failure.id, 17);
        assert_eq!(failure.token, 29);
        assert!(matches!(failure.error, SendError::Closed));
        assert!(failure_rx.try_recv().is_err());
        assert_eq!(messages, ["current"]);
    }

    #[rstest]
    #[case::tracked(true)]
    #[case::legacy(false)]
    #[tokio::test]
    async fn unowned_book_frames_are_dropped_only_on_tracked_streams(#[case] tracked: bool) {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handler = feed_handler(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            Arc::new(AtomicU64::new(1)),
            AuthTracker::default(),
            unauthenticated_epoch(),
        )
        .with_book_sync(tracked.then(|| (BookSyncTracker::default(), Duration::from_secs(10))));
        let data: Value =
            serde_json::from_str(include_str!("../../test_data/spot/ws_orderbook_eth.json"))
                .unwrap();
        let channel = "orderbook.ETH-USDC.1.10";
        raw_tx
            .send(Message::Text(
                json!({"method": "subscription", "params": {"channel": channel, "data": data}})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        raw_tx.send(Message::Text(RECONNECTED.into())).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), handler.next())
            .await
            .unwrap()
            .unwrap();

        if tracked {
            assert!(matches!(event, DeriveWsMessage::Reconnected));
        } else {
            let DeriveWsMessage::Subscription(payload) = event else {
                panic!("expected legacy book event");
            };

            assert_eq!(payload.channel.as_str(), channel);
            assert_eq!(
                serde_json::from_str::<Value>(payload.data.get()).unwrap(),
                data
            );
        }
    }

    #[rstest]
    #[tokio::test]
    async fn book_ack_opens_gate_at_ingress_and_drops_pre_ack_frames() {
        let tracker = BookSyncTracker::default();
        let channel = "orderbook.ETH-USDC.1.10";
        tracker.record_subscription(channel, InstrumentId::from("ETH-USDC.DERIVE"));
        let write = tracker.write(channel).unwrap();
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handler = feed_handler(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            Arc::new(AtomicU64::new(1)),
            AuthTracker::default(),
            Arc::new(AtomicU64::new(UNAUTHENTICATED_CONNECTION_EPOCH)),
        )
        .with_book_sync(Some((tracker, Duration::from_secs(10))));
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        handler.pending.insert(
            1,
            PendingRequest {
                token: 1,
                cancellation: Some(write.generation.clone()),
                book_writes: vec![(channel.to_owned(), write.clone())],
                response_tx,
            },
        );

        let ack: Value =
            serde_json::from_str(include_str!("../../test_data/spot/ws_subscribe_ack.json"))
                .unwrap();
        let mut data: Value =
            serde_json::from_str(include_str!("../../test_data/spot/ws_orderbook_eth.json"))
                .unwrap();

        let notification = |data: &Value| {
            Message::Text(
                json!({"method": "subscription", "params": {"channel": channel, "data": data}})
                    .to_string()
                    .into(),
            )
        };

        raw_tx.send(notification(&data)).unwrap();
        raw_tx
            .send(Message::Text(
                json!({"id": 1, "result": ack["result"]}).to_string().into(),
            ))
            .unwrap();
        data["timestamp"] = json!(data["timestamp"].as_i64().unwrap() + 1);
        raw_tx.send(notification(&data)).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), handler.next())
            .await
            .unwrap()
            .unwrap();

        let DeriveWsMessage::BookSubscription {
            payload,
            generation,
        } = event
        else {
            panic!("expected gated book event");
        };

        assert_eq!(
            serde_json::from_str::<Value>(payload.data.get()).unwrap(),
            data
        );
        assert!(!generation.is_cancelled());
        assert!(!write.gate.lock().is_closed());
        assert_eq!(
            serde_json::from_str::<Value>(response_rx.await.unwrap().unwrap().get()).unwrap(),
            ack["result"]
        );
    }

    #[rstest]
    #[case::cancelled(true, false, false, true)]
    #[case::obsolete(false, true, false, true)]
    #[case::abandoned(false, false, true, true)]
    #[case::current(false, false, false, false)]
    fn book_ack_opens_only_live_request(
        #[case] cancelled: bool,
        #[case] obsolete: bool,
        #[case] abandoned: bool,
        #[case] expected_closed: bool,
    ) {
        let tracker = BookSyncTracker::default();
        let channel = "orderbook.ETH-USDC.1.10";
        tracker.record_subscription(channel, InstrumentId::from("ETH-USDC.DERIVE"));
        let write = tracker.write(channel).unwrap();
        let gate = write.gate.clone();
        let generation = write.generation.clone();
        let cancel = CancellationToken::new();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let _response_rx = if abandoned {
            drop(response_rx);
            None
        } else {
            Some(response_rx)
        };

        let pending = PendingRequest {
            token: 1,
            cancellation: Some(cancel.clone()),
            book_writes: vec![(channel.to_owned(), write)],
            response_tx,
        };

        if cancelled {
            cancel.cancel();
        }

        if obsolete {
            generation.cancel();
        }

        let ack: Value =
            serde_json::from_str(include_str!("../../test_data/spot/ws_subscribe_ack.json"))
                .unwrap();
        let result = RawValue::from_string(ack["result"].to_string()).unwrap();
        open_book_gates(&pending, &result, &tracker);
        assert_eq!(gate.lock().is_closed(), expected_closed);
    }

    mod dst {
        use super::*;

        #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
        #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
        async fn commands_and_frames_progress_when_both_queues_are_ready() {
            let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
            let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
            let mut handler = feed_handler(
                Arc::new(AtomicBool::new(false)),
                cmd_rx,
                raw_rx,
                Arc::new(AtomicU64::new(1)),
                AuthTracker::new(),
                unauthenticated_epoch(),
            );
            let mut responses = Vec::new();

            for _ in 0..2 {
                let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                cmd_tx
                    .send(HandlerCommand::Request {
                        method: "subscribe",
                        params: secret_json(json!({"channels": ["trades.perp.ETH"]})),
                        connection_epoch: None,
                        cancellation: None,
                        response_tx,
                    })
                    .unwrap();

                responses.push(response_rx);
            }

            raw_tx
                .send(Message::text(
                    r#"{"method":"subscription","params":{"channel":"trades.perp.ETH","data":[]}}"#,
                ))
                .unwrap();
            let event =
                nautilus_common::live::dst::time::timeout(Duration::from_secs(1), handler.next())
                    .await
                    .unwrap()
                    .unwrap();

            let DeriveWsMessage::Subscription(payload) = event else {
                panic!("expected subscription frame");
            };

            assert_eq!(payload.channel.as_str(), "trades.perp.ETH");
            assert_eq!(payload.data.get(), "[]");
            assert!(matches!(
                responses[0].try_recv().unwrap(),
                Err(DeriveWsError::NotConnected)
            ));
            assert_eq!(
                responses[1].try_recv().unwrap_err(),
                tokio::sync::oneshot::error::TryRecvError::Empty
            );
            cmd_tx.send(HandlerCommand::Disconnect).unwrap();
            assert!(
                nautilus_common::live::dst::time::timeout(Duration::from_secs(1), handler.next())
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(matches!(
                responses[1].try_recv().unwrap(),
                Err(DeriveWsError::NotConnected)
            ));
        }
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
        handler.dispatch_request("public/login", params, None, None, response_tx);

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
        handler.enqueue_request(
            "first",
            secret_json(json!({"sequence": 1})),
            0,
            None,
            first_tx,
        );
        handler.enqueue_request(
            "second",
            secret_json(json!({"sequence": 2})),
            0,
            None,
            second_tx,
        );

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
        handler.enqueue_request("first", secret_json(json!({})), 0, None, old_tx);
        let old_send = send_rx.recv().await.expect("old queued send");
        let old_pending = handler.pending.remove(&old_send.id).unwrap();
        old_pending
            .response_tx
            .send(Ok(RawValue::NULL.to_owned()))
            .unwrap();
        old_rx.await.unwrap().unwrap();

        next_id.store(old_send.id, Ordering::Relaxed);
        let (new_tx, new_rx) = tokio::sync::oneshot::channel();
        handler.enqueue_request("second", secret_json(json!({})), 0, None, new_tx);
        let new_send = send_rx.recv().await.expect("new queued send");
        handler.handle_send_failure(&SendFailure {
            id: old_send.id,
            token: old_send.token,
            error: SendError::BrokenPipe("late failure".to_string()),
        });

        assert_eq!(old_send.id, new_send.id);
        assert_ne!(old_send.token, new_send.token);
        assert_eq!(
            handler.pending.get(&new_send.id).unwrap().token,
            new_send.token,
        );

        handler.handle_send_failure(&SendFailure {
            id: new_send.id,
            token: new_send.token,
            error: SendError::BrokenPipe("current failure".to_string()),
        });

        let error = new_rx.await.unwrap().expect_err("current request failed");
        assert!(error.to_string().contains("current failure"));
    }

    #[rstest]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
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

        let task = task::spawn(std::future::pending::<()>());
        let abort_handle = task.abort_handle();
        handler.send_task.insert(task);

        let (first_tx, first_rx) = tokio::sync::oneshot::channel();
        let (second_tx, second_rx) = tokio::sync::oneshot::channel();
        handler.enqueue_request("first", secret_json(json!({})), 0, None, first_tx);
        handler.enqueue_request("second", secret_json(json!({})), 0, None, second_tx);
        handler.shutdown_send_path("disconnect requested").await;
        task::yield_now().await;

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
        handler.enqueue_request("request", secret_json(json!({})), 0, None, response_tx);

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
        handler.enqueue_request("first", secret_json(json!({})), 0, None, first_tx);
        handler.enqueue_request("second", secret_json(json!({})), 0, None, second_tx);

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
