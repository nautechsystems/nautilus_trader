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

//! WebSocket message handler for Bybit.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nautilus_network::{
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use tokio_tungstenite::tungstenite::Message;

use super::{
    enums::BybitWsOperation,
    error::{BybitWsError, create_bybit_timeout_error, should_retry_bybit_error},
    messages::{BybitWebSocketError, BybitWsMessage, BybitWsResponse},
};
use crate::common::consts::{
    BYBIT_TOPIC_EXECUTION, BYBIT_TOPIC_KLINE, BYBIT_TOPIC_ORDER, BYBIT_TOPIC_ORDERBOOK,
    BYBIT_TOPIC_POSITION, BYBIT_TOPIC_PUBLIC_TRADE, BYBIT_TOPIC_TICKERS, BYBIT_TOPIC_TRADE,
    BYBIT_TOPIC_WALLET,
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum HandlerCommand {
    SetClient(WebSocketClient),
    Disconnect,
    Authenticate { payload: String },
    Subscribe { topics: Vec<String> },
    Unsubscribe { topics: Vec<String> },
    SendText { payload: String },
}

pub(super) struct FeedHandler {
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    auth_tracker: AuthTracker,
    subscriptions: SubscriptionState,
    retry_manager: RetryManager<BybitWsError>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        auth_tracker: AuthTracker,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            auth_tracker,
            subscriptions,
            retry_manager: create_websocket_retry_manager(),
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    /// Sends a WebSocket message with retry logic.
    async fn send_with_retry(&self, payload: String) -> Result<(), BybitWsError> {
        if let Some(client) = &self.client {
            self.retry_manager
                .execute_with_retry(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        async move {
                            client
                                .send_text(payload, None)
                                .await
                                .map_err(|e| BybitWsError::Transport(format!("Send failed: {e}")))
                        }
                    },
                    should_retry_bybit_error,
                    create_bybit_timeout_error,
                )
                .await
        } else {
            Err(BybitWsError::ClientError(
                "No active WebSocket client".to_string(),
            ))
        }
    }

    pub(super) async fn next(&mut self) -> Option<BybitWsMessage> {
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
                                log::debug!("Subscribing to topic: topic={topic}");
                                if let Err(e) = self.send_with_retry(topic.clone()).await {
                                    log::error!("Failed to send subscription after retries: topic={topic}, error={e}");
                                }
                            }
                        }
                        HandlerCommand::Unsubscribe { topics } => {
                            for topic in topics {
                                log::debug!("Unsubscribing from topic: topic={topic}");
                                if let Err(e) = self.send_with_retry(topic.clone()).await {
                                    log::error!("Failed to send unsubscription after retries: topic={topic}, error={e}");
                                }
                            }
                        }
                        HandlerCommand::SendText { payload } => {
                            if let Err(e) = self.send_with_retry(payload).await {
                                log::error!("Error sending text with retry: {e}");
                            }
                        }
                    }
                }

                () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Relaxed) {
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

                    if let Message::Ping(data) = &msg {
                        log::trace!("Received ping frame with {} bytes", data.len());

                        if let Some(client) = &self.client
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            log::warn!("Failed to send pong frame: error={e}");
                        }
                        continue;
                    }

                    let event = match Self::parse_raw_message(msg) {
                        Some(event) => event,
                        None => continue,
                    };

                    if self.signal.load(Ordering::Relaxed) {
                        log::debug!("Stop signal received");
                        return None;
                    }

                    match event {
                        BybitWsMessage::Subscription(ref sub_msg) => {
                            let pending_topics = self.subscriptions.pending_subscribe_topics();
                            match sub_msg.op {
                                BybitWsOperation::Subscribe => {
                                    if sub_msg.success {
                                        for topic in pending_topics {
                                            self.subscriptions.confirm_subscribe(&topic);
                                            log::debug!("Subscription confirmed: topic={topic}");
                                        }
                                    } else {
                                        for topic in pending_topics {
                                            self.subscriptions.mark_failure(&topic);
                                            log::warn!(
                                                "Subscription failed, will retry on reconnect: topic={topic}, error={:?}",
                                                sub_msg.ret_msg
                                            );
                                        }
                                    }
                                }
                                BybitWsOperation::Unsubscribe => {
                                    let pending_unsub = self.subscriptions.pending_unsubscribe_topics();

                                    if sub_msg.success {
                                        for topic in pending_unsub {
                                            self.subscriptions.confirm_unsubscribe(&topic);
                                            log::debug!("Unsubscription confirmed: topic={topic}");
                                        }
                                    } else {
                                        for topic in pending_unsub {
                                            log::warn!(
                                                "Unsubscription failed: topic={topic}, error={:?}",
                                                sub_msg.ret_msg
                                            );
                                        }
                                    }
                                }
                                _ => {}
                            }
                            // Subscriptions are handled internally, not forwarded
                        }
                        BybitWsMessage::Auth(ref auth_response) => {
                            let is_success = auth_response.success.unwrap_or(false)
                                || (auth_response.ret_code == Some(0));

                            if is_success {
                                self.auth_tracker.succeed();
                                log::info!("WebSocket authenticated");
                            } else {
                                let error_msg = auth_response
                                    .ret_msg
                                    .as_deref()
                                    .unwrap_or("Authentication rejected");
                                self.auth_tracker.fail(error_msg);
                                log::error!("WebSocket authentication failed: error={error_msg}");
                            }
                            return Some(event);
                        }
                        BybitWsMessage::Pong | BybitWsMessage::Response(_) => {
                            // Handled internally
                        }
                        _ => {
                            return Some(event);
                        }
                    }
                }
            }
        }
    }

    fn parse_raw_message(msg: Message) -> Option<BybitWsMessage> {
        use serde_json::Value;

        match msg {
            Message::Text(text) => {
                if text == nautilus_network::RECONNECTED {
                    log::info!("Received WebSocket reconnected signal");
                    return Some(BybitWsMessage::Reconnected);
                }

                if text.trim().eq_ignore_ascii_case("pong") {
                    return None;
                }

                log::trace!("Raw websocket message: {text}");

                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        log::error!("Failed to parse WebSocket message: {e}: {text}");
                        return None;
                    }
                };

                Some(classify_bybit_message(value))
            }
            Message::Binary(msg) => {
                log::debug!("Raw binary: {msg:?}");
                None
            }
            Message::Close(_) => {
                log::debug!("Received close message, waiting for reconnection");
                None
            }
            _ => None,
        }
    }
}

