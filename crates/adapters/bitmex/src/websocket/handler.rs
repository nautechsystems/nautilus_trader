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

//! WebSocket message handler for BitMEX.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use tokio_tungstenite::tungstenite::Message;

use super::{
    enums::{BitmexWsAuthAction, BitmexWsOperation},
    error::BitmexWsError,
    messages::{BitmexHttpRequest, BitmexRawWsMessage, BitmexWsMessage},
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
    /// Subscribe to the given topics.
    Subscribe { topics: Vec<String> },
    /// Unsubscribe from the given topics.
    Unsubscribe { topics: Vec<String> },
}

pub(super) struct FeedHandler {
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<BitmexWsMessage>,
    auth_tracker: AuthTracker,
    subscriptions: SubscriptionState,
    retry_manager: RetryManager<BitmexWsError>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<BitmexWsMessage>,
        auth_tracker: AuthTracker,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            out_tx,
            auth_tracker,
            subscriptions,
            retry_manager: create_websocket_retry_manager(),
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    pub(super) fn send(&self, msg: BitmexWsMessage) -> Result<(), ()> {
        self.out_tx.send(msg).map_err(|_| ())
    }

    /// Sends a WebSocket message with retry logic.
    async fn send_with_retry(&self, payload: String) -> anyhow::Result<()> {
        if let Some(client) = &self.client {
            self.retry_manager
                .execute_with_retry(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        async move {
                            client.send_text(payload, None).await.map_err(|e| {
                                BitmexWsError::ClientError(format!("Send failed: {e}"))
                            })
                        }
                    },
                    should_retry_bitmex_error,
                    create_bitmex_timeout_error,
                )
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            Err(anyhow::anyhow!("No active WebSocket client"))
        }
    }

    pub(super) async fn next(&mut self) -> Option<BitmexWsMessage> {
        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("WebSocketClient received by handler");
                            self.client = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Disconnect command received");

                            if let Some(client) = self.client.take() {
                                client.disconnect().await;
                            }
                        }
                        HandlerCommand::Authenticate { payload } => {
                            log::debug!("Authenticate command received");

                            if let Err(e) = self.send_with_retry(payload).await {
                                log::error!("Failed to send authentication after retries: {e}");
                            }
                        }
                        HandlerCommand::Subscribe { topics } => {
                            for topic in topics {
                                log::debug!("Subscribing to topic: {topic}");
                                if let Err(e) = self.send_with_retry(topic.clone()).await {
                                    log::error!("Failed to send subscription after retries: topic={topic}, error={e}");
                                }
                            }
                        }
                        HandlerCommand::Unsubscribe { topics } => {
                            for topic in topics {
                                log::debug!("Unsubscribing from topic: {topic}");
                                if let Err(e) = self.send_with_retry(topic.clone()).await {
                                    log::error!("Failed to send unsubscription after retries: topic={topic}, error={e}");
                                }
                            }
                        }
                    }
                }

                () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(std::sync::atomic::Ordering::Relaxed) {
                        log::debug!("Stop signal received during idle period");
                        return None;
                    }
                }

                msg = self.raw_rx.recv() => {
                    let msg = match msg {
                        Some(msg) => msg,
                        None => {
                            log::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    // Handle ping frames directly for minimal latency
                    if let Message::Ping(data) = &msg {
                        log::trace!("Received ping frame with {} bytes", data.len());

                        if let Some(client) = &self.client
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            log::warn!("Failed to send pong frame: {e}");
                        }
                        continue;
                    }

                    let event = match Self::parse_raw_message(msg) {
                        Some(event) => event,
                        None => continue,
                    };

                    if self.signal.load(std::sync::atomic::Ordering::Relaxed) {
                        log::debug!("Stop signal received");
                        return None;
                    }

                    match event {
                        BitmexRawWsMessage::Reconnected => {
                            return Some(BitmexWsMessage::Reconnected);
                        }
                        BitmexRawWsMessage::Subscription {
                            success,
                            subscribe,
                            request,
                            error,
                        } => {
                            if let Some(msg) = self.handle_subscription_message(
                                success,
                                subscribe.as_ref(),
                                request.as_ref(),
                                error.as_deref(),
                            ) {
                                return Some(msg);
                            }
                        }
                        BitmexRawWsMessage::Table(table_msg) => {
                            return Some(BitmexWsMessage::Table(table_msg));
                        }
                        BitmexRawWsMessage::Welcome { .. } | BitmexRawWsMessage::Error { .. } => {}
                    }
                }

                // Handle shutdown - either channel closed or stream ended
                else => {
                    log::debug!("Handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    fn parse_raw_message(msg: Message) -> Option<BitmexRawWsMessage> {
        match msg {
            Message::Text(text) => {
                if text == RECONNECTED {
                    log::info!("Received WebSocket reconnected signal");
                    return Some(BitmexRawWsMessage::Reconnected);
                }

                log::trace!("Raw websocket message: {text}");

                if Self::is_heartbeat_message(&text) {
                    log::trace!("Ignoring heartbeat control message: {text}");
                    return None;
                }

                match serde_json::from_str(&text) {
                    Ok(msg) => match &msg {
                        BitmexRawWsMessage::Welcome {
                            version,
                            heartbeat_enabled,
                            limit,
                            ..
                        } => {
                            log::info!(
                                "Welcome to the BitMEX Realtime API: version={}, heartbeat={}, rate_limit={:?}",
                                version,
                                heartbeat_enabled,
                                limit.remaining,
                            );
                        }
                        BitmexRawWsMessage::Subscription { .. } => return Some(msg),
                        BitmexRawWsMessage::Error { status, error, .. } => {
                            log::error!(
                                "Received error from BitMEX: status={status}, error={error}",
                            );
                        }
                        _ => return Some(msg),
                    },
                    Err(e) => {
                        log::error!("Failed to parse WebSocket message: {e}: {text}");
                    }
                }
            }
            Message::Binary(msg) => {
                log::debug!("Raw binary: {msg:?}");
            }
            Message::Close(_) => {
                log::debug!("Received close message, waiting for reconnection");
            }
            Message::Ping(data) => {
                // Handled in select! loop before parse_raw_message
                log::trace!("Ping frame with {} bytes (already handled)", data.len());
            }
            Message::Pong(data) => {
                log::trace!("Received pong frame with {} bytes", data.len());
            }
            Message::Frame(frame) => {
                log::debug!("Received raw frame: {frame:?}");
            }
        }

        None
    }

    fn is_heartbeat_message(text: &str) -> bool {
        let trimmed = text.trim();

        if !trimmed.starts_with('{') || trimmed.len() > 64 {
            return false;
        }

        trimmed.contains("\"op\":\"ping\"") || trimmed.contains("\"op\":\"pong\"")
    }

    fn handle_subscription_ack(
        &self,
        success: bool,
        request: Option<&BitmexHttpRequest>,
        subscribe: Option<&String>,
        error: Option<&str>,
    ) {
        let topics = Self::topics_from_request(request, subscribe);

        if topics.is_empty() {
            log::debug!("Subscription acknowledgement without topics");
            return;
        }

        for topic in topics {
            if success {
                self.subscriptions.confirm_subscribe(topic);
                log::debug!("Subscription confirmed: topic={topic}");
            } else {
                self.subscriptions.mark_failure(topic);
                let reason = error.unwrap_or("Subscription rejected");
                log::error!("Subscription failed: topic={topic}, error={reason}");
            }
        }
    }

    fn handle_unsubscribe_ack(
        &self,
        success: bool,
        request: Option<&BitmexHttpRequest>,
        subscribe: Option<&String>,
        error: Option<&str>,
    ) {
        let topics = Self::topics_from_request(request, subscribe);

        if topics.is_empty() {
            log::debug!("Unsubscription acknowledgement without topics");
            return;
        }

        for topic in topics {
            if success {
                log::debug!("Unsubscription confirmed: topic={topic}");
                self.subscriptions.confirm_unsubscribe(topic);
            } else {
                let reason = error.unwrap_or("Unsubscription rejected");
                log::error!(
                    "Unsubscription failed - restoring subscription: topic={topic}, error={reason}",
                );
                // Venue rejected unsubscribe, so we're still subscribed. Restore state:
                self.subscriptions.confirm_unsubscribe(topic); // Clear pending_unsubscribe
                self.subscriptions.mark_subscribe(topic); // Mark as subscribing
                self.subscriptions.confirm_subscribe(topic); // Confirm subscription
            }
        }
    }

    fn topics_from_request<'a>(
        request: Option<&'a BitmexHttpRequest>,
        fallback: Option<&'a String>,
    ) -> Vec<&'a str> {
        if let Some(req) = request
            && !req.args.is_empty()
        {
            return req.args.iter().filter_map(|arg| arg.as_str()).collect();
        }

        fallback.into_iter().map(|topic| topic.as_str()).collect()
    }

    fn handle_subscription_message(
        &self,
        success: bool,
        subscribe: Option<&String>,
        request: Option<&BitmexHttpRequest>,
        error: Option<&str>,
    ) -> Option<BitmexWsMessage> {
        if let Some(req) = request {
            if req
                .op
                .eq_ignore_ascii_case(BitmexWsAuthAction::AuthKeyExpires.as_ref())
            {
                if success {
                    log::info!("WebSocket authenticated");
                    self.auth_tracker.succeed();
                    return Some(BitmexWsMessage::Authenticated);
                } else {
                    let reason = error.unwrap_or("Authentication rejected").to_string();
                    log::error!("WebSocket authentication failed: {reason}");
                    self.auth_tracker.fail(reason);
                }
                return None;
            }

            if req
                .op
                .eq_ignore_ascii_case(BitmexWsOperation::Subscribe.as_ref())
            {
                self.handle_subscription_ack(success, request, subscribe, error);
                return None;
            }

            if req
                .op
                .eq_ignore_ascii_case(BitmexWsOperation::Unsubscribe.as_ref())
            {
                self.handle_unsubscribe_ack(success, request, subscribe, error);
                return None;
            }
        }

        if subscribe.is_some() {
            self.handle_subscription_ack(success, request, subscribe, error);
            return None;
        }

        if let Some(error) = error {
            log::warn!("Unhandled subscription control message: success={success}, error={error}");
        }

        None
    }
}

/// Returns `true` when a BitMEX error should be retried.
pub(crate) fn should_retry_bitmex_error(error: &BitmexWsError) -> bool {
    match error {
        BitmexWsError::TungsteniteError(_) => true, // Network errors are retryable
        BitmexWsError::ClientError(msg) => {
            // Retry on timeout and connection errors (case-insensitive)
            let msg_lower = msg.to_lowercase();
            msg_lower.contains("timeout")
                || msg_lower.contains("timed out")
                || msg_lower.contains("connection")
                || msg_lower.contains("network")
        }
        _ => false,
    }
}

/// Creates a timeout error for BitMEX retry logic.
pub(crate) fn create_bitmex_timeout_error(msg: String) -> BitmexWsError {
    BitmexWsError::ClientError(msg)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_is_heartbeat_message_detection() {
        assert!(FeedHandler::is_heartbeat_message("{\"op\":\"ping\"}"));
        assert!(FeedHandler::is_heartbeat_message("{\"op\":\"pong\"}"));
        assert!(!FeedHandler::is_heartbeat_message(
            "{\"op\":\"subscribe\",\"args\":[\"trade:XBTUSD\"]}"
        ));
    }
}
