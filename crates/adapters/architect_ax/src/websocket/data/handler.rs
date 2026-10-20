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

//! Market data WebSocket message handler for Ax.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use futures_util::future::BoxFuture;
use nautilus_live::book::snapshot::SnapshotGate;
use nautilus_network::websocket::{SubscriptionState, WebSocketClient};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{AxMdSubscriptionSpec, AxWsClientError};
use crate::{
    common::enums::{AxCandleWidth, AxMdRequestType},
    websocket::{
        messages::{
            AxDataWsMessage, AxMdMessage, AxMdSubscribe, AxMdSubscribeCandles, AxMdUnsubscribe,
            AxMdUnsubscribeCandles,
        },
        parse::parse_md_message,
    },
};

const BOOK_WRITE_CANCELED: &str = "Book write canceled";

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum HandlerCommand {
    /// Set the WebSocket client for this handler.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Replay all subscriptions after a reconnection.
    ReplaySubscriptions,
    /// Subscribe to market data for a symbol.
    Subscribe {
        /// Request ID for correlation.
        request_id: i64,
        /// Instrument symbol.
        symbol: Ustr,
        /// Market data subscription options.
        spec: AxMdSubscriptionSpec,
    },
    /// Writes a book subscription on the current connection, opening `gate` once written.
    ///
    /// With an unsubscribe request ID, first unsubscribes the symbol so the venue restarts its
    /// stream with a snapshot. That acknowledgement stays untracked, keeping the topic desired for
    /// reconnect replay. Cancellation skips unsent work and keeps the gate closed. A write already
    /// admitted to the network can still complete, ahead of later subscription changes.
    WriteBook {
        /// Request ID for correlation.
        request_id: i64,
        /// Request ID of the unsubscribe that replaces an open stream.
        unsubscribe_request_id: Option<i64>,
        /// Instrument symbol.
        symbol: Ustr,
        /// Market data subscription options.
        spec: AxMdSubscriptionSpec,
        /// Cancels unsent work and keeps the gate closed.
        cancel: CancellationToken,
        /// Snapshot gate opened once the write completes.
        gate: SnapshotGate,
        /// Receives the write result.
        completion: tokio::sync::oneshot::Sender<Result<(), AxWsClientError>>,
    },
    /// Unsubscribe from market data for a symbol.
    Unsubscribe {
        /// Request ID for correlation.
        request_id: i64,
        /// Instrument symbol.
        symbol: Ustr,
        /// Subscription topic for state tracking.
        topic: String,
    },
    /// Subscribe to candle data for a symbol.
    SubscribeCandles {
        /// Request ID for correlation.
        request_id: i64,
        /// Instrument symbol.
        symbol: Ustr,
        /// Candle width/interval.
        width: AxCandleWidth,
    },
    /// Unsubscribe from candle data for a symbol.
    UnsubscribeCandles {
        /// Request ID for correlation.
        request_id: i64,
        /// Instrument symbol.
        symbol: Ustr,
        /// Candle width/interval.
        width: AxCandleWidth,
        /// Subscription topic for state tracking.
        topic: String,
    },
}

/// Market data feed handler that processes WebSocket messages.
///
/// Runs in a dedicated Tokio task and owns the WebSocket client exclusively.
/// Emits raw venue types for downstream consumers to parse.
pub(crate) struct AxMdWsFeedHandler {
    signal: Arc<AtomicBool>,
    inner: Option<Arc<WebSocketClient>>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: SubscriptionState,
    message_queue: VecDeque<AxDataWsMessage>,
    replay_request_id: i64,
    needs_subscription_replay: bool,
    pending_subscription_requests: AHashMap<i64, PendingSubscriptionRequest>,
    writes: VecDeque<PendingWrite>,
}