/// Classifies a parsed JSON value into a typed Bybit WebSocket message.
///
/// Returns `Raw(value)` if no specific type matches.
pub fn classify_bybit_message(value: serde_json::Value) -> BybitWsMessage {
    use super::messages::{BybitWsAuthResponse, BybitWsOrderResponse, BybitWsSubscriptionMsg};

    if let Some(op_val) = value.get("op") {
        if let Ok(op) = serde_json::from_value::<BybitWsOperation>(op_val.clone())
            && op == BybitWsOperation::Auth
            && let Ok(auth) = serde_json::from_value::<BybitWsAuthResponse>(value.clone())
        {
            let is_success = auth.success.unwrap_or(false) || auth.ret_code.unwrap_or(-1) == 0;
            if is_success {
                return BybitWsMessage::Auth(auth);
            }
            let resp = BybitWsResponse {
                op: Some(auth.op.clone()),
                topic: None,
                success: auth.success,
                conn_id: auth.conn_id.clone(),
                req_id: None,
                ret_code: auth.ret_code,
                ret_msg: auth.ret_msg,
            };
            let error = BybitWebSocketError::from_response(&resp);
            return BybitWsMessage::Error(error);
        }

        if let Some(op_str) = op_val.as_str()
            && op_str.starts_with("order.")
        {
            return serde_json::from_value::<BybitWsOrderResponse>(value.clone()).map_or_else(
                |_| BybitWsMessage::Raw(value),
                BybitWsMessage::OrderResponse,
            );
        }
    }

    if let Some(success) = value.get("success").and_then(serde_json::Value::as_bool) {
        if success {
            return serde_json::from_value::<BybitWsSubscriptionMsg>(value.clone())
                .map_or_else(|_| BybitWsMessage::Raw(value), BybitWsMessage::Subscription);
        }
        return serde_json::from_value::<BybitWsResponse>(value.clone()).map_or_else(
            |_| BybitWsMessage::Raw(value),
            |resp| {
                let error = BybitWebSocketError::from_response(&resp);
                BybitWsMessage::Error(error)
            },
        );
    }

    // Most common path for market data
    if let Some(topic) = value.get("topic").and_then(serde_json::Value::as_str) {
        if topic.starts_with(BYBIT_TOPIC_ORDERBOOK) {
            return serde_json::from_value(value.clone())
                .map_or_else(|_| BybitWsMessage::Raw(value), BybitWsMessage::Orderbook);
        }

        if topic.contains(BYBIT_TOPIC_PUBLIC_TRADE) || topic.starts_with(BYBIT_TOPIC_TRADE) {
            return serde_json::from_value(value.clone())
                .map_or_else(|_| BybitWsMessage::Raw(value), BybitWsMessage::Trade);
        }

        if topic.starts_with(BYBIT_TOPIC_KLINE) {
            return serde_json::from_value(value.clone())
                .map_or_else(|_| BybitWsMessage::Raw(value), BybitWsMessage::Kline);
        }

        if topic.starts_with(BYBIT_TOPIC_TICKERS) {
            // Option symbols: BTC-6JAN23-17500-C (date, strike, C/P)
            let is_option = value
                .get("data")
                .and_then(|d| d.get("symbol"))
                .and_then(|s| s.as_str())
                .is_some_and(|symbol| symbol.contains('-') && symbol.matches('-').count() >= 3);

            if is_option {
                return serde_json::from_value(value.clone())
                    .map_or_else(|_| BybitWsMessage::Raw(value), BybitWsMessage::TickerOption);
            }
            return serde_json::from_value(value.clone())
                .map_or_else(|_| BybitWsMessage::Raw(value), BybitWsMessage::TickerLinear);
        }

        if topic.starts_with(BYBIT_TOPIC_ORDER) {
            return serde_json::from_value(value.clone())
                .map_or_else(|_| BybitWsMessage::Raw(value), BybitWsMessage::AccountOrder);
        }

        if topic.starts_with(BYBIT_TOPIC_EXECUTION) {
            return serde_json::from_value(value.clone()).map_or_else(
                |_| BybitWsMessage::Raw(value),
                BybitWsMessage::AccountExecution,
            );
        }

        if topic.starts_with(BYBIT_TOPIC_WALLET) {
            return serde_json::from_value(value.clone()).map_or_else(
                |_| BybitWsMessage::Raw(value),
                BybitWsMessage::AccountWallet,
            );
        }

        if topic.starts_with(BYBIT_TOPIC_POSITION) {
            return serde_json::from_value(value.clone()).map_or_else(
                |_| BybitWsMessage::Raw(value),
                BybitWsMessage::AccountPosition,
            );
        }
    }

    BybitWsMessage::Raw(value)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::consts::BYBIT_WS_TOPIC_DELIMITER;

    fn create_test_handler() -> FeedHandler {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let auth_tracker = AuthTracker::new();
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER);

        FeedHandler::new(signal, cmd_rx, raw_rx, auth_tracker, subscriptions)
    }

    #[rstest]
    fn test_handler_initializes() {
        let _handler = create_test_handler();
    }
}