impl AxMdWsFeedHandler {
    /// Creates a new [`AxMdWsFeedHandler`] instance.
    #[must_use]
    pub(crate) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            message_queue: VecDeque::new(),
            replay_request_id: -1,
            needs_subscription_replay: false,
            pending_subscription_requests: AHashMap::new(),
            writes: VecDeque::new(),
        }
    }

    fn next_replay_request_id(&mut self) -> i64 {
        self.replay_request_id -= 1;
        self.replay_request_id
    }

    fn replay_subscriptions(&mut self) {
        self.pending_subscription_requests.clear();
        let topics = self.subscriptions.reset_after_reconnect();
        if topics.is_empty() {
            log::debug!("No subscriptions to replay after reconnect");
            return;
        }

        log::debug!("Replaying {} subscriptions after reconnect", topics.len());

        for topic in topics {
            // Topic format: "symbol:Level:trades:ticker" or "candles:symbol:Width"
            if let Some(rest) = topic.strip_prefix("candles:") {
                if let Some((symbol, width_str)) = rest.rsplit_once(':') {
                    if let Some(width) = Self::parse_candle_width(width_str) {
                        let request_id = self.next_replay_request_id();
                        log::debug!(
                            "Replaying candle subscription: symbol={symbol}, width={width:?}"
                        );
                        self.pending_subscription_requests.insert(
                            request_id,
                            PendingSubscriptionRequest::Subscribe(topic.clone()),
                        );
                        self.send_subscribe_candles(request_id, Ustr::from(symbol), width);
                    } else {
                        log::warn!("Failed to parse candle width from topic: {topic}");
                    }
                } else {
                    log::warn!("Invalid candle topic format: {topic}");
                }
            } else if let Some((symbol, spec)) = AxMdSubscriptionSpec::parse_topic(&topic) {
                let request_id = self.next_replay_request_id();
                log::debug!("Replaying market data subscription: symbol={symbol}, spec={spec:?}");
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Subscribe(topic));
                self.send_subscribe(request_id, symbol, spec);
            } else {
                log::warn!("Failed to parse market data subscription topic: {topic}");
            }
        }

        log::debug!("Subscription replay completed");
    }

    fn parse_candle_width(s: &str) -> Option<AxCandleWidth> {
        match s {
            "Seconds1" => Some(AxCandleWidth::Seconds1),
            "Seconds5" => Some(AxCandleWidth::Seconds5),
            "Minutes1" => Some(AxCandleWidth::Minutes1),
            "Minutes5" => Some(AxCandleWidth::Minutes5),
            "Minutes15" => Some(AxCandleWidth::Minutes15),
            "Hours1" => Some(AxCandleWidth::Hours1),
            "Days1" => Some(AxCandleWidth::Days1),
            _ => None,
        }
    }

    /// Returns the next message from the handler.
    ///
    /// This method blocks until a message is available or the handler is stopped.
    pub(crate) async fn next(&mut self) -> Option<AxDataWsMessage> {
        loop {
            if self.needs_subscription_replay && self.message_queue.is_empty() {
                self.needs_subscription_replay = false;
                self.replay_subscriptions();
            }

            if let Some(msg) = self.message_queue.pop_front() {
                return Some(msg);
            }

            tokio::select! {
                biased;

                failed = async {
                    match self.writes.front_mut() {
                        Some(write) => (&mut write.future).await,
                        None => std::future::pending().await,
                    }
                } => {
                    let write = self.writes.pop_front();

                    if let Some(request_id) = failed {
                        self.pending_subscription_requests.remove(&request_id);
                    } else if let Some(write) = write
                        && let Some((_, Some(snapshot))) = write.book
                    {
                        self.message_queue.push_back(snapshot);
                    }
                }

                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }

                () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Acquire) {
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

                        if let Some(client) = &self.inner
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            log::warn!("Failed to send pong frame: {e}");
                        }
                        continue;
                    }

                    if let Some(message) = self.parse_raw_message(msg) {
                        self.queue_message(message);
                    }

                    if self.signal.load(Ordering::Acquire) {
                        log::debug!("Stop signal received");
                        return None;
                    }
                }
            }
        }
    }

    fn queue_message(&mut self, message: AxDataWsMessage) {
        let symbol = match &message {
            AxDataWsMessage::MdMessage(AxMdMessage::BookL2(book)) if book.st => Some(book.s),
            AxDataWsMessage::MdMessage(AxMdMessage::BookL3(book)) if book.st => Some(book.s),
            _ => None,
        };

        if let Some((pending_symbol, snapshot)) = self
            .writes
            .front_mut()
            .and_then(|write| write.book.as_mut())
            && symbol == Some(*pending_symbol)
        {
            *snapshot = Some(message);
        } else {
            self.message_queue.push_back(message);
        }
    }

    async fn handle_command(&mut self, cmd: HandlerCommand) {
        match cmd {
            HandlerCommand::SetClient(client) => {
                log::debug!("WebSocketClient received by handler");
                self.inner = Some(Arc::new(client));
            }
            HandlerCommand::Disconnect => {
                log::debug!("Disconnect command received");
                self.writes.clear();

                if let Some(inner) = self.inner.take() {
                    inner.disconnect().await;
                }
            }
            HandlerCommand::ReplaySubscriptions => {
                log::debug!("ReplaySubscriptions command received");
                self.replay_subscriptions();
            }
            HandlerCommand::Subscribe {
                request_id,
                symbol,
                spec,
            } => {
                log::debug!(
                    "Subscribe command received: request_id={request_id}, symbol={symbol}, spec={spec:?}"
                );
                let topic = spec.topic(symbol.as_str());
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Subscribe(topic));
                self.send_subscribe(request_id, symbol, spec);
            }
            HandlerCommand::WriteBook {
                request_id,
                unsubscribe_request_id,
                symbol,
                spec,
                cancel,
                gate,
                completion,
            } => {
                log::debug!(
                    "WriteBook command received: request_id={request_id}, symbol={symbol}, spec={spec:?}"
                );

                if cancel.is_cancelled() {
                    let _ = completion.send(Err(AxWsClientError::ClientError(
                        BOOK_WRITE_CANCELED.to_string(),
                    )));
                    return;
                }

                let Some(client) = self.inner.clone() else {
                    let _ = completion.send(Err(AxWsClientError::Transport(
                        "No WebSocket client available".to_string(),
                    )));
                    return;
                };

                let epoch = client.connection_epoch();
                let topic = spec.topic(symbol.as_str());
                let subscriptions = self.subscriptions.clone();
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Subscribe(topic));

                let future = Box::pin(async move {
                    let result = write_book(
                        &client,
                        &subscriptions,
                        request_id,
                        unsubscribe_request_id,
                        symbol,
                        spec,
                        epoch,
                        &cancel,
                    )
                    .await;

                    let written = result.is_ok() && !cancel.is_cancelled();
                    if written {
                        gate.open();
                    }

                    let _ = completion.send(result);
                    (!written).then_some(request_id)
                });

                self.writes.push_back(PendingWrite {
                    future,
                    book: Some((symbol, None)),
                });
            }
            HandlerCommand::Unsubscribe {
                request_id,
                symbol,
                topic,
            } => {
                log::debug!(
                    "Unsubscribe command received: request_id={request_id}, symbol={symbol}"
                );
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Unsubscribe(topic));
                self.send_unsubscribe(request_id, symbol);
            }
            HandlerCommand::SubscribeCandles {
                request_id,
                symbol,
                width,
            } => {
                log::debug!(
                    "SubscribeCandles command received: request_id={request_id}, symbol={symbol}, width={width:?}"
                );
                let topic = format!("candles:{symbol}:{width:?}");
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Subscribe(topic));
                self.send_subscribe_candles(request_id, symbol, width);
            }
            HandlerCommand::UnsubscribeCandles {
                request_id,
                symbol,
                width,
                topic,
            } => {
                log::debug!(
                    "UnsubscribeCandles command received: request_id={request_id}, symbol={symbol}, width={width:?}"
                );
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Unsubscribe(topic));
                self.message_queue
                    .push_back(AxDataWsMessage::CandleUnsubscribed { symbol, width });
                self.send_unsubscribe_candles(request_id, symbol, width);
            }
        }
    }

    fn send_subscribe(&mut self, request_id: i64, symbol: Ustr, spec: AxMdSubscriptionSpec) {
        let msg = AxMdSubscribe {
            rid: request_id,
            msg_type: AxMdRequestType::Subscribe,
            symbol,
            level: spec.level,
            trades: spec.trades,
            ticker: spec.ticker,
        };

        if let Err(e) = self.send_json(request_id, &msg) {
            self.pending_subscription_requests.remove(&request_id);
            log::error!("Failed to send subscribe message: {e}");
        }
    }

    fn send_unsubscribe(&mut self, request_id: i64, symbol: Ustr) {
        let msg = AxMdUnsubscribe {
            rid: request_id,
            msg_type: AxMdRequestType::Unsubscribe,
            symbol,
        };

        if let Err(e) = self.send_json(request_id, &msg) {
            self.pending_subscription_requests.remove(&request_id);
            log::error!("Failed to send unsubscribe message: {e}");
        }
    }

    fn send_subscribe_candles(&mut self, request_id: i64, symbol: Ustr, width: AxCandleWidth) {
        let msg = AxMdSubscribeCandles {
            rid: request_id,
            msg_type: AxMdRequestType::SubscribeCandles,
            symbol,
            width,
        };

        if let Err(e) = self.send_json(request_id, &msg) {
            self.pending_subscription_requests.remove(&request_id);
            log::error!("Failed to send subscribe_candles message: {e}");
        }
    }

    fn send_unsubscribe_candles(&mut self, request_id: i64, symbol: Ustr, width: AxCandleWidth) {
        let msg = AxMdUnsubscribeCandles {
            rid: request_id,
            msg_type: AxMdRequestType::UnsubscribeCandles,
            symbol,
            width,
        };

        if let Err(e) = self.send_json(request_id, &msg) {
            self.pending_subscription_requests.remove(&request_id);
            log::error!("Failed to send unsubscribe_candles message: {e}");
        }
    }

    fn send_json<T: serde::Serialize>(&mut self, request_id: i64, msg: &T) -> Result<(), String> {
        let Some(inner) = self.inner.clone() else {
            return Err("No WebSocket client available".to_string());
        };

        let payload = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        log::trace!("Sending WebSocket payload ({} bytes)", payload.len());

        let future = Box::pin(async move {
            if let Err(e) = inner.send_text(payload, None).await {
                log::error!("Failed to send subscription message: {e}");
                return Some(request_id);
            }

            None
        });

        self.writes.push_back(PendingWrite { future, book: None });

        Ok(())
    }

    fn parse_raw_message(&mut self, msg: Message) -> Option<AxDataWsMessage> {
        match msg {
            Message::Text(text) => {
                if text == nautilus_network::RECONNECTED {
                    log::info!("Received WebSocket reconnected signal");
                    self.needs_subscription_replay = true;
                    return Some(AxDataWsMessage::Reconnected);
                }

                log::trace!("Raw websocket message: {text}");

                match parse_md_message(&text) {
                    Ok(message) => self.handle_message(message),
                    Err(e) => {
                        log::error!("Failed to parse WebSocket message: {e}: {text}");
                        None
                    }
                }
            }
            Message::Binary(data) => {
                log::debug!("Received binary message with {} bytes", data.len());
                None
            }
            Message::Close(_) => {
                log::debug!("Received close message, waiting for reconnection");
                None
            }
            _ => None,
        }
    }

    fn handle_message(&mut self, message: AxMdMessage) -> Option<AxDataWsMessage> {
        match &message {
            AxMdMessage::Error(error) => {
                let is_benign = error.message.contains("already subscribed")
                    || error.message.contains("not subscribed");

                if let Some(rid) = error.request_id
                    && let Some(request) = self.pending_subscription_requests.remove(&rid)
                {
                    match request {
                        PendingSubscriptionRequest::Subscribe(topic) => {
                            self.subscriptions.mark_failure(&topic);
                        }
                        PendingSubscriptionRequest::Unsubscribe(topic) => {
                            self.subscriptions.confirm_unsubscribe(&topic);
                        }
                    }
                }

                if is_benign {
                    log::warn!("Subscription state: {}", error.message);
                } else {
                    log::error!("Received error from exchange: {}", error.message);
                }
            }
            AxMdMessage::SubscriptionResponse(response) => {
                let is_subscribe = response.result.subscribed.is_some()
                    || response.result.subscribed_candle.is_some();
                let is_unsubscribe = response.result.unsubscribed.is_some()
                    || response.result.unsubscribed_candle.is_some();

                if let Some(request) = self.pending_subscription_requests.remove(&response.rid) {
                    match request {
                        PendingSubscriptionRequest::Subscribe(topic) if is_subscribe => {
                            self.subscriptions.confirm_subscribe(&topic);
                        }
                        PendingSubscriptionRequest::Unsubscribe(topic) if is_unsubscribe => {
                            self.subscriptions.confirm_unsubscribe(&topic);
                        }
                        request => {
                            log::warn!(
                                "Unexpected subscription response for request: {request:?}, \
                                 response={response:?}"
                            );
                        }
                    }
                }

                if let Some(symbol) = &response.result.subscribed {
                    log::debug!("Subscription confirmed for symbol: {symbol}");
                } else if let Some(candle) = &response.result.subscribed_candle {
                    log::debug!("Candle subscription confirmed: {candle}");
                } else if let Some(symbol) = &response.result.unsubscribed {
                    log::debug!("Unsubscription confirmed for symbol: {symbol}");
                } else if let Some(candle) = &response.result.unsubscribed_candle {
                    log::debug!("Candle unsubscription confirmed: {candle}");
                }
                return None;
            }
            _ => {}
        }

        Some(AxDataWsMessage::MdMessage(message))
    }
}

#[derive(Debug)]
enum PendingSubscriptionRequest {
    Subscribe(String),
    Unsubscribe(String),
}

struct PendingWrite {
    future: BoxFuture<'static, Option<i64>>,
    book: Option<(Ustr, Option<AxDataWsMessage>)>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "the queued write retains its subscription, connection, and cancellation ownership"
)]
async fn write_book(
    client: &WebSocketClient,
    subscriptions: &SubscriptionState,
    request_id: i64,
    unsubscribe_request_id: Option<i64>,
    symbol: Ustr,
    spec: AxMdSubscriptionSpec,
    epoch: u64,
    cancel: &CancellationToken,
) -> Result<(), AxWsClientError> {
    if cancel.is_cancelled() {
        return Err(AxWsClientError::ClientError(
            BOOK_WRITE_CANCELED.to_string(),
        ));
    }

    if let Some(rid) = unsubscribe_request_id {
        let topic = spec.topic(symbol.as_str());

        // Keeps the topic desired for reconnect replay, so its subscribe ack confirms it again
        subscriptions.mark_failure(&topic);

        let msg = AxMdUnsubscribe {
            rid,
            msg_type: AxMdRequestType::Unsubscribe,
            symbol,
        };

        send_json_on_connection(client, &msg, epoch, cancel).await?;
    }

    let msg = AxMdSubscribe {
        rid: request_id,
        msg_type: AxMdRequestType::Subscribe,
        symbol,
        level: spec.level,
        trades: spec.trades,
        ticker: spec.ticker,
    };

    send_json_on_connection(client, &msg, epoch, cancel).await?;

    if client.connection_epoch() != epoch || !client.is_active() {
        return Err(AxWsClientError::Transport(
            "Book subscription connection changed".to_string(),
        ));
    }

    Ok(())
}

// Sends once on the connection identified by `epoch`, never replaying on a replacement
async fn send_json_on_connection<T: serde::Serialize>(
    client: &WebSocketClient,
    msg: &T,
    epoch: u64,
    cancel: &CancellationToken,
) -> Result<(), AxWsClientError> {
    let payload =
        serde_json::to_string(msg).map_err(|e| AxWsClientError::ClientError(e.to_string()))?;

    tokio::select! {
        biased;
        () = cancel.cancelled() => {
            Err(AxWsClientError::ClientError(BOOK_WRITE_CANCELED.to_string()))
        }
        result = client.send_text_on_connection(payload, None, epoch) => {
            result.map_err(|e| AxWsClientError::Transport(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::{StreamExt, poll};
    use nautilus_network::{
        mode::ConnectionMode,
        websocket::{SubscriptionState, TransportBackend, WebSocketConfig},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::enums::AxMarketDataLevel,
        websocket::messages::{AxMdSubscriptionResponse, AxMdSubscriptionResult, AxWsError},
    };

    const TOPIC: &str = "EURUSD-PERP:Level2:false:false";

    #[rstest]
    fn test_subscription_response_confirms_subscribe() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler
            .pending_subscription_requests
            .insert(1, PendingSubscriptionRequest::Subscribe(TOPIC.to_string()));

        handler.handle_message(AxMdMessage::SubscriptionResponse(
            AxMdSubscriptionResponse {
                rid: 1,
                result: AxMdSubscriptionResult {
                    subscribed: Some(Ustr::from("EURUSD-PERP")),
                    subscribed_candle: None,
                    unsubscribed: None,
                    unsubscribed_candle: None,
                },
            },
        ));

        assert_eq!(subscriptions.len(), 1);
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    fn test_subscription_error_keeps_topic_pending_for_replay() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        subscriptions.confirm_subscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler
            .pending_subscription_requests
            .insert(2, PendingSubscriptionRequest::Subscribe(TOPIC.to_string()));

        handler.handle_message(AxMdMessage::Error(AxWsError {
            code: Some("400".to_string()),
            message: "subscription failed".to_string(),
            request_id: Some(2),
        }));

        assert_eq!(subscriptions.len(), 0);
        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            vec![TOPIC.to_string()]
        );
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    fn test_already_subscribed_keeps_topic_pending_for_replay() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler
            .pending_subscription_requests
            .insert(3, PendingSubscriptionRequest::Subscribe(TOPIC.to_string()));

        handler.handle_message(AxMdMessage::Error(AxWsError {
            code: Some("400".to_string()),
            message: "already subscribed".to_string(),
            request_id: Some(3),
        }));

        assert_eq!(subscriptions.len(), 0);
        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            vec![TOPIC.to_string()]
        );
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    #[case("not subscribed")]
    #[case("subscription failed")]
    fn test_unsubscribe_error_confirms_removal(#[case] message: &str) {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        subscriptions.confirm_subscribe(TOPIC);
        subscriptions.mark_unsubscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler.pending_subscription_requests.insert(
            4,
            PendingSubscriptionRequest::Unsubscribe(TOPIC.to_string()),
        );

        handler.handle_message(AxMdMessage::Error(AxWsError {
            code: Some("400".to_string()),
            message: message.to_string(),
            request_id: Some(4),
        }));

        assert_eq!(subscriptions.len(), 0);
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    fn test_subscription_response_confirms_unsubscribe() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        subscriptions.confirm_subscribe(TOPIC);
        subscriptions.mark_unsubscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler.pending_subscription_requests.insert(
            5,
            PendingSubscriptionRequest::Unsubscribe(TOPIC.to_string()),
        );

        handler.handle_message(AxMdMessage::SubscriptionResponse(
            AxMdSubscriptionResponse {
                rid: 5,
                result: AxMdSubscriptionResult {
                    subscribed: None,
                    subscribed_candle: None,
                    unsubscribed: Some(Ustr::from("EURUSD-PERP")),
                    unsubscribed_candle: None,
                },
            },
        ));

        assert_eq!(subscriptions.len(), 0);
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_replay_clears_stale_requests_and_pending_unsubscribes() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        subscriptions.confirm_subscribe(TOPIC);
        subscriptions.mark_unsubscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler.pending_subscription_requests.insert(
            6,
            PendingSubscriptionRequest::Unsubscribe(TOPIC.to_string()),
        );

        handler.replay_subscriptions();

        assert_eq!(subscriptions.len(), 0);
        assert!(subscriptions.all_topics().is_empty());
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
        assert!(handler.pending_subscription_requests.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_write_book_canceled_before_admission() {
        let mut handler = create_handler(SubscriptionState::new(':'));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let gate = SnapshotGate::default();
        gate.lock().close();
        let (completion, receiver) = tokio::sync::oneshot::channel();

        handler
            .handle_command(HandlerCommand::WriteBook {
                request_id: 9,
                unsubscribe_request_id: Some(10),
                symbol: Ustr::from("EURUSD-PERP"),
                spec: AxMdSubscriptionSpec::new(AxMarketDataLevel::Level2, Some(false), Some(true)),
                cancel,
                gate: gate.clone(),
                completion,
            })
            .await;

        let result = receiver.await.unwrap();

        assert!(matches!(
            result,
            Err(AxWsClientError::ClientError(message)) if message == "Book write canceled"
        ));
        assert!(gate.lock().is_closed());
        assert!(handler.pending_subscription_requests.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_write_book_failure_keeps_gate_closed() {
        let mut handler = create_handler(SubscriptionState::new(':'));
        let gate = SnapshotGate::default();
        gate.lock().close();
        let (completion, receiver) = tokio::sync::oneshot::channel();

        // No client is set, so the write fails before any send
        handler
            .handle_command(HandlerCommand::WriteBook {
                request_id: 7,
                unsubscribe_request_id: Some(8),
                symbol: Ustr::from("EURUSD-PERP"),
                spec: AxMdSubscriptionSpec::new(
                    AxMarketDataLevel::Level2,
                    Some(false),
                    Some(false),
                ),
                cancel: CancellationToken::new(),
                gate: gate.clone(),
                completion,
            })
            .await;

        let result = receiver.await.unwrap();

        assert!(matches!(
            result,
            Err(AxWsClientError::Transport(message)) if message == "No WebSocket client available"
        ));
        assert!(gate.lock().is_closed());
        assert!(handler.pending_subscription_requests.is_empty());
    }

    #[rstest]
    #[case(AxMarketDataLevel::Level2, include_str!("../../../test_data/ws_md_book_l2.json"), false)]
    #[case(AxMarketDataLevel::Level3, include_str!("../../../test_data/ws_md_book_l3.json"), false)]
    #[case(AxMarketDataLevel::Level2, include_str!("../../../test_data/ws_md_book_l2.json"), true)]
    #[tokio::test]
    async fn test_book_snapshot_waits_for_write_confirmation(
        #[case] level: AxMarketDataLevel,
        #[case] fixture: &str,
        #[case] canceled: bool,
    ) {
        let mut connection = HandlerConnection::new().await;
        let cancel = CancellationToken::new();
        let (mut command, gate, result) = book_command(cancel.clone());

        let HandlerCommand::WriteBook { spec, .. } = &mut command else {
            unreachable!();
        };

        spec.level = level;
        connection.handler.handle_command(command).await;
        let (release, held) = tokio::sync::oneshot::channel();
        let mut write = connection.handler.writes.pop_front().unwrap();
        let future = write.future;
        write.future = Box::pin(async move {
            held.await.unwrap();
            future.await
        });

        connection.handler.writes.push_back(write);

        let mut expected: serde_json::Value = serde_json::from_str(fixture).unwrap();
        connection
            .frames
            .send(Message::Text(expected.to_string().into()))
            .unwrap();
        connection.frame().await;
        expected["ts"] = 1609459201_i64.into();
        connection
            .frames
            .send(Message::Text(expected.to_string().into()))
            .unwrap();
        connection.frame().await;
        assert!(gate.lock().is_closed());

        let mut other = expected.clone();
        other["s"] = "GBPUSD-PERP".into();
        connection
            .frames
            .send(Message::Text(other.to_string().into()))
            .unwrap();
        let delivered = tokio::time::timeout(Duration::from_secs(2), connection.handler.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            book_value(delivered),
            book_value(AxDataWsMessage::MdMessage(
                parse_md_message(&other.to_string()).unwrap(),
            ))
        );

        if canceled {
            cancel.cancel();
        }

        release.send(()).unwrap();

        if canceled {
            connection.frame().await;
            assert!(
                matches!(result.await.unwrap(), Err(AxWsClientError::ClientError(message)) if message == "Book write canceled")
            );
            assert!(gate.lock().is_closed());
        } else {
            let delivered = tokio::time::timeout(Duration::from_secs(2), connection.handler.next())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                book_value(delivered),
                book_value(AxDataWsMessage::MdMessage(
                    parse_md_message(&expected.to_string()).unwrap(),
                ))
            );
            assert!(result.await.unwrap().is_ok());
            assert!(!gate.lock().is_closed());
        }

        connection.frame().await;
    }

    fn book_value(message: AxDataWsMessage) -> serde_json::Value {
        match message {
            AxDataWsMessage::MdMessage(AxMdMessage::BookL2(book)) => {
                serde_json::to_value(book).unwrap()
            }
            AxDataWsMessage::MdMessage(AxMdMessage::BookL3(book)) => {
                serde_json::to_value(book).unwrap()
            }
            other => panic!("Expected book snapshot, received {other:?}"),
        }
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn test_canceled_queued_book_sends_nothing(#[case] cancel_before_queue: bool) {
        let mut connection = HandlerConnection::new().await;
        let cancel = CancellationToken::new();
        if cancel_before_queue {
            cancel.cancel();
        }

        let (command, gate, result) = book_command(cancel.clone());
        connection.handler.handle_command(command).await;
        cancel.cancel();
        connection.frame().await;

        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), result).await.unwrap().unwrap(),
            Err(AxWsClientError::ClientError(message)) if message == "Book write canceled"
        ));
        assert_eq!(
            connection.written.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        );
        assert!(gate.lock().is_closed());
        assert!(connection.handler.pending_subscription_requests.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_held_book_write_allows_frames_and_turnover() {
        let mut connection = HandlerConnection::new().await;
        let client = connection.handler.inner.clone().unwrap();
        client
            .connection_mode_atomic()
            .store(ConnectionMode::Reconnect.as_u8(), Ordering::Release);
        let cancel = CancellationToken::new();
        let (command, gate, result) = book_command(cancel.clone());
        connection.handler.handle_command(command).await;
        assert!(poll!(Box::pin(connection.handler.next())).is_pending());
        connection.frame().await;
        assert_eq!(
            connection.written.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        );
        assert!(gate.lock().is_closed());

        cancel.cancel();
        connection
            .commands
            .send(HandlerCommand::Unsubscribe {
                request_id: 13,
                symbol: Ustr::from("EURUSD-PERP"),
                topic: TOPIC.to_string(),
            })
            .unwrap();

        connection
            .commands
            .send(HandlerCommand::Subscribe {
                request_id: 14,
                symbol: Ustr::from("EURUSD-PERP"),
                spec: AxMdSubscriptionSpec::new(AxMarketDataLevel::Level1, Some(true), Some(false)),
            })
            .unwrap();

        connection.frame().await;
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), result).await.unwrap().unwrap(),
            Err(AxWsClientError::ClientError(message)) if message == "Book write canceled"
        ));

        client
            .connection_mode_atomic()
            .store(ConnectionMode::Active.as_u8(), Ordering::Release);

        let expected = [
            serde_json::to_value(AxMdUnsubscribe {
                rid: 13,
                msg_type: AxMdRequestType::Unsubscribe,
                symbol: Ustr::from("EURUSD-PERP"),
            })
            .unwrap(),
            serde_json::to_value(AxMdSubscribe {
                rid: 14,
                msg_type: AxMdRequestType::Subscribe,
                symbol: Ustr::from("EURUSD-PERP"),
                level: AxMarketDataLevel::Level1,
                trades: Some(true),
                ticker: Some(false),
            })
            .unwrap(),
        ];

        let writes = async {
            let mut writes = Vec::new();
            for _ in 0..expected.len() {
                writes.push(connection.written.recv().await.unwrap());
            }

            writes
        };

        let writes = tokio::select! {
            frame = connection.handler.next() => panic!("Unexpected frame: {frame:?}"),
            writes = tokio::time::timeout(Duration::from_secs(2), writes) => writes.unwrap(),
        };

        assert_eq!(writes, expected);
        assert!(gate.lock().is_closed());
        assert_eq!(
            connection.written.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_held_book_write_allows_shutdown() {
        let mut connection = HandlerConnection::new().await;
        connection
            .handler
            .inner
            .as_ref()
            .unwrap()
            .connection_mode_atomic()
            .store(ConnectionMode::Reconnect.as_u8(), Ordering::Release);
        let (command, gate, result) = book_command(CancellationToken::new());
        connection.handler.handle_command(command).await;
        assert!(poll!(Box::pin(connection.handler.next())).is_pending());
        connection
            .commands
            .send(HandlerCommand::Disconnect)
            .unwrap();
        connection.frame().await;

        assert!(result.await.is_err());
        assert!(gate.lock().is_closed());
        assert_eq!(
            connection.written.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_canceled_replacement_skips_subscribe_after_unsubscribe() {
        let mut connection = HandlerConnection::new().await;
        let cancel = CancellationToken::new();
        let (command, gate, result) = book_command(cancel.clone());
        connection.handler.handle_command(command).await;
        assert!(poll!(Box::pin(connection.handler.next())).is_pending());
        let unsubscribe = tokio::time::timeout(Duration::from_secs(2), connection.written.recv())
            .await
            .unwrap()
            .unwrap();
        cancel.cancel();
        connection.frame().await;

        assert_eq!(
            unsubscribe,
            serde_json::to_value(AxMdUnsubscribe {
                rid: 12,
                msg_type: AxMdRequestType::Unsubscribe,
                symbol: Ustr::from("EURUSD-PERP"),
            })
            .unwrap()
        );
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), result).await.unwrap().unwrap(),
            Err(AxWsClientError::ClientError(message)) if message == "Book write canceled"
        ));
        assert!(gate.lock().is_closed());
        assert_eq!(
            connection.written.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnected_book_write_keeps_gate_closed() {
        let mut connection = HandlerConnection::new().await;
        let client = connection.handler.inner.clone().unwrap();
        client
            .connection_mode_atomic()
            .store(ConnectionMode::Reconnect.as_u8(), Ordering::Release);
        let (command, gate, result) = book_command(CancellationToken::new());
        connection.handler.handle_command(command).await;
        assert!(poll!(Box::pin(connection.handler.next())).is_pending());
        client
            .connection_epoch_atomic()
            .fetch_add(1, Ordering::AcqRel);
        client
            .connection_mode_atomic()
            .store(ConnectionMode::Active.as_u8(), Ordering::Release);
        let result = tokio::select! {
            frame = connection.handler.next() => panic!("Unexpected frame: {frame:?}"),
            result = tokio::time::timeout(Duration::from_secs(2), result) => result.unwrap().unwrap(),
        };

        assert!(matches!(
            result,
            Err(AxWsClientError::Transport(message)) if message == "send failed: connection changed before write"
        ));
        assert!(gate.lock().is_closed());
        assert_eq!(
            connection.written.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        );
        assert!(connection.handler.pending_subscription_requests.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_book_write_preserves_flags_and_opens_gate() {
        let mut connection = HandlerConnection::new().await;
        let (command, gate, result) = book_command(CancellationToken::new());
        connection.handler.handle_command(command).await;
        assert!(gate.lock().is_closed());
        assert_eq!(
            connection.written.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        );
        let result = tokio::select! {
            frame = connection.handler.next() => panic!("Unexpected frame: {frame:?}"),
            result = tokio::time::timeout(Duration::from_secs(2), result) => result.unwrap().unwrap(),
        };
        let first = connection.written.recv().await.unwrap();
        let second = connection.written.recv().await.unwrap();

        assert!(result.is_ok());
        assert!(!gate.lock().is_closed());
        assert_eq!(
            first,
            serde_json::to_value(AxMdUnsubscribe {
                rid: 12,
                msg_type: AxMdRequestType::Unsubscribe,
                symbol: Ustr::from("EURUSD-PERP"),
            })
            .unwrap()
        );
        assert_eq!(
            second,
            serde_json::to_value(AxMdSubscribe {
                rid: 11,
                msg_type: AxMdRequestType::Subscribe,
                symbol: Ustr::from("EURUSD-PERP"),
                level: AxMarketDataLevel::Level2,
                trades: Some(false),
                ticker: Some(true),
            })
            .unwrap()
        );
    }

    fn book_command(
        cancel: CancellationToken,
    ) -> (
        HandlerCommand,
        SnapshotGate,
        tokio::sync::oneshot::Receiver<Result<(), AxWsClientError>>,
    ) {
        let gate = SnapshotGate::default();
        gate.lock().close();
        let (completion, result) = tokio::sync::oneshot::channel();

        let command = HandlerCommand::WriteBook {
            request_id: 11,
            unsubscribe_request_id: Some(12),
            symbol: Ustr::from("EURUSD-PERP"),
            spec: AxMdSubscriptionSpec::new(AxMarketDataLevel::Level2, Some(false), Some(true)),
            cancel,
            gate: gate.clone(),
            completion,
        };

        (command, gate, result)
    }

    struct HandlerConnection {
        handler: AxMdWsFeedHandler,
        commands: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
        frames: tokio::sync::mpsc::UnboundedSender<Message>,
        written: tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
        server: tokio::task::JoinHandle<()>,
    }

    impl HandlerConnection {
        async fn new() -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("ws://{}", listener.local_addr().unwrap());
            let (sent, written) = tokio::sync::mpsc::unbounded_channel();

            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                while let Some(Ok(message)) = socket.next().await {
                    if let Message::Text(text) = message {
                        sent.send(serde_json::from_str(&text).unwrap()).unwrap();
                    }
                }
            });

            let config = WebSocketConfig::builder()
                .url(url)
                .backend(TransportBackend::Tungstenite)
                .build()
                .unwrap();
            let client = WebSocketClient::builder()
                .config(config)
                .message_handler(Arc::new(|_| {}))
                .connect()
                .await
                .unwrap();
            let (commands, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
            let (frames, raw_rx) = tokio::sync::mpsc::unbounded_channel();

            let mut handler = AxMdWsFeedHandler::new(
                Arc::new(AtomicBool::new(false)),
                cmd_rx,
                raw_rx,
                SubscriptionState::new(':'),
            );
            handler
                .handle_command(HandlerCommand::SetClient(client))
                .await;

            Self {
                handler,
                commands,
                frames,
                written,
                server,
            }
        }

        async fn frame(&mut self) {
            let heartbeat = include_str!("../../../test_data/ws_md_heartbeat.json");
            self.frames.send(Message::Text(heartbeat.into())).unwrap();
            let frame = tokio::time::timeout(Duration::from_secs(2), self.handler.next())
                .await
                .expect("handler must process frames while a book write waits")
                .unwrap();

            let AxDataWsMessage::MdMessage(AxMdMessage::Heartbeat(heartbeat)) = frame else {
                panic!("Expected heartbeat, received {frame:?}");
            };

            assert_eq!(heartbeat.ts, 1609459200);
            assert_eq!(heartbeat.tn, 123456789);
        }
    }

    impl Drop for HandlerConnection {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    fn create_handler(subscriptions: SubscriptionState) -> AxMdWsFeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();

        AxMdWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            subscriptions,
        )
    }
}
