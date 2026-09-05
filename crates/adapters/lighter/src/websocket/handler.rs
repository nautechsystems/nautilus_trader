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

//! Inner WebSocket feed handler running on a dedicated tokio task.

use std::{
    collections::VecDeque,
    fmt::Debug,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use futures_util::{StreamExt, stream::FuturesUnordered};
use nautilus_core::{
    AtomicTime,
    nanos::UnixNanos,
    string::secret::{REDACTED, SecretString},
    time::get_atomic_clock_realtime,
};
use nautilus_model::{identifiers::AccountId, instruments::InstrumentAny, types::Currency};
use nautilus_network::{
    RECONNECTED,
    error::SendError,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{SubscriptionState, WebSocketClient},
};
use rust_decimal::Decimal;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;
use zeroize::Zeroize;

use super::{
    account_state::LighterAccountStateReconciler,
    error::LighterWsError,
    messages::{
        AccountStream, ExecutionReport, LighterAsset, LighterPosition, LighterUserStats,
        LighterWsCandle, LighterWsChannel, LighterWsChannelKind, LighterWsFrame,
        LighterWsOrderBook, LighterWsRequest, NautilusWsMessage, SendTxRejectionSource,
    },
    parse::{
        parse_ws_bar, parse_ws_funding_rate_update, parse_ws_index_price_update,
        parse_ws_mark_price_update, parse_ws_order_book_deltas, parse_ws_order_book_depth10,
        parse_ws_position_status_report, parse_ws_quote_tick, parse_ws_spot_index_price_update,
        parse_ws_trade_tick,
    },
};
use crate::{
    common::{
        consts::{
            LIGHTER_ERROR_CODE_ALREADY_SUBSCRIBED, LIGHTER_ERROR_CODE_INTEGRATOR_NOT_APPROVED,
            LIGHTER_ERROR_CODE_TX_RANGE, LIGHTER_ERROR_CODE_WS_RATE_LIMITED,
            LIGHTER_ERROR_CODE_WS_SUBSCRIBE_FAILED, LIGHTER_INTEGRATOR_APPROVAL_DOCS_URL,
            SUBSCRIBE_INFLIGHT_MAX, SUBSCRIBE_RETRY_BASE_BACKOFF, SUBSCRIBE_RETRY_MAX,
        },
        enums::LighterCandleResolution,
        rate_limit::LIGHTER_WS_MESSAGE_RATE_LIMIT_KEY,
    },
    http::models::{LighterOrder, LighterPriceLevel, LighterTrade},
};

// Lighter control-frame `type` field values that fall outside the typed
// `LighterWsFrame` variants. These are protocol-level frames the handler
// inspects via the typed-frame parse fallback in `handle_control_value`.
const CTRL_TYPE_CONNECTED: &str = "connected";
const CTRL_TYPE_SUBSCRIBED: &str = "subscribed";
const CTRL_TYPE_UNSUBSCRIBED: &str = "unsubscribed";
const CTRL_TYPE_PING: &str = "ping";
const CTRL_TYPE_PONG: &str = "pong";
const CTRL_TYPE_ERROR: &str = "error";
const CTRL_TYPE_SEND_TX: &str = "jsonapi/sendtx";

#[derive(serde::Deserialize)]
struct LighterWsFrameHeader<'a> {
    #[serde(rename = "type", borrow)]
    kind: &'a str,
    #[serde(borrow)]
    channel: Option<&'a str>,
}

/// Commands sent from the outer client to the inner feed handler.
#[expect(
    clippy::large_enum_variant,
    reason = "commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    /// Hand the live `WebSocketClient` to the handler after the outer client
    /// completes the network connect.
    SetClient(WebSocketClient),
    /// Drain the queue and shut the handler down.
    Disconnect,
    /// Subscribe to a channel. `auth` carries an optional auth token for the
    /// account-scoped channels.
    Subscribe {
        channel: LighterWsChannel,
        auth: Option<SecretString>,
        response_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    },
    /// Unsubscribe from a channel.
    Unsubscribe { channel: LighterWsChannel },
    /// Resubscribe to the venue `order_book` stream after a continuity gap.
    ResubscribeOrderBook { market_index: i16 },
    /// Replace the handler's instrument cache (used on initial connect).
    InitializeInstruments(Vec<(i16, InstrumentAny)>),
    /// Insert or replace a single instrument by `market_index`.
    UpdateInstrument {
        market_index: i16,
        instrument: InstrumentAny,
    },
    /// Toggle whether `update/order_book` frames for `market_index` should
    /// be emitted as [`NautilusWsMessage::Deltas`].
    SetBookDeltasSub { market_index: i16, subscribed: bool },
    /// Toggle whether `update/order_book` frames for `market_index` should
    /// also be emitted as a [`NautilusWsMessage::Depth10`] snapshot.
    SetDepth10Sub { market_index: i16, subscribed: bool },
    /// Provide the execution context (`AccountId` and venue `account_index`)
    /// the handler stamps onto reports parsed from `account_*` frames.
    /// Without this context the handler cannot construct typed reports and
    /// account frames are forwarded as raw JSON instead.
    SetExecutionContext {
        account_id: AccountId,
        account_index: i64,
    },
    /// Dispatch a signed L2 transaction over the WebSocket.
    ///
    /// The handler serializes a [`LighterWsRequest::SendTx`] frame; the venue
    /// confirms acceptance via the `account_*` streams (and reports any
    /// rejection via a control-plane `error` frame).
    SendTx {
        tx_type: u8,
        tx_info: Box<serde_json::value::RawValue>,
        connection_epoch: u64,
        response_tx: tokio::sync::oneshot::Sender<Result<(), LighterWsError>>,
    },
}

impl Debug for HandlerCommand {
    /// Custom `Debug` that redacts the `auth` field of `Subscribe`. The
    /// command is routed in-process via mpsc and may be printed by error
    /// paths or trace logging; deriving `Debug` would otherwise leak the
    /// venue bearer token verbatim.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetClient(_) => f.write_str("SetClient(<WebSocketClient>)"),
            Self::Disconnect => f.write_str("Disconnect"),
            Self::Subscribe { channel, auth, .. } => f
                .debug_struct(stringify!(Subscribe))
                .field("channel", channel)
                .field("auth", auth)
                .finish(),
            Self::Unsubscribe { channel } => f
                .debug_struct(stringify!(Unsubscribe))
                .field("channel", channel)
                .finish(),
            Self::ResubscribeOrderBook { market_index } => f
                .debug_struct(stringify!(ResubscribeOrderBook))
                .field("market_index", market_index)
                .finish(),
            Self::InitializeInstruments(instruments) => f
                .debug_tuple(stringify!(InitializeInstruments))
                .field(&instruments.len())
                .finish(),
            Self::UpdateInstrument { market_index, .. } => f
                .debug_struct(stringify!(UpdateInstrument))
                .field("market_index", market_index)
                .finish(),
            Self::SetBookDeltasSub {
                market_index,
                subscribed,
            } => f
                .debug_struct(stringify!(SetBookDeltasSub))
                .field("market_index", market_index)
                .field("subscribed", subscribed)
                .finish(),
            Self::SetDepth10Sub {
                market_index,
                subscribed,
            } => f
                .debug_struct(stringify!(SetDepth10Sub))
                .field("market_index", market_index)
                .field("subscribed", subscribed)
                .finish(),
            Self::SetExecutionContext {
                account_id,
                account_index,
            } => f
                .debug_struct(stringify!(SetExecutionContext))
                .field("account_id", account_id)
                .field("account_index", account_index)
                .finish(),
            Self::SendTx { tx_type, .. } => f
                .debug_struct(stringify!(SendTx))
                .field("tx_type", tx_type)
                .field("tx_info", &REDACTED)
                .finish(),
        }
    }
}

/// Inner feed handler. Owns the [`WebSocketClient`] exclusively and routes
/// raw frames into the venue-message channel.
///
/// Subscribe dispatch is gated by a closed-loop inflight count: the venue caps
/// unacknowledged client messages per connection, and acknowledgement latency
/// is multi-second, so queued subscribes are held back until acks free a slot.
pub(super) struct FeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<(u64, Message)>,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    subscriptions: SubscriptionState,
    retry_manager: RetryManager<LighterWsError>,
    pending_messages: VecDeque<NautilusWsMessage>,
    pending_subs: VecDeque<(Ustr, u64)>,
    inflight_subs: AHashMap<Ustr, u64>,
    subscription_attempts: AHashMap<Ustr, SubscriptionAttempt>,
    subscription_retries: FuturesUnordered<SubscriptionRetry>,
    ignored_completions: AHashMap<Ustr, CompletionKind>,
    next_subscription_generation: u64,
    instruments: AHashMap<i16, InstrumentAny>,
    book_delta_subs: AHashSet<i16>,
    book_depth_10_subs: AHashSet<i16>,
    book_snapshots_seen: AHashSet<i16>,
    book_states: AHashMap<i16, CachedOrderBook>,
    last_candles: AHashMap<(i16, LighterCandleResolution), LighterWsCandle>,
    exec_account: Option<(AccountId, i64)>,
    account_state_reconciler: LighterAccountStateReconciler,
}

type SubscriptionRetry = Pin<Box<dyn Future<Output = (Ustr, u64)> + Send + Sync + 'static>>;

#[derive(Debug, Clone)]
struct CachedOrderBook {
    book: LighterWsOrderBook,
    timestamp: u64,
}

struct SubscriptionAttempt {
    channel: LighterWsChannel,
    auth: Option<SecretString>,
    pending_auth: Option<SecretString>,
    generation: u64,
    retries: u8,
    response_txs: Vec<tokio::sync::oneshot::Sender<Result<(), String>>>,
    pending_response_txs: Vec<tokio::sync::oneshot::Sender<Result<(), String>>>,
}

impl SubscriptionAttempt {
    fn fold_pending_auth(&mut self) {
        if let Some(auth) = self.pending_auth.take() {
            self.auth = Some(auth);
            self.response_txs.append(&mut self.pending_response_txs);
        } else {
            debug_assert!(self.pending_response_txs.is_empty());
        }
    }
}

/// Which venue frame produced a subscription completion.
///
/// The venue answers a subscribe with a generic control ack followed by a
/// typed `subscribed/<channel>` snapshot, and both complete by topic. The kind
/// lets `complete_subscription` swallow the predecessor's trailing frame so a
/// same-topic generation dispatched between the two cannot be completed by a
/// response that belongs to the earlier request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletionKind {
    ControlAck,
    Typed,
    AlreadySubscribed,
}

impl FeedHandler {
    #[cfg(test)]
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<(u64, Message)>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self::new_with_settlement_currency(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            subscriptions,
            Currency::get_or_create_crypto("USDC"),
        )
    }

    pub(super) fn new_with_settlement_currency(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<(u64, Message)>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        subscriptions: SubscriptionState,
        settlement_currency: Currency,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            inner: None,
            cmd_rx,
            cmd_tx: None,
            raw_rx,
            out_tx,
            subscriptions,
            retry_manager: create_websocket_retry_manager(),
            pending_messages: VecDeque::new(),
            pending_subs: VecDeque::new(),
            inflight_subs: AHashMap::new(),
            subscription_attempts: AHashMap::new(),
            subscription_retries: FuturesUnordered::new(),
            ignored_completions: AHashMap::new(),
            next_subscription_generation: 1,
            instruments: AHashMap::new(),
            book_delta_subs: AHashSet::new(),
            book_depth_10_subs: AHashSet::new(),
            book_snapshots_seen: AHashSet::new(),
            book_states: AHashMap::new(),
            last_candles: AHashMap::new(),
            exec_account: None,
            account_state_reconciler: LighterAccountStateReconciler::new_with_settlement_currency(
                settlement_currency,
            ),
        }
    }

    pub(super) fn send(&self, msg: NautilusWsMessage) -> Result<(), String> {
        self.out_tx
            .send(msg)
            .map_err(|e| format!("Failed to send message: {e}"))
    }

    pub(super) fn set_command_sender(
        &mut self,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    ) {
        self.cmd_tx = Some(cmd_tx);
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    async fn send_with_retry(&self, payload: String) -> Result<(), LighterWsError> {
        self.send_secret_with_retry(SecretString::from(payload))
            .await
    }

    async fn send_secret_with_retry(&self, payload: SecretString) -> Result<(), LighterWsError> {
        if let Some(client) = &self.inner {
            self.retry_manager
                .invocation(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        async move {
                            client
                                .send_text(
                                    payload.expose_secret().to_owned(),
                                    Some(LIGHTER_WS_MESSAGE_RATE_LIMIT_KEY.as_slice()),
                                )
                                .await
                                .map_err(LighterWsError::Transport)
                        }
                    },
                    should_retry_lighter_ws_error,
                    |e| create_lighter_ws_timeout_error(e.to_string()),
                )
                .execute()
                .await
        } else {
            Err(LighterWsError::Client(
                "no active WebSocket client".to_string(),
            ))
        }
    }

    // Single-shot: sendTx payloads carry a signed nonce; transport-layer
    // retry could double-submit if the original landed and only the ack was lost.
    async fn send_once(
        &self,
        payload: String,
        connection_epoch: u64,
    ) -> Result<(), LighterWsError> {
        if let Some(client) = &self.inner {
            match client
                .send_text_on_connection(payload, None, connection_epoch)
                .await
            {
                Err(SendError::BrokenPipe(message)) => {
                    Err(LighterWsError::SendTxOutcomeUnknown(message))
                }
                result => result.map_err(LighterWsError::Transport),
            }
        } else {
            Err(LighterWsError::Client(
                "no active WebSocket client".to_string(),
            ))
        }
    }

    async fn dispatch_subscribe(
        &self,
        channel: LighterWsChannel,
        auth: Option<SecretString>,
    ) -> Result<(), String> {
        let topic = channel.topic_key();

        let authed = auth.is_some();
        let mut request = match auth {
            Some(token) => LighterWsRequest::subscribe_auth(
                channel.subscription_channel(),
                token.expose_secret(),
            ),
            None => LighterWsRequest::subscribe(channel.subscription_channel()),
        };

        let payload = serde_json::to_string(&request);
        request.zeroize();

        match payload {
            Ok(payload) => {
                // Avoid logging the serialized payload for authenticated channels;
                // it embeds a live Lighter L2 bearer token.
                log::debug!("Sending Lighter subscribe: topic={topic} authed={authed}");
                let result = if authed {
                    self.send_secret_with_retry(SecretString::from(payload))
                        .await
                } else {
                    self.send_with_retry(payload).await
                };

                if let Err(e) = result {
                    log::error!("Error subscribing to {topic}: {e}");
                    Err(e.to_string())
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                log::error!("Error serializing subscription for {topic}: {e}");
                Err(format!("failed to serialize subscription for {topic}: {e}"))
            }
        }
    }

    async fn dispatch_send_tx(
        &self,
        tx_type: u8,
        tx_info: Box<serde_json::value::RawValue>,
        connection_epoch: u64,
    ) -> Result<(), LighterWsError> {
        let request = LighterWsRequest::SendTx {
            data: super::messages::LighterWsSendTx { tx_type, tx_info },
        };

        match serde_json::to_string(&request) {
            Ok(payload) => {
                log::debug!(
                    "Sending Lighter sendTx: tx_type={tx_type} ({} bytes)",
                    payload.len(),
                );

                match self.send_once(payload, connection_epoch).await {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        log::error!("Error dispatching Lighter sendTx (tx_type={tx_type}): {e}");
                        Err(e)
                    }
                }
            }
            Err(e) => {
                log::error!("Error serializing Lighter sendTx (tx_type={tx_type}): {e}");
                Err(LighterWsError::Client(format!(
                    "failed to serialize Lighter sendTx: {e}"
                )))
            }
        }
    }

    async fn dispatch_unsubscribe(&self, channel: LighterWsChannel) {
        let topic = channel.topic_key();
        self.subscriptions.mark_unsubscribe(&topic);

        let request = LighterWsRequest::unsubscribe(channel.subscription_channel());
        match serde_json::to_string(&request) {
            Ok(payload) => {
                log::debug!("Sending Lighter unsubscribe ({} bytes)", payload.len());
                if let Err(e) = self.send_with_retry(payload).await {
                    log::error!("Error unsubscribing from {topic}: {e}");
                }
            }
            Err(e) => {
                log::error!("Error serializing unsubscription for {topic}: {e}");
            }
        }
    }

    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        if let Some(msg) = self.pending_messages.pop_front() {
            return Some(msg);
        }

        loop {
            // Pump each iteration: the select below drains acks that free slots
            self.pump_pending_subscribes().await;

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("Setting WebSocket client in Lighter handler");
                            self.inner = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Lighter handler received disconnect");
                            if let Some(ref client) = self.inner {
                                client.disconnect().await;
                            }
                            self.signal.store(true, Ordering::SeqCst);
                            return None;
                        }
                        HandlerCommand::Subscribe {
                            channel,
                            auth,
                            response_tx,
                        } => {
                            self.queue_subscribe(channel, auth, response_tx);
                        }
                        HandlerCommand::Unsubscribe { channel } => {
                            // Drop a queued-but-unsent subscribe for this channel so a freed
                            // slot cannot resubscribe after the caller unsubscribed.
                            let topic = channel.topic_key();
                            self.cancel_subscription_attempt(
                                &topic,
                                "subscription cancelled before venue acknowledgement",
                            );

                            if let LighterWsChannel::OrderBook(market_index) = &channel {
                                self.book_snapshots_seen.remove(market_index);
                                self.book_states.remove(market_index);
                            }
                            self.dispatch_unsubscribe(channel).await;
                        }
                        HandlerCommand::ResubscribeOrderBook { market_index } => {
                            self.resubscribe_order_book_stream(market_index).await;
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            self.instruments.clear();
                            for (market_index, inst) in instruments {
                                self.instruments.insert(market_index, inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument { market_index, instrument } => {
                            self.instruments.insert(market_index, instrument);
                        }
                        HandlerCommand::SetBookDeltasSub { market_index, subscribed } => {
                            if subscribed {
                                let inserted = self.book_delta_subs.insert(market_index);
                                if inserted
                                    && let Some(first) = self
                                        .emit_cached_order_book_deltas_snapshot(market_index)
                                {
                                    return Some(first);
                                }
                            } else {
                                self.book_delta_subs.remove(&market_index);
                            }
                        }
                        HandlerCommand::SetDepth10Sub { market_index, subscribed } => {
                            if subscribed {
                                let inserted = self.book_depth_10_subs.insert(market_index);
                                if inserted
                                    && let Some(first) =
                                        self.emit_cached_order_book_depth10_snapshot(market_index)
                                {
                                    return Some(first);
                                }
                            } else {
                                self.book_depth_10_subs.remove(&market_index);
                            }
                        }
                        HandlerCommand::SetExecutionContext { account_id, account_index } => {
                            self.exec_account = Some((account_id, account_index));
                        }
                        HandlerCommand::SendTx {
                            tx_type,
                            tx_info,
                            connection_epoch,
                            response_tx,
                        } => {
                            let result = self
                                .dispatch_send_tx(tx_type, tx_info, connection_epoch)
                                .await;

                            if response_tx.send(result).is_err() {
                                log::debug!("Lighter sendTx result receiver dropped");
                            }
                        }
                    }
                }
                Some((topic, generation)) = self.subscription_retries.next(),
                    if !self.subscription_retries.is_empty() =>
                {
                    self.queue_subscription_retry(topic, generation);
                }
                Some((connection_epoch, raw_msg)) = self.raw_rx.recv() => {
                    match raw_msg {
                        Message::Text(text) => {
                            if text == RECONNECTED {
                                log::debug!("Received Lighter WebSocket RECONNECTED sentinel");
                                self.book_snapshots_seen.clear();
                                self.book_states.clear();
                                // Resubscribe replays a fresh `subscribed/candle`; pre-disconnect cache is stale.
                                self.last_candles.clear();
                                // The new socket starts with zero inflight and restore_subscriptions
                                // re-queues every topic. Retire stale generations before accepting
                                // replay commands so late work cannot complete a replacement attempt.
                                self.reset_subscription_attempts_after_reconnect();
                                self.account_state_reconciler.reset();
                                return Some(NautilusWsMessage::Reconnected {
                                    connection_epoch,
                                });
                            }

                            let ts_init = self.clock.get_time_ns();

                            if let Ok(frame) = serde_json::from_str::<LighterWsFrame>(&text) {
                                if let Some(topic) = typed_subscribe_topic(&text) {
                                    self.complete_subscription(topic, CompletionKind::Typed);
                                }
                                let messages = self
                                    .handle_frame(frame, ts_init)
                                    .into_iter()
                                    .map(|msg| msg.with_connection_epoch(connection_epoch))
                                    .collect();

                                if let Some(first) = self.dispatch_results(messages) {
                                    return Some(first);
                                }
                            } else if let Ok(value) =
                                serde_json::from_str::<serde_json::Value>(&text)
                            {
                                let (matched, msg) = self.handle_control_value(&value);
                                if let Some(first) = msg {
                                    return Some(first.with_connection_epoch(connection_epoch));
                                }

                                if !matched {
                                    // Neither a typed frame nor a recognized
                                    // control type. Surface raw so venue
                                    // errors (bad signature, margin rejection,
                                    // ...) don't drop silently.
                                    log::warn!("Lighter WS unparsed frame: {value}");
                                    return Some(NautilusWsMessage::Raw(value));
                                }
                            } else {
                                log::warn!("Lighter WS non-JSON text: {text}");
                            }
                        }
                        Message::Ping(data) => {
                            if let Some(ref client) = self.inner
                                && let Err(e) = client.send_pong(data.to_vec()).await {
                                log::error!("Error sending Lighter pong: {e}");
                            }
                        }
                        Message::Close(frame) => {
                            log::debug!("Received Lighter WebSocket close frame: {frame:?}");
                            return None;
                        }
                        _ => {}
                    }
                }
                else => {
                    log::debug!("Lighter handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    fn dispatch_results(
        &mut self,
        mut messages: Vec<NautilusWsMessage>,
    ) -> Option<NautilusWsMessage> {
        if messages.is_empty() {
            return None;
        }
        let first = messages.remove(0);
        for extra in messages {
            self.pending_messages.push_back(extra);
        }
        Some(first)
    }

    // Non-blocking count check, not a permit await: this task also drains the
    // acks that free slots, so awaiting a permit here would deadlock.
    async fn pump_pending_subscribes(&mut self) {
        while self.inflight_subs.len() < SUBSCRIBE_INFLIGHT_MAX {
            let Some((topic, generation)) = self.pending_subs.pop_front() else {
                break;
            };

            let Some(attempt) = self.subscription_attempts.get(&topic) else {
                continue;
            };

            if attempt.generation != generation {
                continue;
            }

            let channel = attempt.channel.clone();
            let auth = attempt.auth.clone();
            self.inflight_subs.insert(topic, generation);
            if let Err(message) = self.dispatch_subscribe(channel, auth).await {
                self.schedule_subscription_retry(topic, generation, &message);
            }
        }
    }

    fn queue_subscribe(
        &mut self,
        channel: LighterWsChannel,
        auth: Option<SecretString>,
        response_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) {
        let topic = Ustr::from(channel.topic_key().as_str());
        if let Some(attempt) = self.subscription_attempts.get_mut(&topic) {
            attempt.channel = channel;
            let effective_auth = attempt
                .pending_auth
                .as_ref()
                .or(attempt.auth.as_ref())
                .map(SecretString::expose_secret);
            let auth_changed = auth
                .as_ref()
                .map(SecretString::expose_secret)
                .is_some_and(|auth| Some(auth) != effective_auth);

            if auth_changed {
                let is_inflight = self.inflight_subs.get(&topic) == Some(&attempt.generation);
                if is_inflight {
                    attempt.pending_auth = auth;
                    if let Some(response_tx) = response_tx {
                        attempt.pending_response_txs.push(response_tx);
                    }
                } else {
                    debug_assert!(attempt.pending_auth.is_none());
                    debug_assert!(attempt.pending_response_txs.is_empty());
                    attempt.auth = auth;
                    if let Some(response_tx) = response_tx {
                        attempt.response_txs.push(response_tx);
                    }
                }
            } else if let Some(response_tx) = response_tx {
                if auth.is_some() && attempt.pending_auth.is_some() {
                    attempt.pending_response_txs.push(response_tx);
                } else {
                    attempt.response_txs.push(response_tx);
                }
            }
            return;
        }

        let newly_pending = self.subscriptions.try_mark_subscribe(topic.as_str());
        if !newly_pending
            && auth.is_none()
            && !self
                .subscriptions
                .pending_subscribe_topics()
                .iter()
                .any(|pending| pending == topic.as_str())
        {
            if let Some(response_tx) = response_tx {
                let _ = response_tx.send(Ok(()));
            }
            return;
        }

        let generation = self.take_subscription_generation();
        self.subscription_attempts.insert(
            topic,
            SubscriptionAttempt {
                channel,
                auth,
                pending_auth: None,
                generation,
                retries: 0,
                response_txs: response_tx.into_iter().collect(),
                pending_response_txs: Vec::new(),
            },
        );
        self.pending_subs.push_back((topic, generation));
    }

    fn queue_subscription_retry(&mut self, topic: Ustr, generation: u64) {
        if self
            .subscription_attempts
            .get(&topic)
            .is_some_and(|attempt| attempt.generation == generation)
            && self.inflight_subs.get(&topic) != Some(&generation)
            && !self
                .pending_subs
                .iter()
                .any(|pending| *pending == (topic, generation))
        {
            self.pending_subs.push_back((topic, generation));
        }
    }

    fn complete_subscription(&mut self, topic: &str, kind: CompletionKind) -> bool {
        let topic = Ustr::from(topic);

        // Swallow the predecessor's trailing frame before the inflight guard,
        // so it cannot complete a same-topic generation dispatched between the
        // venue's control ack and its typed snapshot - whether that generation
        // is a serialized successor, a fresh attempt, queued, or in backoff.
        if self.ignored_completions.get(&topic) == Some(&kind) {
            self.ignored_completions.remove(&topic);
            return false;
        }

        let Some(generation) = self.inflight_subs.get(&topic).copied() else {
            return false;
        };

        if !self
            .subscription_attempts
            .get(&topic)
            .is_some_and(|attempt| attempt.generation == generation)
        {
            return false;
        }

        self.inflight_subs.remove(&topic);
        self.subscriptions.confirm_subscribe(topic.as_str());

        // The venue contract is control ack first, typed snapshot second, so a
        // control-ack completion owes one trailing typed frame. Typed and
        // already-subscribed completions have no established trailing frame.
        if kind == CompletionKind::ControlAck {
            self.ignored_completions
                .insert(topic, CompletionKind::Typed);
        }
        let mut attempt = self
            .subscription_attempts
            .remove(&topic)
            .expect("matching subscription attempt disappeared");
        for response_tx in std::mem::take(&mut attempt.response_txs) {
            let _ = response_tx.send(Ok(()));
        }

        if let Some(auth) = attempt.pending_auth.take() {
            let generation = self.take_subscription_generation();
            attempt.auth = Some(auth);
            attempt.generation = generation;
            attempt.retries = 0;
            attempt.response_txs = std::mem::take(&mut attempt.pending_response_txs);
            self.subscription_attempts.insert(topic, attempt);
            self.pending_subs.push_back((topic, generation));
        } else {
            debug_assert!(attempt.pending_response_txs.is_empty());
        }
        true
    }

    fn retry_inflight_subscriptions(&mut self, message: &str) {
        let mut inflight: Vec<(Ustr, u64)> = self
            .inflight_subs
            .iter()
            .map(|(topic, generation)| (*topic, *generation))
            .collect();
        inflight.sort_unstable_by_key(|(topic, _)| *topic);

        for (topic, generation) in inflight {
            self.schedule_subscription_retry(topic, generation, message);
        }
    }

    fn fail_inflight_subscriptions(&mut self, message: &str) {
        let mut topics: Vec<Ustr> = self.inflight_subs.keys().copied().collect();
        topics.sort_unstable();

        for topic in topics {
            self.fail_subscription_attempt(topic, message);
        }
    }

    fn schedule_subscription_retry(&mut self, topic: Ustr, generation: u64, message: &str) {
        if self.inflight_subs.get(&topic) != Some(&generation) {
            return;
        }
        self.inflight_subs.remove(&topic);
        self.subscriptions.mark_failure(topic.as_str());

        let Some(attempt) = self.subscription_attempts.get_mut(&topic) else {
            return;
        };

        if attempt.generation != generation {
            return;
        }

        attempt.fold_pending_auth();
        if attempt.retries >= SUBSCRIBE_RETRY_MAX {
            self.fail_subscription_attempt(topic, message);
            return;
        }

        attempt.retries += 1;
        let retry = attempt.retries;
        let next_generation = self.take_subscription_generation();
        let attempt = self
            .subscription_attempts
            .get_mut(&topic)
            .expect("subscription attempt disappeared before retry");
        attempt.generation = next_generation;

        let delay = SUBSCRIBE_RETRY_BASE_BACKOFF.saturating_mul(1_u32 << (retry - 1));
        self.subscription_retries.push(Box::pin(async move {
            tokio::time::sleep(delay).await;
            (topic, next_generation)
        }));
    }

    fn fail_subscription_attempt(&mut self, topic: Ustr, message: &str) {
        self.inflight_subs.remove(&topic);
        self.pending_subs.retain(|(pending, _)| *pending != topic);
        self.subscriptions.mark_unsubscribe(topic.as_str());
        self.subscriptions.confirm_unsubscribe(topic.as_str());

        if let Some(attempt) = self.subscription_attempts.remove(&topic) {
            let attempts = attempt.retries + 1;
            for response_tx in attempt
                .response_txs
                .into_iter()
                .chain(attempt.pending_response_txs)
            {
                let _ = response_tx.send(Err(format!(
                    "subscription {topic} failed after {attempts} attempts: {message}",
                )));
            }
        }
    }

    fn cancel_subscription_attempt(&mut self, topic: &str, message: &str) {
        let topic = Ustr::from(topic);
        self.inflight_subs.remove(&topic);
        self.pending_subs.retain(|(pending, _)| *pending != topic);
        if let Some(attempt) = self.subscription_attempts.remove(&topic) {
            for response_tx in attempt
                .response_txs
                .into_iter()
                .chain(attempt.pending_response_txs)
            {
                let _ = response_tx.send(Err(format!("{message}: {topic}")));
            }
        }
    }

    fn reset_subscription_attempts_after_reconnect(&mut self) {
        for topic in self.subscriptions.all_topics() {
            self.subscriptions.mark_failure(&topic);
        }
        self.pending_subs.clear();
        self.inflight_subs.clear();
        // Completion frames from the old connection must not be applied to -
        // or swallowed from - the new connection.
        self.ignored_completions.clear();

        let mut topics: Vec<Ustr> = self.subscription_attempts.keys().copied().collect();
        topics.sort_unstable();
        for topic in topics {
            let generation = self.take_subscription_generation();
            let attempt = self
                .subscription_attempts
                .get_mut(&topic)
                .expect("subscription attempt disappeared during reconnect");
            attempt.fold_pending_auth();
            attempt.generation = generation;
            attempt.retries = 0;
            self.pending_subs.push_back((topic, generation));
        }
    }

    fn take_subscription_generation(&mut self) -> u64 {
        let generation = self.next_subscription_generation;
        self.next_subscription_generation =
            self.next_subscription_generation.wrapping_add(1).max(1);
        generation
    }

    /// Returns `(matched, msg)` where `matched=true` means the frame's
    /// `type` was recognized as a known control type (whether or not a
    /// message is emitted); `matched=false` lets the caller surface the
    /// raw frame so venue errors with unknown shapes aren't lost.
    fn handle_control_value(
        &mut self,
        value: &serde_json::Value,
    ) -> (bool, Option<NautilusWsMessage>) {
        if let Some(error) = already_subscribed_error(value) {
            self.confirm_already_subscribed(error);
            return (true, None);
        }
        let subscription_code = subscription_error_code(value);

        if subscription_code == Some(LIGHTER_ERROR_CODE_WS_RATE_LIMITED) {
            self.retry_inflight_subscriptions(&format!(
                "venue rejected the WebSocket subscribe with code \
                 {LIGHTER_ERROR_CODE_WS_RATE_LIMITED}",
            ));
            return (true, None);
        }

        if subscription_code == Some(LIGHTER_ERROR_CODE_WS_SUBSCRIBE_FAILED) {
            self.fail_inflight_subscriptions(&format!(
                "venue rejected the WebSocket subscribe with code \
                 {LIGHTER_ERROR_CODE_WS_SUBSCRIBE_FAILED}",
            ));
            return (true, None);
        }

        let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match kind {
            CTRL_TYPE_CONNECTED => {
                log::debug!("Lighter WebSocket handshake complete");
                (true, None)
            }
            CTRL_TYPE_PING | CTRL_TYPE_PONG => (true, None),
            CTRL_TYPE_SEND_TX => {
                let raw_code = value.get("code").and_then(|v| v.as_u64());
                match raw_code {
                    Some(LIGHTER_ERROR_CODE_INTEGRATOR_NOT_APPROVED) => {
                        log_integrator_not_approved();
                        (
                            true,
                            Some(send_tx_rejected_from_value(
                                value,
                                SendTxRejectionSource::Ack,
                            )),
                        )
                    }
                    Some(200) => {
                        log::debug!("Lighter WebSocket sendTx ack: {value}");
                        let tx_hash = value
                            .get("tx_hash")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        (
                            true,
                            Some(NautilusWsMessage::SendTxAck {
                                connection_epoch: 0,
                                tx_hash,
                                code: 200,
                            }),
                        )
                    }
                    Some(_) => {
                        log::error!("Lighter sendTx rejected: {value}");
                        (
                            true,
                            Some(send_tx_rejected_from_value(
                                value,
                                SendTxRejectionSource::Ack,
                            )),
                        )
                    }
                    None => {
                        log::warn!(
                            "Ignoring malformed Lighter sendTx response without numeric code: {value}",
                        );
                        (true, None)
                    }
                }
            }
            CTRL_TYPE_SUBSCRIBED | CTRL_TYPE_UNSUBSCRIBED => {
                if let Some(topic) = value.get("channel").and_then(|v| v.as_str()) {
                    if kind == CTRL_TYPE_SUBSCRIBED {
                        self.complete_subscription(topic, CompletionKind::ControlAck);
                    } else {
                        let was_pending_unsubscribe = self
                            .subscriptions
                            .pending_unsubscribe_topics()
                            .iter()
                            .any(|pending| pending == topic);
                        self.subscriptions.confirm_unsubscribe(topic);

                        if was_pending_unsubscribe {
                            // Only matched unsubscribe ACKs should reset stream state
                            if let Some(market_index) = order_book_market_index_from_topic(topic) {
                                self.clear_cached_order_book(market_index);
                            }

                            if let Some(key) = candle_market_and_resolution_from_topic(topic) {
                                self.last_candles.remove(&key);
                            }
                        }
                    }
                }
                (true, None)
            }
            CTRL_TYPE_ERROR => {
                let code = value.get("code").and_then(|v| v.as_u64());
                if code == Some(LIGHTER_ERROR_CODE_INTEGRATOR_NOT_APPROVED) {
                    log_integrator_not_approved();
                } else {
                    log::warn!("Lighter WebSocket error frame: {value}");
                }

                if is_sendtx_error_code(code) {
                    (
                        true,
                        Some(send_tx_rejected_from_value(
                            value,
                            SendTxRejectionSource::BareError,
                        )),
                    )
                } else {
                    (true, None)
                }
            }
            _ => {
                if let Some(error) = value.get("error") {
                    let nested_code = error.get("code").and_then(|v| v.as_u64());
                    if nested_code == Some(LIGHTER_ERROR_CODE_INTEGRATOR_NOT_APPROVED) {
                        log_integrator_not_approved();
                    } else {
                        log::warn!("Lighter WebSocket error frame: {value}");
                    }
                    let rejected = is_sendtx_error_code(nested_code).then(|| {
                        send_tx_rejected_from_nested_error(error, SendTxRejectionSource::BareError)
                    });
                    return (true, rejected);
                }
                (false, None)
            }
        }
    }

    fn confirm_already_subscribed(&mut self, error: &serde_json::Value) {
        let Some(topic) = error
            .get("message")
            .and_then(|value| value.as_str())
            .and_then(|message| message.strip_prefix("Already Subscribed to : "))
        else {
            log::debug!(
                "Lighter WebSocket subscription response: code={LIGHTER_ERROR_CODE_ALREADY_SUBSCRIBED}",
            );
            return;
        };

        if !self.complete_subscription(topic, CompletionKind::AlreadySubscribed) {
            log::debug!(
                "Lighter WebSocket subscription response: code={LIGHTER_ERROR_CODE_ALREADY_SUBSCRIBED}",
            );
            return;
        }

        log::debug!(
            "Lighter WebSocket subscription response: code={LIGHTER_ERROR_CODE_ALREADY_SUBSCRIBED}, topic={topic}",
        );
    }

    fn handle_frame(
        &mut self,
        frame: LighterWsFrame,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        match frame {
            LighterWsFrame::OrderBookSnapshot {
                channel,
                order_book,
                timestamp,
                ..
            } => self.handle_order_book(channel, &order_book, timestamp, true, ts_init),
            LighterWsFrame::OrderBook {
                channel,
                order_book,
                timestamp,
                ..
            } => self.handle_order_book(channel, &order_book, timestamp, false, ts_init),
            LighterWsFrame::TickerSnapshot {
                channel,
                ticker,
                timestamp,
                ..
            }
            | LighterWsFrame::Ticker {
                channel,
                ticker,
                timestamp,
                ..
            } => self.handle_ticker(channel, &ticker, timestamp, ts_init),
            LighterWsFrame::TradeSnapshot {
                trades,
                liquidation_trades,
                ..
            }
            | LighterWsFrame::Trade {
                trades,
                liquidation_trades,
                ..
            } => self.handle_trades(&trades, &liquidation_trades, ts_init),
            LighterWsFrame::AccountOrders { ref orders, .. }
            | LighterWsFrame::AccountAllOrders { ref orders, .. } => {
                if self.exec_account.is_none() {
                    return raw_message(&frame);
                }
                let mut msgs = self.handle_account_orders(orders, ts_init);
                msgs.push(NautilusWsMessage::AccountStreamFirstFrame(
                    AccountStream::Orders,
                ));
                msgs
            }
            // Lighter publishes the historical fill replay as `subscribed/`
            // rather than `update/`. When typed execution routing is active
            // we drop the snapshot to avoid re-emitting prior executions as
            // fresh fill reports on every reconnect (HTTP reconciliation
            // owns historical state recovery). When no execution context is
            // set we still forward as `Raw` to preserve the prior contract
            // for unauthenticated subscribers.
            LighterWsFrame::AccountAllTradesSnapshot { .. } => {
                if self.exec_account.is_none() {
                    return raw_message(&frame);
                }
                log::debug!(
                    "Skipping Lighter account_all_trades snapshot frame; \
                     reconcile historical fills via HTTP",
                );
                vec![NautilusWsMessage::AccountStreamFirstFrame(
                    AccountStream::Trades,
                )]
            }
            LighterWsFrame::AccountAllTrades { ref trades, .. } => {
                if self.exec_account.is_none() {
                    return raw_message(&frame);
                }
                let mut msgs = self.handle_account_trades(trades.values().flatten(), ts_init);
                msgs.push(NautilusWsMessage::AccountStreamFirstFrame(
                    AccountStream::Trades,
                ));
                msgs
            }
            LighterWsFrame::AccountAllPositionsSnapshot { ref positions, .. } => {
                if self.exec_account.is_none() {
                    return raw_message(&frame);
                }
                let mut msgs =
                    self.handle_account_positions(positions, ts_init, PositionFrameType::Snapshot);
                msgs.push(NautilusWsMessage::AccountStreamFirstFrame(
                    AccountStream::Positions,
                ));
                msgs
            }
            LighterWsFrame::AccountAllPositions { ref positions, .. } => {
                if self.exec_account.is_none() {
                    return raw_message(&frame);
                }
                self.handle_account_positions(positions, ts_init, PositionFrameType::Update)
            }
            LighterWsFrame::AccountAllAssets {
                ref assets,
                timestamp,
                ..
            } => {
                if self.exec_account.is_none() {
                    return raw_message(&frame);
                }
                let mut msgs = self.handle_account_assets(assets, timestamp, ts_init);
                msgs.push(NautilusWsMessage::AccountStreamFirstFrame(
                    AccountStream::Assets,
                ));
                msgs
            }
            LighterWsFrame::UserStats {
                ref stats,
                timestamp,
                ..
            } => {
                if self.exec_account.is_none() {
                    return raw_message(&frame);
                }
                let mut msgs = self.handle_user_stats(stats, timestamp, ts_init);
                msgs.push(NautilusWsMessage::AccountStreamFirstFrame(
                    AccountStream::UserStats,
                ));
                msgs
            }
            LighterWsFrame::MarketStats {
                ref market_stats,
                timestamp,
                ..
            } => self.handle_market_stats(market_stats, timestamp, ts_init),
            LighterWsFrame::SpotMarketStats {
                ref spot_market_stats,
                timestamp,
                ..
            } => self.handle_spot_market_stats(spot_market_stats, timestamp, ts_init),
            LighterWsFrame::CandleSnapshot {
                channel,
                ref candles,
                ..
            }
            | LighterWsFrame::Candle {
                channel,
                ref candles,
                ..
            } => self.handle_candles(channel, candles, ts_init),
            LighterWsFrame::Height { .. } => raw_message(&frame),
        }
    }

    fn handle_order_book(
        &mut self,
        channel: Ustr,
        book: &super::messages::LighterWsOrderBook,
        timestamp: u64,
        is_snapshot: bool,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let market_index = match market_index_from_topic(channel.as_str()) {
            Some(index) => index,
            None => {
                log::debug!("Lighter order_book frame missing market index in channel '{channel}'");
                return Vec::new();
            }
        };

        if !self.instruments.contains_key(&market_index) {
            log::debug!("No instrument cached for Lighter market_index={market_index}");
            return Vec::new();
        }

        if !self.book_delta_subs.contains(&market_index)
            && !self.book_depth_10_subs.contains(&market_index)
        {
            return Vec::new();
        }

        // The venue tags the initial book as `subscribed/order_book` and follows
        // up with `update/order_book` for incrementals. An incremental cannot
        // seed the book because it only carries changed levels.
        if !is_snapshot && !self.book_snapshots_seen.contains(&market_index) {
            log::warn!(
                "Dropping Lighter order_book update before snapshot for market_index={market_index}",
            );
            return Vec::new();
        }

        if is_snapshot {
            self.book_snapshots_seen.insert(market_index);
            self.book_states.insert(
                market_index,
                CachedOrderBook {
                    book: book.clone(),
                    timestamp,
                },
            );
        } else if let Some(cached_nonce) = self
            .book_states
            .get(&market_index)
            .map(|state| state.book.nonce)
        {
            if book.begin_nonce != cached_nonce {
                log::warn!(
                    "Dropping Lighter order_book update with nonce gap for \
                     market_index={market_index}: begin_nonce={}, cached_nonce={cached_nonce}",
                    book.begin_nonce,
                );
                self.clear_cached_order_book(market_index);
                self.queue_order_book_resync(market_index);
                return Vec::new();
            }

            if let Some(state) = self.book_states.get_mut(&market_index) {
                apply_order_book_update(&mut state.book, book);
                state.timestamp = timestamp;
            }
        } else {
            log::warn!(
                "Dropping Lighter order_book update without cached state for \
                 market_index={market_index}",
            );
            self.clear_cached_order_book(market_index);
            self.queue_order_book_resync(market_index);
            return Vec::new();
        }

        self.order_book_messages(market_index, book, timestamp, is_snapshot, ts_init)
    }

    fn clear_cached_order_book(&mut self, market_index: i16) {
        self.book_snapshots_seen.remove(&market_index);
        self.book_states.remove(&market_index);
    }

    fn queue_order_book_resync(&self, market_index: i16) {
        if !self.order_book_stream_is_referenced(market_index) {
            log::debug!(
                "Skipping Lighter order_book resync: subscription cancelled, \
                 market_index={market_index}",
            );
            return;
        }

        let Some(cmd_tx) = &self.cmd_tx else {
            log::error!(
                "Cannot resync Lighter order_book stream without command sender: \
                 market_index={market_index}",
            );
            return;
        };

        if let Err(e) = cmd_tx.send(HandlerCommand::ResubscribeOrderBook { market_index }) {
            log::error!("Failed to queue Lighter order_book resync: {e}");
        }
    }

    async fn resubscribe_order_book_stream(&mut self, market_index: i16) {
        if !self.order_book_stream_is_referenced(market_index) {
            log::debug!(
                "Skipping Lighter order_book resync: subscription cancelled before venue \
                 unsubscribe, market_index={market_index}",
            );
            return;
        }

        let channel = LighterWsChannel::OrderBook(market_index);
        self.dispatch_unsubscribe(channel.clone()).await;

        if !self.order_book_stream_is_referenced(market_index) {
            log::debug!(
                "Skipping Lighter order_book resubscribe: subscription cancelled after venue \
                 unsubscribe, market_index={market_index}",
            );
            return;
        }

        let topic = Ustr::from(channel.topic_key().as_str());
        self.queue_subscribe(channel.clone(), None, None);
        self.pump_pending_subscribes().await;

        if !self.order_book_stream_is_referenced(market_index) {
            log::debug!(
                "Cancelling Lighter order_book resync subscribe after user unsubscribe: \
                 market_index={market_index}",
            );
            let attempted = self.inflight_subs.contains_key(&topic);
            self.cancel_subscription_attempt(topic.as_str(), "order book resubscription cancelled");

            if attempted {
                self.dispatch_unsubscribe(channel).await;
            }
        }
    }

    fn order_book_stream_is_referenced(&self, market_index: i16) -> bool {
        let channel = LighterWsChannel::OrderBook(market_index);
        self.subscriptions.get_reference_count(&channel.topic_key()) > 0
            && (self.book_delta_subs.contains(&market_index)
                || self.book_depth_10_subs.contains(&market_index))
    }

    fn emit_cached_order_book_deltas_snapshot(
        &self,
        market_index: i16,
    ) -> Option<NautilusWsMessage> {
        let cached = self.book_states.get(&market_index)?.clone();
        let instrument = self.instruments.get(&market_index)?;
        let ts_init = self.clock.get_time_ns();
        match parse_ws_order_book_deltas(&cached.book, instrument, cached.timestamp, true, ts_init)
        {
            Ok(deltas) => Some(NautilusWsMessage::Deltas(deltas)),
            Err(e) => {
                log::error!("Error parsing cached Lighter order_book deltas: {e}");
                None
            }
        }
    }

    fn emit_cached_order_book_depth10_snapshot(
        &self,
        market_index: i16,
    ) -> Option<NautilusWsMessage> {
        let cached = self.book_states.get(&market_index)?.clone();
        let instrument = self.instruments.get(&market_index)?;
        let ts_init = self.clock.get_time_ns();
        match parse_ws_order_book_depth10(&cached.book, instrument, cached.timestamp, ts_init) {
            Ok(depth) => Some(NautilusWsMessage::Depth10(Box::new(depth))),
            Err(e) => {
                log::error!("Error parsing cached Lighter order_book depth10: {e}");
                None
            }
        }
    }

    fn order_book_messages(
        &self,
        market_index: i16,
        book: &LighterWsOrderBook,
        timestamp: u64,
        is_snapshot: bool,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let Some(instrument) = self.instruments.get(&market_index) else {
            log::debug!("No instrument cached for Lighter market_index={market_index}");
            return Vec::new();
        };

        let mut messages = Vec::new();

        if self.book_delta_subs.contains(&market_index) {
            match parse_ws_order_book_deltas(book, instrument, timestamp, is_snapshot, ts_init) {
                Ok(deltas) => messages.push(NautilusWsMessage::Deltas(deltas)),
                Err(e) => log::error!("Error parsing Lighter order_book deltas: {e}"),
            }
        }

        if self.book_depth_10_subs.contains(&market_index)
            && let Some(cached) = self.book_states.get(&market_index)
        {
            match parse_ws_order_book_depth10(&cached.book, instrument, cached.timestamp, ts_init) {
                Ok(depth) => messages.push(NautilusWsMessage::Depth10(Box::new(depth))),
                Err(e) => log::error!("Error parsing Lighter order_book depth10: {e}"),
            }
        }

        messages
    }

    fn handle_ticker(
        &self,
        channel: Ustr,
        ticker: &super::messages::LighterTicker,
        timestamp: u64,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        // Resolve the instrument from the channel's `ticker:N` market index,
        // not the ticker payload's `s` symbol. Lighter sends the unsuffixed
        // venue symbol (e.g. "ETH"), while cached `raw_symbol` carries the
        // -PERP / -SPOT suffix, so a symbol-name match never resolves and
        // would conflate spot vs perp listings of the same asset.
        let Some(market_index) = market_index_from_topic(channel.as_str()) else {
            log::debug!("Lighter ticker frame missing market index in channel '{channel}'");
            return Vec::new();
        };

        let Some(instrument) = self.instruments.get(&market_index) else {
            log::debug!("No instrument cached for Lighter ticker market_index={market_index}");
            return Vec::new();
        };

        match parse_ws_quote_tick(ticker, instrument, timestamp, ts_init) {
            Ok(Some(quote)) => vec![NautilusWsMessage::Quote(quote)],
            Ok(None) => {
                log::debug!(
                    "Skipping Lighter ticker for market_index={market_index}: one-sided book",
                );
                Vec::new()
            }
            Err(e) => {
                log::error!("Error parsing Lighter ticker frame: {e}");
                Vec::new()
            }
        }
    }

    fn handle_trades(
        &self,
        trades: &[crate::http::models::LighterTrade],
        liquidation_trades: &[crate::http::models::LighterTrade],
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        // Lighter splits trade prints across `trades` and `liquidation_trades`
        // on the same `update/trade` frame; both share the LighterTrade model
        // and are public executions, so emit them together.
        let Some(market_index) = trades
            .first()
            .or_else(|| liquidation_trades.first())
            .map(|t| t.market_id)
        else {
            return Vec::new();
        };

        let Some(instrument) = self.instruments.get(&market_index) else {
            log::debug!("No instrument cached for Lighter trade market_index={market_index}");
            return Vec::new();
        };

        let mut ticks = Vec::with_capacity(trades.len() + liquidation_trades.len());
        for trade in trades.iter().chain(liquidation_trades.iter()) {
            match parse_ws_trade_tick(trade, instrument, ts_init) {
                Ok(tick) => ticks.push(tick),
                Err(e) => log::error!("Error parsing Lighter trade tick: {e}"),
            }
        }

        if ticks.is_empty() {
            Vec::new()
        } else {
            vec![NautilusWsMessage::Trades(ticks)]
        }
    }

    fn handle_market_stats(
        &self,
        payload: &super::messages::LighterMarketStatsPayload,
        timestamp: u64,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        match payload {
            super::messages::LighterMarketStatsPayload::All(stats) => stats
                .values()
                .flat_map(|stats| self.handle_one_market_stats(stats, timestamp, ts_init))
                .collect(),
            super::messages::LighterMarketStatsPayload::One(stats) => {
                self.handle_one_market_stats(stats, timestamp, ts_init)
            }
        }
    }

    fn handle_one_market_stats(
        &self,
        stats: &super::messages::LighterMarketStats,
        timestamp: u64,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let Some(instrument) = self.instruments.get(&stats.market_id) else {
            log::debug!(
                "No instrument cached for Lighter market_stats market_id={}",
                stats.market_id,
            );
            return Vec::new();
        };

        let mut messages = Vec::with_capacity(3);

        match parse_ws_mark_price_update(stats, instrument, timestamp, ts_init) {
            Ok(mark_price) => messages.push(NautilusWsMessage::MarkPrice(mark_price)),
            Err(e) => log::error!("Error parsing Lighter mark price: {e}"),
        }

        match parse_ws_index_price_update(stats, instrument, timestamp, ts_init) {
            Ok(index_price) => messages.push(NautilusWsMessage::IndexPrice(index_price)),
            Err(e) => log::error!("Error parsing Lighter index price: {e}"),
        }

        match parse_ws_funding_rate_update(stats, instrument, timestamp, ts_init) {
            Ok(funding_rate) => messages.push(NautilusWsMessage::FundingRate(funding_rate)),
            Err(e) => log::error!("Error parsing Lighter funding rate: {e}"),
        }

        messages
    }

    fn handle_spot_market_stats(
        &self,
        payload: &super::messages::LighterSpotMarketStatsPayload,
        timestamp: u64,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        match payload {
            super::messages::LighterSpotMarketStatsPayload::All(stats) => stats
                .values()
                .filter_map(|stats| self.handle_one_spot_market_stats(stats, timestamp, ts_init))
                .collect(),
            super::messages::LighterSpotMarketStatsPayload::One(stats) => self
                .handle_one_spot_market_stats(stats, timestamp, ts_init)
                .into_iter()
                .collect(),
        }
    }

    fn handle_one_spot_market_stats(
        &self,
        stats: &super::messages::LighterSpotMarketStats,
        timestamp: u64,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let Some(instrument) = self.instruments.get(&stats.market_id) else {
            log::debug!(
                "No instrument cached for Lighter spot_market_stats market_id={}",
                stats.market_id,
            );
            return None;
        };

        match parse_ws_spot_index_price_update(stats, instrument, timestamp, ts_init) {
            Ok(index_price) => Some(NautilusWsMessage::IndexPrice(index_price)),
            Err(e) => {
                log::error!("Error parsing Lighter spot index price: {e}");
                None
            }
        }
    }

    fn handle_candles(
        &mut self,
        channel: Ustr,
        candles: &[LighterWsCandle],
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let Some((market_index, resolution)) =
            candle_market_and_resolution_from_topic(channel.as_str())
        else {
            log::warn!("Lighter candle frame with unparsable channel `{channel}`");
            return Vec::new();
        };

        let Some(instrument) = self.instruments.get(&market_index) else {
            log::debug!("No instrument cached for Lighter candle market_index={market_index}");
            return Vec::new();
        };

        let key = (market_index, resolution);
        let mut emitted = Vec::new();

        for candle in candles {
            let previous = self.last_candles.get(&key).cloned();
            match previous {
                None => {}
                Some(prev) if candle.t > prev.t => {
                    // `t` advanced; previous candle is closed.
                    match parse_ws_bar(instrument, &prev, resolution, ts_init) {
                        Ok(bar) => emitted.push(NautilusWsMessage::Bar(bar)),
                        Err(e) => log::error!("Error parsing Lighter candle bar: {e}"),
                    }
                }
                Some(prev) if candle.t < prev.t => continue,
                Some(_) => {}
            }
            self.last_candles.insert(key, candle.clone());
        }

        emitted
    }

    fn handle_account_orders(
        &self,
        orders_by_market: &AHashMap<Ustr, Vec<LighterOrder>>,
        _ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        if self.exec_account.is_none() {
            log::debug!("Lighter account_orders frame skipped: no execution context set");
            return Vec::new();
        }

        let mut reports = Vec::new();

        for orders in orders_by_market.values() {
            for order in orders {
                if !self.instruments.contains_key(&order.market_index) {
                    log::debug!(
                        "No instrument cached for Lighter order market_index={}",
                        order.market_index,
                    );
                    continue;
                }

                reports.push(ExecutionReport::Order(order.clone()));
            }
        }

        if reports.is_empty() {
            Vec::new()
        } else {
            vec![NautilusWsMessage::ExecutionReports(reports)]
        }
    }

    fn handle_account_trades<'a>(
        &self,
        trades: impl IntoIterator<Item = &'a LighterTrade>,
        _ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let Some((_account_id, account_index)) = self.exec_account else {
            log::debug!("Lighter account_trades frame skipped: no execution context set");
            return Vec::new();
        };

        let mut reports = Vec::new();

        for trade in trades {
            if !self.instruments.contains_key(&trade.market_id) {
                log::debug!(
                    "No instrument cached for Lighter account trade market_id={}",
                    trade.market_id,
                );
                continue;
            }

            // The venue shares the channel with crossed pairs the account is
            // not part of; gate the cheap account check at the handler so
            // unrelated traffic never reaches the execution consumer.
            if trade.bid_account_id != account_index && trade.ask_account_id != account_index {
                continue;
            }

            reports.push(ExecutionReport::Fill(trade.clone()));
        }

        if reports.is_empty() {
            Vec::new()
        } else {
            vec![NautilusWsMessage::ExecutionReports(reports)]
        }
    }

    fn handle_account_positions(
        &self,
        positions: &AHashMap<Ustr, LighterPosition>,
        ts_init: UnixNanos,
        frame_type: PositionFrameType,
    ) -> Vec<NautilusWsMessage> {
        let Some((account_id, _)) = self.exec_account else {
            log::debug!("Lighter account_positions frame skipped: no execution context set");
            return Vec::new();
        };

        // The position frame carries no top-level event timestamp, so stamp
        // ts_event with the wall-clock time the handler observed the frame.
        let ts_event = ts_init;

        let mut reports = Vec::new();
        let mut skipped_market_ids = Vec::new();
        let mut closed_market_ids = Vec::new();

        for position in positions.values() {
            if position.position.is_zero() {
                if matches!(frame_type, PositionFrameType::Update) {
                    closed_market_ids.push(position.market_id);
                }
                continue;
            }

            let Some(instrument) = self.instruments.get(&position.market_id) else {
                log::debug!(
                    "No instrument cached for Lighter position market_id={}",
                    position.market_id,
                );

                skipped_market_ids.push(position.market_id);
                continue;
            };

            match parse_ws_position_status_report(
                position, instrument, account_id, ts_event, ts_init,
            ) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    skipped_market_ids.push(position.market_id);
                    log::error!("Error parsing Lighter position status report: {e}");
                }
            }
        }

        match frame_type {
            PositionFrameType::Snapshot => {
                // Emit even when empty: signals the last position closed.
                vec![NautilusWsMessage::PositionSnapshot {
                    reports,
                    skipped_market_ids,
                }]
            }
            PositionFrameType::Update => vec![NautilusWsMessage::PositionUpdate {
                reports,
                closed_market_ids,
                skipped_market_ids,
            }],
        }
    }

    fn handle_account_assets(
        &self,
        assets: &AHashMap<Ustr, LighterAsset>,
        timestamp_ms: u64,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let Some((account_id, _)) = self.exec_account else {
            log::debug!("Lighter account_assets frame skipped: no execution context set");
            return Vec::new();
        };

        let ts_event = match crate::common::parse::parse_millis_to_nanos(timestamp_ms) {
            Ok(ts) => ts,
            Err(e) => {
                log::error!("Invalid Lighter account_assets timestamp {timestamp_ms}: {e}");
                return Vec::new();
            }
        };

        self.account_state_reconciler.update_assets(assets);
        self.emit_unified_account_state(account_id, ts_event, ts_init)
    }

    fn handle_user_stats(
        &self,
        stats: &LighterUserStats,
        timestamp_ms: u64,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let Some((account_id, _)) = self.exec_account else {
            log::debug!("Lighter user_stats frame skipped: no execution context set");
            return Vec::new();
        };

        let ts_event = match crate::common::parse::parse_millis_to_nanos(timestamp_ms) {
            Ok(ts) => ts,
            Err(e) => {
                log::error!("Invalid Lighter user_stats timestamp {timestamp_ms}: {e}");
                return Vec::new();
            }
        };

        self.account_state_reconciler.update_user_stats(stats);
        self.emit_unified_account_state(account_id, ts_event, ts_init)
    }

    /// Asks the reconciler for a merged [`AccountState`] and wraps it as a
    /// `NautilusWsMessage`. Returns an empty vec when the second stream
    /// hasn't arrived yet; the AccountStreamFirstFrame marker still gets
    /// pushed by the caller so the startup gate progresses.
    fn emit_unified_account_state(
        &self,
        account_id: AccountId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        match self
            .account_state_reconciler
            .build_state(account_id, ts_event, ts_init)
        {
            Some(Ok(state)) => vec![NautilusWsMessage::AccountState(Box::new(state))],
            Some(Err(e)) => {
                log::error!("Error building unified Lighter account state: {e}");
                Vec::new()
            }
            None => Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
enum PositionFrameType {
    Snapshot,
    Update,
}

fn raw_message(frame: &LighterWsFrame) -> Vec<NautilusWsMessage> {
    let value = serde_json::to_value(frame).unwrap_or(serde_json::Value::Null);
    vec![NautilusWsMessage::Raw(value)]
}

fn apply_order_book_update(state: &mut LighterWsOrderBook, update: &LighterWsOrderBook) {
    apply_book_side_update(&mut state.bids, &update.bids, true);
    apply_book_side_update(&mut state.asks, &update.asks, false);

    state.code = update.code;
    state.offset = update.offset;
    state.nonce = update.nonce;
    state.last_updated_at = update.last_updated_at;
    state.begin_nonce = update.begin_nonce;
}

fn apply_book_side_update(
    levels: &mut Vec<LighterPriceLevel>,
    updates: &[LighterPriceLevel],
    bids: bool,
) {
    for update in updates {
        if update.price == Decimal::ZERO {
            continue;
        }

        match find_book_level(levels, update.price, bids) {
            Ok(index) if update.size == Decimal::ZERO => {
                levels.remove(index);
            }
            Ok(index) => {
                levels[index] = update.clone();
            }
            Err(_) if update.size == Decimal::ZERO => {}
            Err(index) => {
                levels.insert(index, update.clone());
            }
        }
    }
}

fn find_book_level(
    levels: &[LighterPriceLevel],
    price: Decimal,
    bids: bool,
) -> Result<usize, usize> {
    levels.binary_search_by(|level| {
        if bids {
            price.cmp(&level.price)
        } else {
            level.price.cmp(&price)
        }
    })
}

// Codes outside the transaction range (e.g. 30003 "Already Subscribed")
// would falsely reject a live order; see `LIGHTER_ERROR_CODE_TX_RANGE`.
fn is_sendtx_error_code(code: Option<u64>) -> bool {
    code.is_some_and(|c| LIGHTER_ERROR_CODE_TX_RANGE.contains(&c))
}

fn already_subscribed_error(value: &serde_json::Value) -> Option<&serde_json::Value> {
    if value.get("code").and_then(|code| code.as_u64())
        == Some(LIGHTER_ERROR_CODE_ALREADY_SUBSCRIBED)
    {
        Some(value)
    } else {
        value.get("error").filter(|e| {
            e.get("code").and_then(|code| code.as_u64())
                == Some(LIGHTER_ERROR_CODE_ALREADY_SUBSCRIBED)
        })
    }
}

fn subscription_error_code(value: &serde_json::Value) -> Option<u64> {
    value
        .get("code")
        .and_then(|code| code.as_u64())
        .or_else(|| {
            value
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|code| code.as_u64())
        })
}

fn typed_subscribe_topic(text: &str) -> Option<&str> {
    let header = serde_json::from_str::<LighterWsFrameHeader<'_>>(text).ok()?;
    header
        .kind
        .starts_with("subscribed/")
        .then_some(header.channel?)
}

// SendTxRejected from a top-level `{code, message}` frame: non-200 sendTx
// ACK or `{"type":"error",...}`.
fn send_tx_rejected_from_value(
    value: &serde_json::Value,
    source: SendTxRejectionSource,
) -> NautilusWsMessage {
    let code = value.get("code").and_then(|v| v.as_i64());
    let message = value
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tx_hash = value
        .get("tx_hash")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    NautilusWsMessage::SendTxRejected {
        connection_epoch: 0,
        source,
        code,
        message,
        tx_hash,
    }
}

// SendTxRejected from a nested `{"error":{"code":N,"message":...}}` frame;
// these frames never carry a `tx_hash`.
fn send_tx_rejected_from_nested_error(
    error: &serde_json::Value,
    source: SendTxRejectionSource,
) -> NautilusWsMessage {
    let code = error.get("code").and_then(|v| v.as_i64());
    let message = error
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    NautilusWsMessage::SendTxRejected {
        connection_epoch: 0,
        source,
        code,
        message,
        tx_hash: None,
    }
}

fn log_integrator_not_approved() {
    log::error!(
        "Lighter venue rejected with code {LIGHTER_ERROR_CODE_INTEGRATOR_NOT_APPROVED} \
         'integrator is not approved'.\n\
         Tagged orders require Nautilus integrator approval. \
         See: {LIGHTER_INTEGRATOR_APPROVAL_DOCS_URL}",
    );
}

fn market_index_from_topic(topic: &str) -> Option<i16> {
    let (_, rest) = topic.split_once(':')?;
    rest.parse::<i16>().ok()
}

fn candle_market_and_resolution_from_topic(topic: &str) -> Option<(i16, LighterCandleResolution)> {
    let (channel, rest) = topic.split_once(':')?;
    if LighterWsChannelKind::from_wire_str(channel) != Some(LighterWsChannelKind::Candle) {
        return None;
    }
    let (market, res) = rest.split_once(':')?;
    let market_index = market.parse::<i16>().ok()?;
    let resolution = res.parse::<LighterCandleResolution>().ok()?;
    Some((market_index, resolution))
}

fn order_book_market_index_from_topic(topic: &str) -> Option<i16> {
    let (channel, rest) = topic.split_once(':')?;
    if LighterWsChannelKind::from_wire_str(channel) != Some(LighterWsChannelKind::OrderBook) {
        return None;
    }
    rest.parse::<i16>().ok()
}

pub(crate) fn should_retry_lighter_ws_error(error: &LighterWsError) -> bool {
    match error {
        LighterWsError::Network(_) => true,
        // Closed and BrokenPipe are terminal on this client; only Timeout
        // (wait_for_active) can recover if the connection comes up. WriteTimeout
        // is not retryable: the write was cancelled after it began, so the peer
        // may already have the message and a retry could duplicate it.
        LighterWsError::Transport(send_error) => match send_error {
            SendError::Timeout => true,
            SendError::InvalidInput(_)
            | SendError::Closed
            | SendError::ConnectionChanged
            | SendError::BrokenPipe(_)
            | SendError::WriteTimeout => false,
        },
        LighterWsError::Authentication(_)
        | LighterWsError::Parse(_)
        | LighterWsError::Client(_)
        | LighterWsError::SendTxOutcomeUnknown(_) => false,
    }
}

pub(crate) fn create_lighter_ws_timeout_error(_msg: String) -> LighterWsError {
    // Structured variant so the classifier retries; the retry manager
    // already logs the textual timeout context.
    LighterWsError::Transport(SendError::Timeout)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use log::{Level, LevelFilter, Log, Metadata, Record};
    use nautilus_model::{
        enums::AccountType,
        identifiers::{InstrumentId, Symbol, Venue},
        instruments::{CryptoPerpetual, CurrencyPair},
        types::{Currency, Money, Price, Quantity},
    };
    use parking_lot::Mutex;
    use rstest::rstest;
    use rust_decimal::Decimal;
    use serde_json::json;

    use super::*;
    use crate::{
        common::enums::{LighterCandleResolution, LighterTxType},
        websocket::messages::{LighterMarketSelection, LighterWsCandle, LighterWsChannel},
    };

    const SECRET_MARKER: &str = "426426426";

    struct OutboundLogCapture {
        messages: Mutex<Vec<String>>,
    }

    static OUTBOUND_LOG_CAPTURE: OutboundLogCapture = OutboundLogCapture {
        messages: Mutex::new(Vec::new()),
    };

    impl OutboundLogCapture {
        fn clear(&self) {
            self.messages.lock().clear();
        }

        fn messages(&self) -> Vec<String> {
            self.messages.lock().clone()
        }
    }

    impl Log for OutboundLogCapture {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() == Level::Debug
                && metadata.target() == "nautilus_lighter::websocket::handler"
        }

        fn log(&self, record: &Record<'_>) {
            if self.enabled(record.metadata()) {
                let message = record.args().to_string();
                if message.starts_with("Sending Lighter unsubscribe") {
                    self.messages.lock().push(message);
                }
            }
        }

        fn flush(&self) {}
    }

    const WS_ACCOUNT_ORDERS_UPDATE: &str =
        include_str!("../../test_data/ws_account_orders_update.json");
    const WS_ACCOUNT_ALL_TRADES_UPDATE: &str =
        include_str!("../../test_data/ws_account_all_trades_update.json");
    const WS_ACCOUNT_ALL_POSITIONS_UPDATE: &str =
        include_str!("../../test_data/ws_account_all_positions_update.json");
    const WS_ACCOUNT_ALL_ASSETS_UPDATE: &str =
        include_str!("../../test_data/ws_account_all_assets_update.json");
    const WS_USER_STATS_UPDATE: &str = include_str!("../../test_data/ws_user_stats_update.json");
    const WS_ACCOUNT_ALL_ASSETS_WITH_POSITION: &str =
        include_str!("../../test_data/ws_account_all_assets_with_position.json");
    const WS_USER_STATS_WITH_POSITION: &str =
        include_str!("../../test_data/ws_user_stats_with_position.json");
    const WS_MARKET_STATS_UPDATE_SINGLE: &str =
        include_str!("../../test_data/ws_market_stats_update_single.json");
    const WS_MARKET_STATS_UPDATE_ALL: &str =
        include_str!("../../test_data/ws_market_stats_update_all.json");
    const WS_SPOT_MARKET_STATS_UPDATE_SINGLE: &str =
        include_str!("../../test_data/ws_spot_market_stats_update_single.json");
    const WS_SPOT_MARKET_STATS_UPDATE_ALL: &str =
        include_str!("../../test_data/ws_spot_market_stats_update_all.json");
    const WS_CANDLE_SUBSCRIBED: &str = include_str!("../../test_data/ws_candle_subscribed.json");

    fn handle_control_text(
        handler: &mut FeedHandler,
        text: &str,
    ) -> (bool, Option<NautilusWsMessage>) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return (false, None);
        };
        handler.handle_control_value(&value)
    }

    fn stub_eth_perp_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), Venue::new("LIGHTER"));
        InstrumentAny::CryptoPerpetual(
            CryptoPerpetual::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("ETH-PERP"))
                .base_currency(Currency::from("ETH"))
                .quote_currency(Currency::from("USDC"))
                .settlement_currency(Currency::from("USDC"))
                .is_inverse(false)
                .price_precision(2)
                .size_precision(4)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.0001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn stub_eth_spot_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("ETH-SPOT"), Venue::new("LIGHTER"));
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("ETH-SPOT"))
                .base_currency(Currency::from("ETH"))
                .quote_currency(Currency::from("USDC"))
                .price_precision(2)
                .size_precision(4)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.0001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn make_handler_with_account() -> FeedHandler {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));
        handler.instruments.insert(0, stub_eth_perp_instrument());
        handler.exec_account = Some((AccountId::from("LIGHTER-1234"), 1234));
        handler
    }

    #[rstest]
    #[tokio::test]
    async fn test_outbound_unsubscribe_log_omits_payload_body() {
        log::set_logger(&OUTBOUND_LOG_CAPTURE).expect("test logger already installed");
        log::set_max_level(LevelFilter::Debug);

        let handler = make_handler_with_account();
        let account_index = SECRET_MARKER.parse::<i64>().unwrap();
        let channel = LighterWsChannel::AccountAll(account_index);
        let payload_len = serde_json::to_string(&LighterWsRequest::unsubscribe(
            channel.subscription_channel(),
        ))
        .unwrap()
        .len();
        OUTBOUND_LOG_CAPTURE.clear();

        handler.dispatch_unsubscribe(channel).await;

        let messages = OUTBOUND_LOG_CAPTURE.messages();

        assert!(
            messages
                .iter()
                .all(|message| !message.contains(SECRET_MARKER)),
            "outbound logs exposed the secret marker: {messages:?}"
        );
        assert!(
            messages.iter().any(|message| {
                message == &format!("Sending Lighter unsubscribe ({payload_len} bytes)")
            }),
            "unsubscribe metadata missing or inaccurate: {messages:?}"
        );
    }

    fn mark_subscription_inflight(
        handler: &mut FeedHandler,
        channel: LighterWsChannel,
        response_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> (Ustr, u64) {
        handler.queue_subscribe(channel, None, response_tx);
        let (topic, generation) = handler
            .pending_subs
            .pop_front()
            .expect("subscription was not queued");
        handler.inflight_subs.insert(topic, generation);
        (topic, generation)
    }

    fn saturate_subscription_gate(handler: &mut FeedHandler) {
        for i in 0..SUBSCRIBE_INFLIGHT_MAX {
            handler
                .inflight_subs
                .insert(Ustr::from(format!("dummy:{i}").as_str()), i as u64 + 1);
        }
    }

    /// Strip the trailing `AccountStreamFirstFrame` marker from a handler
    /// emission so account-stream tests can assert on the typed payload
    /// alone. The marker contract is pinned in
    /// `handle_frame_emits_account_stream_first_frame_marker_per_variant`.
    fn strip_account_marker(mut msgs: Vec<NautilusWsMessage>) -> Vec<NautilusWsMessage> {
        if matches!(
            msgs.last(),
            Some(NautilusWsMessage::AccountStreamFirstFrame(_)),
        ) {
            msgs.pop();
        }
        msgs
    }

    #[rstest]
    fn handle_frame_routes_account_orders_to_execution_reports() {
        let mut handler = make_handler_with_account();
        let frame: super::LighterWsFrame = serde_json::from_str(WS_ACCOUNT_ORDERS_UPDATE).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::ExecutionReports(reports) => {
                assert_eq!(reports.len(), 1);
                match &reports[0] {
                    super::ExecutionReport::Order(order) => {
                        assert_eq!(order.order_id, "281476929510110");
                        assert_eq!(order.client_order_id, "42");
                    }
                    other => panic!("expected order report, was {other:?}"),
                }
            }
            other => panic!("expected execution reports, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_routes_account_trades_to_execution_reports() {
        let mut handler = make_handler_with_account();
        let frame: super::LighterWsFrame =
            serde_json::from_str(WS_ACCOUNT_ALL_TRADES_UPDATE).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::ExecutionReports(reports) => {
                assert_eq!(reports.len(), 1);
                match &reports[0] {
                    super::ExecutionReport::Fill(fill) => {
                        // Bid side is the user's account in this fixture
                        // (see handle_frame_routes_account_trades_to_execution_reports
                        // fixture: bid_account_id == 1234). The raw payload
                        // carries the bid order id as the venue id; the
                        // execution loop derives `venue_order_id` from it.
                        assert_eq!(fill.bid_id_str.as_deref(), Some("562947905631053"),);
                    }
                    other => panic!("expected fill report, was {other:?}"),
                }
            }
            other => panic!("expected execution reports, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_routes_account_positions_to_update_without_readiness_marker() {
        let mut handler = make_handler_with_account();
        let frame: super::LighterWsFrame =
            serde_json::from_str(WS_ACCOUNT_ALL_POSITIONS_UPDATE).unwrap();

        let messages = handler.handle_frame(frame, UnixNanos::from(11));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::PositionUpdate {
                reports,
                closed_market_ids,
                skipped_market_ids,
            } => {
                assert!(closed_market_ids.is_empty());
                assert!(skipped_market_ids.is_empty());
                assert_eq!(reports.len(), 1);
                assert_eq!(reports[0].quantity, Quantity::from("1.5000"));
            }
            other => panic!("expected position update, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_tolerates_unknown_position_margin_mode() {
        let mut handler = make_handler_with_account();
        let mut frame_json: serde_json::Value =
            serde_json::from_str(WS_ACCOUNT_ALL_POSITIONS_UPDATE).unwrap();
        frame_json["positions"]["0"]["margin_mode"] = json!(99);
        let frame: super::LighterWsFrame = serde_json::from_value(frame_json).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::PositionUpdate {
                reports,
                closed_market_ids,
                skipped_market_ids,
            } => {
                assert!(closed_market_ids.is_empty());
                assert!(skipped_market_ids.is_empty());
                assert_eq!(reports.len(), 1);
            }
            other => panic!("expected position update, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_routes_empty_account_positions_to_empty_update() {
        let mut handler = make_handler_with_account();
        let frame_json = serde_json::json!({
            "type": "update/account_all_positions",
            "channel": "account_all_positions:1234",
            "positions": {},
            "shares": [],
            "last_funding_round": null,
            "last_funding_discount": null,
        });
        let frame: super::LighterWsFrame = serde_json::from_value(frame_json).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::PositionUpdate {
                reports,
                closed_market_ids,
                skipped_market_ids,
            } => {
                assert!(closed_market_ids.is_empty());
                assert!(skipped_market_ids.is_empty());
                assert!(reports.is_empty());
            }
            other => panic!("expected empty position update, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_routes_zero_account_position_to_closed_update() {
        let mut handler = make_handler_with_account();
        let mut frame_json: serde_json::Value =
            serde_json::from_str(WS_ACCOUNT_ALL_POSITIONS_UPDATE).unwrap();
        frame_json["positions"]["0"]["position"] = json!("0.0000");
        let frame: super::LighterWsFrame = serde_json::from_value(frame_json).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::PositionUpdate {
                reports,
                closed_market_ids,
                skipped_market_ids,
            } => {
                assert!(reports.is_empty());
                assert!(skipped_market_ids.is_empty());
                assert_eq!(closed_market_ids, &[0]);
            }
            other => panic!("expected closed position update, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_marks_account_positions_incomplete_when_position_instrument_uncached() {
        let mut handler = make_handler_with_account();
        let mut frame_json: serde_json::Value =
            serde_json::from_str(WS_ACCOUNT_ALL_POSITIONS_UPDATE).unwrap();
        frame_json["type"] = json!("subscribed/account_all_positions");
        frame_json["positions"]["0"]["market_id"] = json!(999);
        let frame: super::LighterWsFrame = serde_json::from_value(frame_json).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::PositionSnapshot {
                reports,
                skipped_market_ids,
            } => {
                assert_eq!(skipped_market_ids, &[999]);
                assert!(reports.is_empty());
            }
            other => panic!("expected incomplete position snapshot, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_marks_account_positions_incomplete_when_position_parse_fails() {
        let mut handler = make_handler_with_account();
        let mut frame_json: serde_json::Value =
            serde_json::from_str(WS_ACCOUNT_ALL_POSITIONS_UPDATE).unwrap();
        frame_json["type"] = json!("subscribed/account_all_positions");
        frame_json["positions"]["0"]["position"] = json!("-1.5000");
        let frame: super::LighterWsFrame = serde_json::from_value(frame_json).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::PositionSnapshot {
                reports,
                skipped_market_ids,
            } => {
                assert_eq!(skipped_market_ids, &[0]);
                assert!(reports.is_empty());
            }
            other => panic!("expected incomplete position snapshot, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_marks_position_update_incomplete_when_position_parse_fails() {
        let mut handler = make_handler_with_account();
        let mut frame_json: serde_json::Value =
            serde_json::from_str(WS_ACCOUNT_ALL_POSITIONS_UPDATE).unwrap();
        frame_json["positions"]["0"]["position"] = json!("-1.5000");
        let frame: super::LighterWsFrame = serde_json::from_value(frame_json).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::PositionUpdate {
                reports,
                closed_market_ids,
                skipped_market_ids,
            } => {
                assert!(reports.is_empty());
                assert!(closed_market_ids.is_empty());
                assert_eq!(skipped_market_ids, &[0]);
            }
            other => panic!("expected incomplete position update, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_routes_subscribed_account_all_positions_snapshot() {
        let mut handler = make_handler_with_account();
        let frame_json = serde_json::json!({
            "type": "subscribed/account_all_positions",
            "channel": "account_all_positions:1234",
            "positions": {
                "0": {
                    "allocated_margin": "0.000000",
                    "avg_entry_price": "0.111230",
                    "initial_margin_fraction": "10.00",
                    "liquidation_price": "0.100598",
                    "margin_mode": 0,
                    "market_id": 0,
                    "open_order_count": 0,
                    "pending_order_count": 0,
                    "position": "100",
                    "position_tied_order_count": 0,
                    "position_value": "11.123000",
                    "realized_pnl": "0.000000",
                    "sign": 1,
                    "symbol": "ETH",
                    "total_discount": "0.000000",
                    "total_funding_paid_out": "0.000000",
                    "unrealized_pnl": "0.000000"
                }
            },
        });
        let frame: super::LighterWsFrame = serde_json::from_value(frame_json).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::PositionSnapshot {
                reports,
                skipped_market_ids,
            } => {
                assert!(skipped_market_ids.is_empty());
                assert_eq!(reports.len(), 1);
                assert_eq!(reports[0].quantity, Quantity::from("100"));
            }
            other => panic!("expected position snapshot, was {other:?}"),
        }
    }

    #[rstest]
    #[case::connected(serde_json::json!({"type": "connected", "session_id": "x"}), true, false)]
    #[case::ping(serde_json::json!({"type": "ping"}), true, false)]
    #[case::pong(serde_json::json!({"type": "pong"}), true, false)]
    #[case::send_tx_ack(
        serde_json::json!({"type": "jsonapi/sendtx", "code": 200, "tx_hash": "abc"}),
        true,
        true,
    )]
    #[case::send_tx_without_code(
        serde_json::json!({"type": "jsonapi/sendtx", "tx_hash": "abc"}),
        true,
        false,
    )]
    #[case::send_tx_with_nonnumeric_code(
        serde_json::json!({"type": "jsonapi/sendtx", "code": "200", "tx_hash": "abc"}),
        true,
        false,
    )]
    #[case::error_frame(
        serde_json::json!({"type": "error", "code": 21727, "message": "invalid client order index"}),
        true,
        true,
    )]
    #[case::error_frame_integrator_not_approved(
        serde_json::json!({"type": "error", "code": 21149, "message": "integrator is not approved"}),
        true,
        true,
    )]
    #[case::send_tx_ack_integrator_not_approved(
        serde_json::json!({"type": "jsonapi/sendtx", "code": 21149, "message": "integrator is not approved"}),
        true,
        true,
    )]
    #[case::wrapped_error_integrator_not_approved(
        serde_json::json!({"error": {"code": 21149, "message": "integrator is not approved"}}),
        true,
        true,
    )]
    #[case::subscription_error_frame(
        serde_json::json!({"type": "error", "code": 30003, "message": "Already Subscribed to : ticker:3"}),
        true,
        false,
    )]
    #[case::wrapped_subscription_error(
        serde_json::json!({"error": {"code": 30003, "message": "Already Subscribed to : ticker:3"}}),
        true,
        false,
    )]
    #[case::codeless_error_frame(
        serde_json::json!({"type": "error", "message": "unclassifiable"}),
        true,
        false,
    )]
    #[case::unknown_type(
        serde_json::json!({"type": "something_unexpected", "payload": "x"}),
        false,
        false,
    )]
    #[case::no_type_field(
        serde_json::json!({"error": {"code": 21702, "message": "invalid price"}}),
        true,
        true,
    )]
    fn handle_control_text_tri_state(
        #[case] payload: serde_json::Value,
        #[case] expected_matched: bool,
        #[case] expected_has_msg: bool,
    ) {
        // Pins the (matched, msg) contract that lets the WS receive loop
        // distinguish "known control type, consume silently" from
        // "surface a typed/Raw message to the consumer". sendTx success ACKs
        // and venue-error frames now produce typed SendTxAck/SendTxRejected
        // variants instead of Raw, so the execution loop can correlate them
        // back to in-flight cloids.
        let mut handler = make_handler_with_account();
        let text = payload.to_string();
        let (matched, msg) = handle_control_text(&mut handler, &text);
        assert_eq!(matched, expected_matched, "matched flag");
        assert_eq!(msg.is_some(), expected_has_msg, "msg presence");
    }

    #[rstest]
    fn handle_control_text_sendtx_success_emits_typed_ack() {
        let mut handler = make_handler_with_account();
        let payload = serde_json::json!({
            "type": "jsonapi/sendtx",
            "code": 200,
            "tx_hash": "0000abcd",
        })
        .to_string();

        let (_, msg) = handle_control_text(&mut handler, &payload);

        match msg.expect("SendTxAck emitted") {
            NautilusWsMessage::SendTxAck { tx_hash, code, .. } => {
                assert_eq!(code, 200);
                assert_eq!(tx_hash.as_deref(), Some("0000abcd"));
            }
            other => panic!("expected SendTxAck, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_control_text_sendtx_failure_emits_ack_sourced_rejection() {
        let mut handler = make_handler_with_account();
        let payload = serde_json::json!({
            "type": "jsonapi/sendtx",
            "code": 21727,
            "message": "invalid client order index",
        })
        .to_string();

        let (_, msg) = handle_control_text(&mut handler, &payload);

        match msg.expect("SendTxRejected emitted") {
            NautilusWsMessage::SendTxRejected {
                source,
                code,
                message,
                tx_hash,
                ..
            } => {
                assert_eq!(source, SendTxRejectionSource::Ack);
                assert_eq!(code, Some(21727));
                assert_eq!(message, "invalid client order index");
                assert_eq!(tx_hash, None);
            }
            other => panic!("expected SendTxRejected, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_control_text_sendtx_failure_carries_echoed_tx_hash() {
        let mut handler = make_handler_with_account();
        let payload = serde_json::json!({
            "type": "jsonapi/sendtx",
            "code": 21727,
            "message": "invalid client order index",
            "tx_hash": "0000abcd",
        })
        .to_string();

        let (_, msg) = handle_control_text(&mut handler, &payload);

        match msg.expect("SendTxRejected emitted") {
            NautilusWsMessage::SendTxRejected { tx_hash, .. } => {
                assert_eq!(tx_hash.as_deref(), Some("0000abcd"));
            }
            other => panic!("expected SendTxRejected, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_control_text_bare_error_frame_emits_bare_error_rejection() {
        let mut handler = make_handler_with_account();
        let payload = serde_json::json!({
            "type": "error",
            "code": 21702,
            "message": "invalid price",
        })
        .to_string();

        let (_, msg) = handle_control_text(&mut handler, &payload);

        match msg.expect("SendTxRejected emitted") {
            NautilusWsMessage::SendTxRejected {
                source,
                code,
                message,
                tx_hash,
                ..
            } => {
                assert_eq!(source, SendTxRejectionSource::BareError);
                assert_eq!(code, Some(21702));
                assert_eq!(message, "invalid price");
                assert_eq!(tx_hash, None);
            }
            other => panic!("expected SendTxRejected, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_control_text_wrapped_error_emits_bare_error_rejection() {
        // No top-level `type` field; the rejection sits under `error.{code,message}`.
        let mut handler = make_handler_with_account();
        let payload = serde_json::json!({
            "error": {"code": 21149, "message": "integrator is not approved"},
        })
        .to_string();

        let (_, msg) = handle_control_text(&mut handler, &payload);

        match msg.expect("SendTxRejected emitted") {
            NautilusWsMessage::SendTxRejected {
                source,
                code,
                message,
                tx_hash,
                ..
            } => {
                assert_eq!(source, SendTxRejectionSource::BareError);
                assert_eq!(code, Some(21149));
                assert_eq!(message, "integrator is not approved");
                assert_eq!(tx_hash, None);
            }
            other => panic!("expected SendTxRejected, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_emits_no_account_state_until_both_streams_seen() {
        // Reconciler refuses to emit until both `account_all_assets` and
        // `user_stats` have delivered. An assets frame alone produces only
        // the AccountStreamFirstFrame marker, no AccountState payload.
        let mut handler = make_handler_with_account();
        let assets_only: super::LighterWsFrame =
            serde_json::from_str(WS_ACCOUNT_ALL_ASSETS_UPDATE).unwrap();

        let messages = strip_account_marker(handler.handle_frame(assets_only, UnixNanos::from(11)));

        assert!(
            messages.is_empty(),
            "expected no AccountState before user_stats arrives, received {messages:?}"
        );
    }

    #[rstest]
    fn handle_frame_routes_account_assets_and_user_stats_to_unified_state() {
        // Fixture is the captured production no-position payload (10 USDC
        // on spot, 40 USDC pledged as perp collateral, no resting orders).
        // Lighter runs unified margin: both legs are deployable equity, so
        // total = balance + margin_balance, locked = locked_balance only,
        // and MarginBalance.initial = 0 because user_stats.collateral ==
        // available_balance (no margin in use).
        let mut handler = make_handler_with_account();
        let assets_frame: super::LighterWsFrame =
            serde_json::from_str(WS_ACCOUNT_ALL_ASSETS_UPDATE).unwrap();
        let user_stats_frame: super::LighterWsFrame =
            serde_json::from_str(WS_USER_STATS_UPDATE).unwrap();

        let _ = handler.handle_frame(assets_frame, UnixNanos::from(11));
        let messages =
            strip_account_marker(handler.handle_frame(user_stats_frame, UnixNanos::from(12)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::AccountState(state) => {
                let usdc = Currency::get_or_create_crypto("USDC");
                assert_eq!(state.account_type, AccountType::Margin);
                assert_eq!(state.base_currency, None);
                assert_eq!(state.balances.len(), 1);
                assert_eq!(state.balances[0].currency, usdc);
                // balance 10 + margin_balance 40 = 50 total; locked = 0
                // (no resting spot orders); free = 50 (all deployable).
                assert_eq!(state.balances[0].total, Money::from("50.000000 USDC"));
                assert_eq!(state.balances[0].locked, Money::from("0 USDC"));
                assert_eq!(state.balances[0].free, Money::from("50.000000 USDC"));
                assert_eq!(state.margins.len(), 1);
                assert_eq!(state.margins[0].currency, usdc);
                assert_eq!(state.margins[0].initial, Money::from("0 USDC"));
                assert_eq!(state.margins[0].maintenance, Money::from("0 USDC"));
                assert!(state.margins[0].instrument_id.is_none());
                assert!(state.is_reported);
            }
            other => panic!("expected account state, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_unified_state_reflects_open_position() {
        // Fixture pair captured live with one open ETH-LONG position:
        //   account_all_assets[USDC] = {balance:10, locked:0, margin_balance:39.995369556}
        //   user_stats              = {collateral:39.995369, available:39.168314, margin_usage:2.07}
        //
        // The 5 mUSDC haircut on margin_balance is the entry fee. The
        // 0.827055 USDC gap between collateral and available_balance is
        // the initial margin pledged to the open position. AccountBalance
        // is unchanged in shape; perp-margin-in-use lives on
        // MarginBalance, not in `locked`. Maintenance is zero because
        // Lighter doesn't publish a maintenance value on user_stats.
        let mut handler = make_handler_with_account();
        let assets_frame: super::LighterWsFrame =
            serde_json::from_str(WS_ACCOUNT_ALL_ASSETS_WITH_POSITION).unwrap();
        let user_stats_frame: super::LighterWsFrame =
            serde_json::from_str(WS_USER_STATS_WITH_POSITION).unwrap();

        let _ = handler.handle_frame(assets_frame, UnixNanos::from(11));
        let messages =
            strip_account_marker(handler.handle_frame(user_stats_frame, UnixNanos::from(12)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::AccountState(state) => {
                assert_eq!(state.account_type, AccountType::Margin);
                assert_eq!(state.base_currency, None);
                assert_eq!(state.balances.len(), 1);
                // total = 10 + 39.995369556 = 49.99536956 (Money truncates
                // the trailing digit to 8 decimals, matching the
                // production log).
                assert_eq!(state.balances[0].total, Money::from("49.99536956 USDC"));
                assert_eq!(state.balances[0].locked, Money::from("0 USDC"));
                assert_eq!(state.balances[0].free, Money::from("49.99536956 USDC"));
                assert_eq!(state.margins.len(), 1);
                // initial = collateral 39.995369 - available 39.168314 = 0.827055
                assert_eq!(state.margins[0].initial, Money::from("0.82705500 USDC"));
                assert_eq!(state.margins[0].maintenance, Money::from("0 USDC"));
                assert!(state.margins[0].instrument_id.is_none());
            }
            other => panic!("expected account state, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_emits_account_stream_first_frame_marker_per_variant() {
        // Pins the strict-await wiring: the handler appends an
        // `AccountStreamFirstFrame` marker after any typed reports so the
        // execution consumption loop can mark readiness only after the
        // accompanying state has been applied. The marker fires even for
        // empty content frames that produce no typed reports.
        let mut handler = make_handler_with_account();
        let orders_frame: super::LighterWsFrame =
            serde_json::from_str(WS_ACCOUNT_ORDERS_UPDATE).unwrap();
        let trades_frame: super::LighterWsFrame =
            serde_json::from_str(WS_ACCOUNT_ALL_TRADES_UPDATE).unwrap();
        let positions_frame: super::LighterWsFrame = serde_json::from_value({
            let mut value: serde_json::Value =
                serde_json::from_str(WS_ACCOUNT_ALL_POSITIONS_UPDATE).unwrap();
            value["type"] = json!("subscribed/account_all_positions");
            value
        })
        .unwrap();
        let assets_frame: super::LighterWsFrame =
            serde_json::from_str(WS_ACCOUNT_ALL_ASSETS_UPDATE).unwrap();
        let user_stats_frame: super::LighterWsFrame =
            serde_json::from_str(WS_USER_STATS_UPDATE).unwrap();

        let cases = [
            (orders_frame, AccountStream::Orders),
            (trades_frame, AccountStream::Trades),
            (positions_frame, AccountStream::Positions),
            (assets_frame, AccountStream::Assets),
            (user_stats_frame, AccountStream::UserStats),
        ];

        for (frame, expected) in cases {
            let msgs = handler.handle_frame(frame, UnixNanos::from(11));
            let marker = msgs
                .iter()
                .find(|m| matches!(m, NautilusWsMessage::AccountStreamFirstFrame(_)))
                .unwrap_or_else(|| panic!("missing marker for {expected:?}"));
            match marker {
                NautilusWsMessage::AccountStreamFirstFrame(stream) => {
                    assert_eq!(*stream, expected);
                }
                other => panic!("expected AccountStreamFirstFrame, was {other:?}"),
            }
            // The marker must trail any typed reports the handler emitted
            // so the consumption loop applies state first.
            assert!(
                matches!(
                    msgs.last(),
                    Some(NautilusWsMessage::AccountStreamFirstFrame(_)),
                ),
                "marker must trail typed reports for {expected:?}",
            );
        }
    }

    #[rstest]
    fn handle_frame_account_orders_without_context_falls_back_to_raw() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));
        handler.instruments.insert(0, stub_eth_perp_instrument());
        // exec_account intentionally left unset; the handler must preserve
        // the prior `Raw` forwarding contract for unauthenticated subscribers.

        let frame: super::LighterWsFrame = serde_json::from_str(WS_ACCOUNT_ORDERS_UPDATE).unwrap();
        let messages = handler.handle_frame(frame, UnixNanos::from(11));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::Raw(value) => {
                assert_eq!(value["type"], "update/account_orders");
            }
            other => panic!("expected raw fallback, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_account_orders_skips_unknown_market() {
        // Build a handler with the execution context but no instrument
        // cached for the order's market_index; the handler should log and
        // emit no execution reports (the trailing readiness marker still
        // fires so `connect()` does not stall when the venue resubscribes
        // before instrument bootstrap completes).
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));
        handler.exec_account = Some((AccountId::from("LIGHTER-1234"), 1234));
        // No instrument inserted for market_index=0.

        let frame: super::LighterWsFrame = serde_json::from_str(WS_ACCOUNT_ORDERS_UPDATE).unwrap();
        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert!(messages.is_empty());
    }

    #[rstest]
    fn handle_frame_account_assets_invalid_timestamp_returns_empty() {
        let mut handler = make_handler_with_account();
        // u64::MAX millis overflows the seconds-to-nanos conversion in
        // `parse_millis_to_nanos`; the handler must log and emit no typed
        // account state rather than surface a partially constructed one.
        // The trailing readiness marker is still emitted so `connect()`
        // does not stall if the venue ever sends a malformed timestamp on
        // the initial frame.
        let frame_json = r#"{
            "type": "update/account_all_assets",
            "channel": "account_all_assets:1234",
            "timestamp": 18446744073709551615,
            "assets": {
                "0": {
                    "symbol": "USDC",
                    "asset_id": 0,
                    "balance": "100.000000",
                    "locked_balance": "1.000000"
                }
            }
        }"#;
        let frame: super::LighterWsFrame = serde_json::from_str(frame_json).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert!(messages.is_empty());
    }

    #[rstest]
    fn handle_frame_account_all_orders_routes_to_execution_reports() {
        // The `update/account_all_orders` variant lacks the per-account
        // top-level `account` and `nonce` fields that `account_orders`
        // carries, but the handler should treat them identically.
        let mut handler = make_handler_with_account();
        let frame_json = r#"{
            "type": "update/account_all_orders",
            "channel": "account_all_orders:1234",
            "orders": {
                "0": [{
                    "order_index": 281476929510110,
                    "client_order_index": 42,
                    "order_id": "281476929510110",
                    "client_order_id": "42",
                    "market_index": 0,
                    "owner_account_index": 1234,
                    "initial_base_amount": "0.0050",
                    "price": "2352.74",
                    "nonce": 9182390020,
                    "remaining_base_amount": "0.0050",
                    "is_ask": true,
                    "base_size": 50,
                    "base_price": 235274,
                    "filled_base_amount": "0.0000",
                    "filled_quote_amount": "0.000000",
                    "side": "sell",
                    "type": "limit",
                    "time_in_force": "good-till-time",
                    "reduce_only": false,
                    "trigger_price": "0.00",
                    "order_expiry": 1780360584479,
                    "status": "open",
                    "trigger_status": "na",
                    "trigger_time": 0,
                    "parent_order_index": 0,
                    "parent_order_id": "0",
                    "to_trigger_order_id_0": "0",
                    "to_trigger_order_id_1": "0",
                    "to_cancel_order_id_0": "0",
                    "integrator_fee_collector_index": "0",
                    "integrator_taker_fee": "0",
                    "integrator_maker_fee": "0",
                    "block_height": 227535532,
                    "timestamp": 1777941383576,
                    "created_at": 1777941383576,
                    "updated_at": 1777941383576,
                    "transaction_time": 1777941383576735
                }]
            }
        }"#;
        let frame: super::LighterWsFrame = serde_json::from_str(frame_json).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::ExecutionReports(reports) => {
                assert_eq!(reports.len(), 1);
                match &reports[0] {
                    super::ExecutionReport::Order(order) => {
                        assert_eq!(order.order_id, "281476929510110");
                    }
                    other => panic!("expected order report, was {other:?}"),
                }
            }
            other => panic!("expected execution reports, was {other:?}"),
        }
    }

    fn snapshot_trade_frame_json() -> &'static str {
        r#"{
            "type": "subscribed/account_all_trades",
            "channel": "account_all_trades:1234",
            "trades": [{
                "trade_id": 19209006902,
                "trade_id_str": "19209006902",
                "tx_hash": "000000128b1ee814",
                "type": "trade",
                "market_id": 0,
                "size": "0.1336",
                "price": "2352.73",
                "usd_amount": "314.324728",
                "ask_id": 281476929510102,
                "bid_id": 562947905631053,
                "ask_client_id": 0,
                "bid_client_id": 7001011966,
                "ask_account_id": 91249,
                "bid_account_id": 1234,
                "is_maker_ask": true,
                "block_height": 227535535,
                "timestamp": 1777941384181,
                "transaction_time": 1777941384181586
            }],
            "total_volume": "100.0",
            "monthly_volume": "100.0",
            "weekly_volume": "100.0",
            "daily_volume": "100.0"
        }"#
    }

    #[rstest]
    fn handle_frame_account_all_trades_snapshot_is_dropped_with_context() {
        let mut handler = make_handler_with_account();
        // With execution context active the snapshot must be dropped to
        // avoid replaying historical fills as live FillReports on reconnect.
        // The trailing readiness marker is still emitted so `connect()`
        // observes the trades stream as having delivered.
        let frame: super::LighterWsFrame =
            serde_json::from_str(snapshot_trade_frame_json()).unwrap();

        let messages = strip_account_marker(handler.handle_frame(frame, UnixNanos::from(11)));

        assert!(messages.is_empty());
    }

    #[rstest]
    fn handle_frame_account_all_trades_snapshot_falls_back_to_raw_without_context() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));
        handler.instruments.insert(0, stub_eth_perp_instrument());
        // No exec_account: preserve prior Raw forwarding so unauthenticated
        // subscribers still see the snapshot frame.

        let frame: super::LighterWsFrame =
            serde_json::from_str(snapshot_trade_frame_json()).unwrap();
        let messages = handler.handle_frame(frame, UnixNanos::from(11));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::Raw(value) => {
                assert_eq!(value["type"], "subscribed/account_all_trades");
            }
            other => panic!("expected raw fallback, was {other:?}"),
        }
    }

    #[rstest]
    fn handle_frame_market_stats_emits_mark_index_and_funding_updates() {
        let mut handler = make_handler_with_account();
        let frame: super::LighterWsFrame =
            serde_json::from_str(WS_MARKET_STATS_UPDATE_SINGLE).unwrap();

        let messages = handler.handle_frame(frame, UnixNanos::from(11));

        assert_eq!(messages.len(), 3);
        match &messages[0] {
            NautilusWsMessage::MarkPrice(update) => {
                assert_eq!(update.instrument_id.to_string(), "ETH-PERP.LIGHTER");
                assert_eq!(update.value, Price::from("2064.47"));
                assert_eq!(update.ts_event, UnixNanos::from(1_774_883_844_933_000_000));
            }
            event => panic!("expected mark price update, was {event:?}"),
        }

        match &messages[1] {
            NautilusWsMessage::IndexPrice(update) => {
                assert_eq!(update.instrument_id.to_string(), "ETH-PERP.LIGHTER");
                assert_eq!(update.value, Price::from("2064.48"));
            }
            event => panic!("expected index price update, was {event:?}"),
        }

        match &messages[2] {
            NautilusWsMessage::FundingRate(update) => {
                assert_eq!(update.instrument_id.to_string(), "ETH-PERP.LIGHTER");
                assert_eq!(update.rate, Decimal::new(1, 6));
                assert_eq!(update.next_funding_ns, None);
            }
            event => panic!("expected funding rate update, was {event:?}"),
        }
    }

    #[rstest]
    fn handle_frame_market_stats_all_emits_mark_index_and_funding_updates() {
        let mut handler = make_handler_with_account();
        let frame: super::LighterWsFrame =
            serde_json::from_str(WS_MARKET_STATS_UPDATE_ALL).unwrap();

        let messages = handler.handle_frame(frame, UnixNanos::from(11));

        assert_eq!(messages.len(), 3);
        assert!(matches!(&messages[0], NautilusWsMessage::MarkPrice(_)));
        assert!(matches!(&messages[1], NautilusWsMessage::IndexPrice(_)));
        assert!(matches!(&messages[2], NautilusWsMessage::FundingRate(_)));

        match &messages[0] {
            NautilusWsMessage::MarkPrice(update) => {
                assert_eq!(update.instrument_id.to_string(), "ETH-PERP.LIGHTER");
                assert_eq!(update.value, Price::from("2064.47"));
            }
            event => panic!("expected mark price update, was {event:?}"),
        }

        match &messages[2] {
            NautilusWsMessage::FundingRate(update) => {
                assert_eq!(update.rate, Decimal::new(1, 6));
                assert_eq!(update.next_funding_ns, None);
            }
            event => panic!("expected funding rate update, was {event:?}"),
        }
    }

    #[rstest]
    fn handle_frame_spot_market_stats_emits_index_update() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));
        handler.instruments.insert(2048, stub_eth_spot_instrument());
        let frame: super::LighterWsFrame =
            serde_json::from_str(WS_SPOT_MARKET_STATS_UPDATE_SINGLE).unwrap();

        let messages = handler.handle_frame(frame, UnixNanos::from(11));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::IndexPrice(update) => {
                assert_eq!(update.instrument_id.to_string(), "ETH-SPOT.LIGHTER");
                assert_eq!(update.value, Price::from("1.00"));
            }
            event => panic!("expected spot index price update, was {event:?}"),
        }
    }

    #[rstest]
    fn handle_frame_spot_market_stats_all_emits_index_update() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));
        handler.instruments.insert(2048, stub_eth_spot_instrument());
        let frame: super::LighterWsFrame =
            serde_json::from_str(WS_SPOT_MARKET_STATS_UPDATE_ALL).unwrap();

        let messages = handler.handle_frame(frame, UnixNanos::from(11));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::IndexPrice(update) => {
                assert_eq!(update.instrument_id.to_string(), "ETH-SPOT.LIGHTER");
                assert_eq!(update.value, Price::from("1.00"));
            }
            event => panic!("expected spot index price update, was {event:?}"),
        }
    }

    #[rstest]
    #[case(LighterWsChannel::OrderBook(0), "order_book:0", "order_book/0")]
    #[case(LighterWsChannel::Trade(7), "trade:7", "trade/7")]
    #[case(LighterWsChannel::Ticker(2), "ticker:2", "ticker/2")]
    #[case(LighterWsChannel::Height, "height", "height")]
    #[case(
        LighterWsChannel::MarketStats(LighterMarketSelection::All),
        "market_stats:all",
        "market_stats/all"
    )]
    #[case(
        LighterWsChannel::SpotMarketStats(LighterMarketSelection::Market(2048)),
        "spot_market_stats:2048",
        "spot_market_stats/2048"
    )]
    #[case(
        LighterWsChannel::AccountOrders { market_index: 0, account_index: 1234 },
        "account_orders:0:1234",
        "account_orders/0/1234",
    )]
    fn topic_and_subscription_round_trip(
        #[case] channel: LighterWsChannel,
        #[case] expected_topic: &str,
        #[case] expected_subscription: &str,
    ) {
        assert_eq!(channel.topic_key(), expected_topic);
        assert_eq!(channel.subscription_channel(), expected_subscription);
    }

    #[rstest]
    #[case("order_book:0", Some(0))]
    #[case("trade:42", Some(42))]
    #[case("height", None)]
    #[case("malformed", None)]
    fn market_index_extraction(#[case] topic: &str, #[case] expected: Option<i16>) {
        assert_eq!(market_index_from_topic(topic), expected);
    }

    #[rstest]
    #[case("order_book:0", Some(0))]
    #[case("order_book:42", Some(42))]
    #[case("trade:42", None)]
    #[case("ticker:2", None)]
    #[case("market_stats:0", None)]
    #[case("height", None)]
    #[case("order_book:not-an-int", None)]
    fn order_book_market_index_only_matches_order_book_channel(
        #[case] topic: &str,
        #[case] expected: Option<i16>,
    ) {
        assert_eq!(order_book_market_index_from_topic(topic), expected);
    }

    #[rstest]
    #[case(LighterWsChannel::AccountAll(1234), true)]
    #[case(LighterWsChannel::OrderBook(0), false)]
    #[case(LighterWsChannel::AccountAllPositions(1), true)]
    #[case(LighterWsChannel::Trade(0), false)]
    fn requires_auth_classification(#[case] channel: LighterWsChannel, #[case] expected: bool) {
        assert_eq!(channel.requires_auth(), expected);
    }

    #[rstest]
    fn handler_command_subscribe_debug_redacts_auth_token() {
        let token = "schnorr-signature-bytes-do-not-leak";
        let cmd = HandlerCommand::Subscribe {
            channel: LighterWsChannel::AccountAll(1234),
            auth: Some(SecretString::from(token)),
            response_tx: None,
        };

        let dbg = format!("{cmd:?}");

        assert!(
            !dbg.contains(token),
            "Debug output must not contain the auth token, found: {dbg}",
        );
        assert!(dbg.contains(REDACTED));
    }

    #[tokio::test]
    async fn send_tx_command_returns_handler_send_error_without_active_client() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        cmd_tx
            .send(HandlerCommand::SendTx {
                tx_type: LighterTxType::CreateOrder as u8,
                tx_info: serde_json::value::RawValue::from_string(
                    r#"{"AccountIndex":12345,"Nonce":42}"#.to_string(),
                )
                .unwrap(),
                connection_epoch: 0,
                response_tx,
            })
            .unwrap();
        drop(cmd_tx);
        drop(raw_tx);

        let next = tokio::time::timeout(Duration::from_secs(2), handler.next())
            .await
            .expect("timed out waiting for handler to drain command");
        let result = response_rx.await.expect("sendTx response channel closed");

        assert!(next.is_none());
        let Err(LighterWsError::Client(message)) = result else {
            panic!("expected client send error, was {result:?}");
        };
        assert!(message.contains("no active WebSocket client"));
    }

    #[tokio::test]
    async fn resubscribe_order_book_command_skips_when_reference_removed() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let subscriptions = SubscriptionState::new(':');
        let topic = LighterWsChannel::OrderBook(0).topic_key();
        assert!(subscriptions.add_reference(&topic));
        assert!(subscriptions.remove_reference(&topic));

        let mut handler = FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, subscriptions.clone());
        handler.book_delta_subs.insert(0);

        cmd_tx
            .send(HandlerCommand::ResubscribeOrderBook { market_index: 0 })
            .expect("queue resync");
        drop(cmd_tx);
        drop(raw_tx);

        let next = tokio::time::timeout(Duration::from_secs(2), handler.next())
            .await
            .expect("timed out waiting for handler to drain command");

        assert!(next.is_none());
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[tokio::test]
    async fn resubscribe_order_book_queues_when_inflight_is_at_cap() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let subscriptions = SubscriptionState::new(':');
        let topic = LighterWsChannel::OrderBook(0).topic_key();
        assert!(subscriptions.add_reference(&topic));

        let mut handler = FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, subscriptions);
        handler.book_delta_subs.insert(0);
        saturate_subscription_gate(&mut handler);

        handler.resubscribe_order_book_stream(0).await;

        assert_eq!(handler.inflight_subs.len(), SUBSCRIBE_INFLIGHT_MAX);
        assert!(
            handler
                .pending_subs
                .iter()
                .any(|(topic, _)| *topic == Ustr::from("order_book:0")),
        );
    }

    fn stub_candle(
        t: i64,
        open: i64,
        high: i64,
        low: i64,
        close: i64,
        volume_ticks: i64,
    ) -> LighterWsCandle {
        LighterWsCandle {
            t,
            o: Decimal::new(open, 2),
            h: Decimal::new(high, 2),
            l: Decimal::new(low, 2),
            c: Decimal::new(close, 2),
            v: Decimal::new(volume_ticks, 4),
            quote_volume: Decimal::ZERO,
            i: 0,
        }
    }

    fn candle_frame(channel: &str, candle: LighterWsCandle, is_snapshot: bool) -> LighterWsFrame {
        if is_snapshot {
            LighterWsFrame::CandleSnapshot {
                channel: Ustr::from(channel),
                candles: vec![candle],
                timestamp: 0,
            }
        } else {
            LighterWsFrame::Candle {
                channel: Ustr::from(channel),
                candles: vec![candle],
                timestamp: 0,
            }
        }
    }

    #[rstest]
    fn handle_candles_first_observation_caches_without_emit() {
        let mut handler = make_handler_with_account();
        let frame = candle_frame(
            "candle:0:1m",
            stub_candle(1_000_000, 10_000, 10_000, 10_000, 10_000, 10_000),
            true,
        );

        let messages = handler.handle_frame(frame, UnixNanos::from(99));

        assert!(messages.is_empty(), "first observation must not emit");
        let key = (0_i16, LighterCandleResolution::OneMinute);
        assert_eq!(handler.last_candles.get(&key).map(|c| c.t), Some(1_000_000));
    }

    #[rstest]
    fn handle_candles_t_advance_emits_bar_for_previous_candle() {
        let mut handler = make_handler_with_account();
        let prev = stub_candle(1_000_000, 10_000, 11_000, 9_900, 10_500, 10_000);
        let next = stub_candle(1_060_000, 10_500, 10_600, 10_450, 10_550, 20_000);
        let next_t = next.t;
        handler.handle_frame(candle_frame("candle:0:1m", prev, true), UnixNanos::from(1));

        let messages =
            handler.handle_frame(candle_frame("candle:0:1m", next, false), UnixNanos::from(2));

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            NautilusWsMessage::Bar(bar) => {
                // The emitted bar reflects the previous (closed) candle, not the new one.
                assert_eq!(bar.open, Price::from("100.00"));
                assert_eq!(bar.high, Price::from("110.00"));
                assert_eq!(bar.low, Price::from("99.00"));
                assert_eq!(bar.close, Price::from("105.00"));
                assert_eq!(bar.volume, Quantity::from("1.0000"));
                assert_eq!(bar.ts_event, UnixNanos::from(1_000_000 * 1_000_000));
            }
            other => panic!("expected Bar message, was {other:?}"),
        }
        let cached = handler
            .last_candles
            .get(&(0_i16, LighterCandleResolution::OneMinute))
            .expect("cache populated");
        assert_eq!(cached.t, next_t);
    }

    #[rstest]
    fn handle_candles_same_t_updates_cache_without_emit() {
        let mut handler = make_handler_with_account();
        let initial = stub_candle(1_000_000, 10_000, 10_050, 9_950, 10_025, 5_000);
        let same_t_updated = stub_candle(1_000_000, 10_000, 10_100, 9_950, 10_075, 7_500);
        let same_t_h = same_t_updated.h;
        let same_t_c = same_t_updated.c;
        handler.handle_frame(
            candle_frame("candle:0:1m", initial, true),
            UnixNanos::from(1),
        );

        let messages = handler.handle_frame(
            candle_frame("candle:0:1m", same_t_updated, false),
            UnixNanos::from(2),
        );

        assert!(messages.is_empty(), "same-`t` update must not emit");
        let cached = handler
            .last_candles
            .get(&(0_i16, LighterCandleResolution::OneMinute))
            .expect("cache populated");
        assert_eq!(cached.h, same_t_h);
        assert_eq!(cached.c, same_t_c);
    }

    #[rstest]
    fn handle_candles_regressed_t_is_skipped() {
        let mut handler = make_handler_with_account();
        let initial = stub_candle(2_000_000, 10_000, 10_000, 10_000, 10_000, 5_000);
        let regressed = stub_candle(1_000_000, 9_000, 9_000, 9_000, 9_000, 5_000);
        let initial_t = initial.t;
        handler.handle_frame(
            candle_frame("candle:0:1m", initial, true),
            UnixNanos::from(1),
        );

        let messages = handler.handle_frame(
            candle_frame("candle:0:1m", regressed, false),
            UnixNanos::from(2),
        );

        assert!(messages.is_empty(), "regressed `t` must not emit");
        let cached = handler
            .last_candles
            .get(&(0_i16, LighterCandleResolution::OneMinute))
            .expect("cache populated");
        // Regressed frame is skipped entirely; cache stays on the original entry.
        assert_eq!(cached.t, initial_t);
    }

    #[rstest]
    fn handle_candles_unknown_market_returns_empty() {
        let mut handler = make_handler_with_account();
        let frame = candle_frame(
            "candle:99:1m",
            stub_candle(1_000_000, 10_000, 10_000, 10_000, 10_000, 5_000),
            true,
        );

        let messages = handler.handle_frame(frame, UnixNanos::from(1));

        assert!(messages.is_empty());
    }

    #[rstest]
    fn handle_unsubscribe_ack_clears_only_matching_candle_key() {
        let mut handler = make_handler_with_account();
        handler.last_candles.insert(
            (0, LighterCandleResolution::OneMinute),
            stub_candle(1, 0, 0, 0, 0, 0),
        );
        handler.last_candles.insert(
            (0, LighterCandleResolution::FiveMinute),
            stub_candle(2, 0, 0, 0, 0, 0),
        );
        handler.subscriptions.mark_unsubscribe("candle:0:1m");

        let payload = json!({"type": "unsubscribed", "channel": "candle:0:1m"});
        let (matched, _) = handle_control_text(&mut handler, &payload.to_string());

        assert!(matched);
        assert!(
            handler
                .last_candles
                .get(&(0, LighterCandleResolution::OneMinute))
                .is_none(),
        );
        assert!(
            handler
                .last_candles
                .get(&(0, LighterCandleResolution::FiveMinute))
                .is_some(),
        );
    }

    #[rstest]
    #[case::well_formed("candle:0:1m", Some((0, LighterCandleResolution::OneMinute)))]
    #[case::weekly("candle:3:1w", Some((3, LighterCandleResolution::OneWeek)))]
    #[case::other_kind("order_book:0", None)]
    #[case::missing_resolution("candle:0", None)]
    #[case::bad_market("candle:notanint:1m", None)]
    #[case::bad_resolution("candle:0:bogus", None)]
    fn test_candle_market_and_resolution_from_topic(
        #[case] topic: &str,
        #[case] expected: Option<(i16, LighterCandleResolution)>,
    ) {
        assert_eq!(candle_market_and_resolution_from_topic(topic), expected);
    }

    #[rstest]
    #[case::network_retries(LighterWsError::Network("disconnected".into()), true)]
    #[case::auth_does_not_retry(LighterWsError::Authentication("bad token".into()), false)]
    #[case::parse_does_not_retry(LighterWsError::Parse("bad json".into()), false)]
    #[case::client_does_not_retry(LighterWsError::Client("no active WebSocket client".into()), false)]
    #[case::transport_closed_does_not_retry(LighterWsError::Transport(SendError::Closed), false)]
    #[case::transport_invalid_input_does_not_retry(
        LighterWsError::Transport(SendError::InvalidInput("pong payload too large".into())),
        false
    )]
    #[case::transport_connection_changed_does_not_retry(
        LighterWsError::Transport(SendError::ConnectionChanged),
        false
    )]
    #[case::transport_timeout_retries(LighterWsError::Transport(SendError::Timeout), true)]
    #[case::transport_write_timeout_does_not_retry(
        LighterWsError::Transport(SendError::WriteTimeout),
        false
    )]
    #[case::transport_broken_pipe_does_not_retry(
        LighterWsError::Transport(SendError::BrokenPipe(
            "writer closed".into(),
        )),
        false,
    )]
    fn test_should_retry_lighter_ws_error(#[case] error: LighterWsError, #[case] expected: bool) {
        assert_eq!(should_retry_lighter_ws_error(&error), expected);
    }

    // Pins the `#[from] SendError` derive that `.map_err(LighterWsError::Transport)` relies on.
    #[rstest]
    #[case::closed(SendError::Closed)]
    #[case::invalid_input(SendError::InvalidInput("pong payload too large".into()))]
    #[case::timeout(SendError::Timeout)]
    #[case::write_timeout(SendError::WriteTimeout)]
    #[case::connection_changed(SendError::ConnectionChanged)]
    #[case::broken_pipe(SendError::BrokenPipe("writer dropped".into()))]
    fn send_error_converts_into_transport_variant(#[case] send_error: SendError) {
        let err: LighterWsError = send_error.into();
        assert!(
            matches!(err, LighterWsError::Transport(_)),
            "expected Transport variant, was {err:?}",
        );
    }

    #[tokio::test]
    async fn subscribe_command_parks_in_pending_subs_when_inflight_at_cap() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));

        // Saturate the gate so the pump cannot dispatch the queued subscribe
        saturate_subscription_gate(&mut handler);

        cmd_tx
            .send(HandlerCommand::Subscribe {
                channel: LighterWsChannel::Candle {
                    market_index: 0,
                    resolution: LighterCandleResolution::OneMinute,
                },
                auth: None,
                response_tx: None,
            })
            .expect("queue subscribe");
        drop(cmd_tx);
        drop(raw_tx);

        let next = tokio::time::timeout(Duration::from_secs(2), handler.next())
            .await
            .expect("timed out waiting for handler to drain command");

        assert!(next.is_none());
        assert_eq!(handler.inflight_subs.len(), SUBSCRIBE_INFLIGHT_MAX);
        assert_eq!(handler.pending_subs.len(), 1);
        assert_eq!(handler.subscription_attempts.len(), 1);
        assert_eq!(handler.pending_subs[0].0, Ustr::from("candle:0:1m"));
    }

    #[tokio::test]
    async fn unsubscribe_drops_queued_subscribe_while_gate_full() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));

        // Saturate the gate so the queued subscribe cannot dispatch before the unsubscribe
        saturate_subscription_gate(&mut handler);

        cmd_tx
            .send(HandlerCommand::Subscribe {
                channel: LighterWsChannel::Trade(0),
                auth: None,
                response_tx: None,
            })
            .expect("queue subscribe");
        cmd_tx
            .send(HandlerCommand::Unsubscribe {
                channel: LighterWsChannel::Trade(0),
            })
            .expect("queue unsubscribe");
        drop(cmd_tx);
        drop(raw_tx);

        let next = tokio::time::timeout(Duration::from_secs(2), handler.next())
            .await
            .expect("timed out waiting for handler to drain commands");

        assert!(next.is_none());
        assert!(handler.pending_subs.is_empty());
        assert!(handler.subscription_attempts.is_empty());
    }

    #[tokio::test]
    async fn reconnect_requeues_attempt_with_new_generation() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));

        saturate_subscription_gate(&mut handler);
        handler.queue_subscribe(LighterWsChannel::Trade(0), None, None);
        let old_generation = handler.pending_subs[0].1;

        raw_tx
            .send((7, Message::Text(RECONNECTED.to_string().into())))
            .expect("queue reconnect sentinel");

        let next = tokio::time::timeout(Duration::from_secs(2), handler.next())
            .await
            .expect("timed out waiting for reconnect");

        assert!(matches!(next, Some(NautilusWsMessage::Reconnected { .. })));
        assert!(handler.inflight_subs.is_empty());
        assert_eq!(handler.pending_subs.len(), 1);
        assert_eq!(handler.pending_subs[0].0, Ustr::from("trade:0"));
        assert_ne!(handler.pending_subs[0].1, old_generation);
        assert_eq!(
            handler.subscription_attempts[&Ustr::from("trade:0")].generation,
            handler.pending_subs[0].1,
        );
    }

    #[rstest]
    #[tokio::test]
    async fn pump_releases_inflight_slot_and_schedules_send_failure_retry() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));

        // No active client, so every dispatch fails; the gate must not leak slots
        for market_index in 0..3 {
            handler.queue_subscribe(LighterWsChannel::Trade(market_index), None, None);
        }

        handler.pump_pending_subscribes().await;

        assert!(handler.pending_subs.is_empty());
        assert!(handler.inflight_subs.is_empty());
        assert_eq!(handler.subscription_attempts.len(), 3);
        assert!(
            handler
                .subscription_attempts
                .values()
                .all(|attempt| attempt.retries == 1),
        );
        assert_eq!(handler.subscription_retries.len(), 3);
    }

    #[rstest]
    fn subscribed_control_frame_releases_inflight_slot() {
        let mut handler = make_handler_with_account();
        mark_subscription_inflight(
            &mut handler,
            LighterWsChannel::Candle {
                market_index: 0,
                resolution: LighterCandleResolution::OneMinute,
            },
            None,
        );

        let (matched, msg) = handle_control_text(
            &mut handler,
            r#"{"type":"subscribed","channel":"candle:0:1m"}"#,
        );

        assert!(matched);
        assert!(msg.is_none());
        assert!(
            !handler
                .inflight_subs
                .contains_key(&Ustr::from("candle:0:1m"))
        );
        assert!(handler.subscription_attempts.is_empty());
    }

    #[rstest]
    #[case::top_level(
        r#"{"type":"error","code":30003,"message":"Already Subscribed to : account_all_orders:12345"}"#
    )]
    #[case::nested(
        r#"{"error":{"code":30003,"message":"Already Subscribed to : account_all_orders:12345"}}"#
    )]
    #[case::nested_typed(
        r#"{"type":"error","error":{"code":30003,"message":"Already Subscribed to : account_all_orders:12345"}}"#
    )]
    fn already_subscribed_confirms_matching_inflight_topic(#[case] payload: &str) {
        let mut handler = make_handler_with_account();
        mark_subscription_inflight(
            &mut handler,
            LighterWsChannel::AccountAllOrders(12345),
            None,
        );

        let (matched, msg) = handle_control_text(&mut handler, payload);

        assert!(matched);
        assert!(msg.is_none());
        assert!(handler.inflight_subs.is_empty());
        assert!(handler.subscriptions.pending_subscribe_topics().is_empty());
        assert_eq!(handler.subscriptions.len(), 1);
    }

    #[rstest]
    fn already_subscribed_does_not_confirm_unmatched_topic() {
        let mut handler = make_handler_with_account();
        let inflight = "account_all_orders:12345";
        mark_subscription_inflight(
            &mut handler,
            LighterWsChannel::AccountAllOrders(12345),
            None,
        );

        let (matched, msg) = handle_control_text(
            &mut handler,
            r#"{"type":"error","code":30003,"message":"Already Subscribed to : account_all_trades:12345"}"#,
        );

        assert!(matched);
        assert!(msg.is_none());
        assert!(handler.inflight_subs.contains_key(&Ustr::from(inflight)));
        assert_eq!(
            handler.subscriptions.pending_subscribe_topics(),
            vec![inflight]
        );
        assert_eq!(handler.subscriptions.len(), 0);
    }

    #[tokio::test]
    async fn duplicate_queued_and_inflight_topics_share_one_generation() {
        let mut handler = make_handler_with_account();
        let (response_tx_1, mut response_rx_1) = tokio::sync::oneshot::channel();
        let (response_tx_2, mut response_rx_2) = tokio::sync::oneshot::channel();
        let (response_tx_3, mut response_rx_3) = tokio::sync::oneshot::channel();
        let channel = LighterWsChannel::Trade(7);

        handler.queue_subscribe(channel.clone(), None, Some(response_tx_1));
        let generation = handler.pending_subs[0].1;
        handler.queue_subscribe(channel.clone(), None, Some(response_tx_2));

        assert_eq!(handler.pending_subs.len(), 1);
        assert_eq!(handler.subscription_attempts.len(), 1);
        assert_eq!(
            handler.subscription_attempts[&Ustr::from("trade:7")]
                .response_txs
                .len(),
            2,
        );

        let (topic, queued_generation) = handler.pending_subs.pop_front().unwrap();
        handler.inflight_subs.insert(topic, queued_generation);
        handler.queue_subscribe(channel, None, Some(response_tx_3));

        assert_eq!(queued_generation, generation);
        assert!(handler.pending_subs.is_empty());
        assert_eq!(handler.inflight_subs.len(), 1);
        assert_eq!(
            handler.subscription_attempts[&Ustr::from("trade:7")]
                .response_txs
                .len(),
            3,
        );
        assert!(matches!(
            response_rx_1.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));
        assert!(matches!(
            response_rx_2.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));
        assert!(matches!(
            response_rx_3.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));

        handle_control_text(&mut handler, r#"{"type":"subscribed","channel":"trade:7"}"#);

        assert_eq!(response_rx_1.await.unwrap(), Ok(()));
        assert_eq!(response_rx_2.await.unwrap(), Ok(()));
        assert_eq!(response_rx_3.await.unwrap(), Ok(()));
        assert!(handler.inflight_subs.is_empty());
        assert!(handler.subscription_attempts.is_empty());
    }

    #[tokio::test]
    async fn changed_inflight_auth_waits_for_serialized_successor() {
        let mut handler = make_handler_with_account();
        let channel = LighterWsChannel::AccountAllOrders(12345);
        let topic = Ustr::from(channel.topic_key().as_str());
        let (old_tx, old_rx) = tokio::sync::oneshot::channel();
        let (fresh_tx, mut fresh_rx) = tokio::sync::oneshot::channel();

        handler.queue_subscribe(
            channel.clone(),
            Some(SecretString::from("old-token")),
            Some(old_tx),
        );
        let (queued_topic, old_generation) = handler.pending_subs.pop_front().unwrap();
        handler.inflight_subs.insert(queued_topic, old_generation);
        handler.queue_subscribe(
            channel,
            Some(SecretString::from("fresh-token")),
            Some(fresh_tx),
        );

        let attempt = &handler.subscription_attempts[&topic];
        assert_eq!(
            attempt.auth.as_ref().map(SecretString::expose_secret),
            Some("old-token")
        );
        assert_eq!(
            attempt
                .pending_auth
                .as_ref()
                .map(SecretString::expose_secret),
            Some("fresh-token")
        );
        assert_eq!(attempt.response_txs.len(), 1);
        assert_eq!(attempt.pending_response_txs.len(), 1);

        assert!(handler.complete_subscription(topic.as_str(), CompletionKind::ControlAck));
        assert_eq!(old_rx.await.unwrap(), Ok(()));
        assert!(matches!(
            fresh_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));
        let attempt = &handler.subscription_attempts[&topic];
        assert_ne!(attempt.generation, old_generation);
        assert_eq!(
            attempt.auth.as_ref().map(SecretString::expose_secret),
            Some("fresh-token")
        );
        assert!(attempt.pending_auth.is_none());
        assert_eq!(
            handler.pending_subs.front(),
            Some(&(topic, attempt.generation)),
        );

        let (_, fresh_generation) = handler.pending_subs.pop_front().unwrap();
        handler.inflight_subs.insert(topic, fresh_generation);

        // The predecessor's trailing typed snapshot must not complete the
        // dispatched successor.
        assert!(!handler.complete_subscription(topic.as_str(), CompletionKind::Typed));
        assert!(matches!(
            fresh_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));
        assert_eq!(
            handler.subscription_attempts[&topic].generation,
            fresh_generation,
        );

        assert!(handler.complete_subscription(topic.as_str(), CompletionKind::ControlAck));
        assert_eq!(fresh_rx.await.unwrap(), Ok(()));
        assert!(handler.subscription_attempts.is_empty());
    }

    #[tokio::test]
    async fn stale_typed_frame_does_not_complete_fresh_attempt_after_predecessor_removed() {
        let mut handler = make_handler_with_account();
        let channel = LighterWsChannel::AccountAllOrders(12345);
        let topic = Ustr::from(channel.topic_key().as_str());
        let (old_tx, old_rx) = tokio::sync::oneshot::channel();
        let (fresh_tx, mut fresh_rx) = tokio::sync::oneshot::channel();

        // Predecessor completes on its control ack and is removed before any
        // pending auth exists.
        handler.queue_subscribe(
            channel.clone(),
            Some(SecretString::from("old-token")),
            Some(old_tx),
        );
        let (queued_topic, old_generation) = handler.pending_subs.pop_front().unwrap();
        handler.inflight_subs.insert(queued_topic, old_generation);
        assert!(handler.complete_subscription(topic.as_str(), CompletionKind::ControlAck));
        assert_eq!(old_rx.await.unwrap(), Ok(()));
        assert!(handler.subscription_attempts.is_empty());

        // The fresh command then creates and dispatches a brand-new attempt.
        handler.queue_subscribe(
            channel,
            Some(SecretString::from("fresh-token")),
            Some(fresh_tx),
        );
        let (_, fresh_generation) = handler.pending_subs.pop_front().unwrap();
        handler.inflight_subs.insert(topic, fresh_generation);

        // The predecessor's trailing typed snapshot must not complete it.
        assert!(!handler.complete_subscription(topic.as_str(), CompletionKind::Typed));
        assert!(matches!(
            fresh_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));
        assert_eq!(
            handler.subscription_attempts[&topic].generation,
            fresh_generation,
        );

        assert!(handler.complete_subscription(topic.as_str(), CompletionKind::ControlAck));
        assert_eq!(fresh_rx.await.unwrap(), Ok(()));
        assert!(handler.subscription_attempts.is_empty());
    }

    #[tokio::test]
    async fn changed_queued_auth_updates_existing_generation_before_dispatch() {
        let mut handler = make_handler_with_account();
        let channel = LighterWsChannel::AccountAllOrders(12345);
        let topic = Ustr::from(channel.topic_key().as_str());
        let (old_tx, old_rx) = tokio::sync::oneshot::channel();
        let (fresh_tx, fresh_rx) = tokio::sync::oneshot::channel();

        handler.queue_subscribe(
            channel.clone(),
            Some(SecretString::from("old-token")),
            Some(old_tx),
        );
        let generation = handler.pending_subs[0].1;
        handler.queue_subscribe(
            channel,
            Some(SecretString::from("fresh-token")),
            Some(fresh_tx),
        );

        assert_eq!(handler.pending_subs.len(), 1);
        assert_eq!(handler.pending_subs[0], (topic, generation));
        let attempt = &handler.subscription_attempts[&topic];
        assert_eq!(attempt.generation, generation);
        assert_eq!(
            attempt.auth.as_ref().map(SecretString::expose_secret),
            Some("fresh-token")
        );
        assert!(attempt.pending_auth.is_none());
        assert_eq!(attempt.response_txs.len(), 2);
        assert!(attempt.pending_response_txs.is_empty());

        handler.pending_subs.pop_front();
        handler.inflight_subs.insert(topic, generation);
        assert!(handler.complete_subscription(topic.as_str(), CompletionKind::ControlAck));
        assert_eq!(old_rx.await.unwrap(), Ok(()));
        assert_eq!(fresh_rx.await.unwrap(), Ok(()));
    }

    #[rstest]
    fn second_reconnect_folds_pending_auth_into_requeued_generation() {
        let mut handler = make_handler_with_account();
        let channel = LighterWsChannel::AccountAllOrders(12345);
        let topic = Ustr::from(channel.topic_key().as_str());
        let (old_tx, mut old_rx) = tokio::sync::oneshot::channel();
        let (fresh_tx, mut fresh_rx) = tokio::sync::oneshot::channel();

        handler.queue_subscribe(
            channel.clone(),
            Some(SecretString::from("old-token")),
            Some(old_tx),
        );
        handler.reset_subscription_attempts_after_reconnect();
        let (_, replay_generation) = handler.pending_subs.pop_front().unwrap();
        handler.inflight_subs.insert(topic, replay_generation);
        handler.queue_subscribe(
            channel,
            Some(SecretString::from("fresh-token")),
            Some(fresh_tx),
        );

        let attempt = &handler.subscription_attempts[&topic];
        assert_eq!(
            attempt.auth.as_ref().map(SecretString::expose_secret),
            Some("old-token")
        );
        assert_eq!(
            attempt
                .pending_auth
                .as_ref()
                .map(SecretString::expose_secret),
            Some("fresh-token")
        );
        assert_eq!(attempt.response_txs.len(), 1);
        assert_eq!(attempt.pending_response_txs.len(), 1);

        handler.reset_subscription_attempts_after_reconnect();

        let attempt = &handler.subscription_attempts[&topic];
        assert_ne!(attempt.generation, replay_generation);
        assert_eq!(
            attempt.auth.as_ref().map(SecretString::expose_secret),
            Some("fresh-token")
        );
        assert!(attempt.pending_auth.is_none());
        assert_eq!(attempt.response_txs.len(), 2);
        assert!(attempt.pending_response_txs.is_empty());
        assert_eq!(handler.pending_subs.len(), 1);
        assert_eq!(handler.pending_subs[0], (topic, attempt.generation));
        assert!(handler.inflight_subs.is_empty());
        assert!(matches!(
            old_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));
        assert!(matches!(
            fresh_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));

        let (_, generation) = handler.pending_subs.pop_front().unwrap();
        handler.inflight_subs.insert(topic, generation);
        assert!(handler.complete_subscription(topic.as_str(), CompletionKind::ControlAck));

        assert_eq!(old_rx.try_recv(), Ok(Ok(())));
        assert_eq!(fresh_rx.try_recv(), Ok(Ok(())));
        assert!(handler.subscription_attempts.is_empty());
    }

    #[tokio::test]
    async fn retry_folds_pending_auth_without_resetting_retry_budget() {
        let mut handler = make_handler_with_account();
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.set_command_sender(cmd_tx);
        let channel = LighterWsChannel::AccountAllOrders(12345);
        let topic = Ustr::from(channel.topic_key().as_str());
        let (old_tx, mut old_rx) = tokio::sync::oneshot::channel();
        let (fresh_tx, mut fresh_rx) = tokio::sync::oneshot::channel();

        handler.queue_subscribe(
            channel.clone(),
            Some(SecretString::from("old-token")),
            Some(old_tx),
        );
        let (_, generation) = handler.pending_subs.pop_front().unwrap();
        handler.inflight_subs.insert(topic, generation);
        handler
            .subscription_attempts
            .get_mut(&topic)
            .unwrap()
            .retries = 2;
        handler.queue_subscribe(
            channel,
            Some(SecretString::from("fresh-token")),
            Some(fresh_tx),
        );

        handler.schedule_subscription_retry(topic, generation, "retry");

        let attempt = &handler.subscription_attempts[&topic];
        assert_eq!(attempt.retries, 3);
        assert_ne!(attempt.generation, generation);
        assert_eq!(
            attempt.auth.as_ref().map(SecretString::expose_secret),
            Some("fresh-token")
        );
        assert!(attempt.pending_auth.is_none());
        assert_eq!(attempt.response_txs.len(), 2);
        assert!(attempt.pending_response_txs.is_empty());
        assert!(handler.pending_subs.is_empty());
        assert!(handler.inflight_subs.is_empty());
        assert_eq!(handler.subscription_retries.len(), 1);
        assert!(matches!(
            old_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));
        assert!(matches!(
            fresh_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));
    }

    #[tokio::test]
    async fn confirmed_authenticated_topic_opens_new_generation() {
        let mut handler = make_handler_with_account();
        let channel = LighterWsChannel::AccountAllOrders(12345);
        let topic = channel.topic_key();
        handler.subscriptions.mark_subscribe(&topic);
        handler.subscriptions.confirm_subscribe(&topic);
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        handler.queue_subscribe(
            channel,
            Some(SecretString::from("rotated-auth-token")),
            Some(response_tx),
        );

        assert_eq!(handler.pending_subs.len(), 1);
        assert_eq!(handler.subscription_attempts.len(), 1);
        assert_eq!(handler.subscriptions.len(), 1);
        assert!(handler.subscriptions.pending_subscribe_topics().is_empty());

        let (topic, generation) = handler.pending_subs.pop_front().unwrap();
        handler.inflight_subs.insert(topic, generation);
        handle_control_text(
            &mut handler,
            r#"{"type":"subscribed","channel":"account_all_orders:12345"}"#,
        );

        assert_eq!(response_rx.await.unwrap(), Ok(()));
        assert!(handler.subscription_attempts.is_empty());
        assert_eq!(handler.subscriptions.len(), 1);
    }

    #[tokio::test]
    async fn typed_update_before_ack_does_not_complete_subscription_generation() {
        let mut handler = make_handler_with_account();
        let (response_tx, mut response_rx) = tokio::sync::oneshot::channel();
        let (topic, generation) = mark_subscription_inflight(
            &mut handler,
            LighterWsChannel::Candle {
                market_index: 0,
                resolution: LighterCandleResolution::OneMinute,
            },
            Some(response_tx),
        );

        handler.handle_frame(
            candle_frame(
                "candle:0:1m",
                stub_candle(1_000_000, 10_000, 10_000, 10_000, 10_000, 10_000),
                false,
            ),
            UnixNanos::from(1),
        );

        assert_eq!(
            handler
                .last_candles
                .get(&(0, LighterCandleResolution::OneMinute))
                .map(|candle| candle.t),
            Some(1_000_000),
        );
        assert_eq!(handler.inflight_subs.get(&topic), Some(&generation));
        assert_eq!(
            handler.subscriptions.pending_subscribe_topics(),
            vec!["candle:0:1m"],
        );
        assert_eq!(handler.subscriptions.len(), 0);
        assert!(matches!(
            response_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));

        handle_control_text(
            &mut handler,
            r#"{"type":"subscribed","channel":"candle:0:1m"}"#,
        );

        assert_eq!(response_rx.await.unwrap(), Ok(()));
        assert_eq!(handler.subscriptions.len(), 1);
    }

    #[tokio::test]
    async fn typed_subscribed_frame_completes_subscription_generation() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        let mut handler =
            FeedHandler::new(signal, cmd_rx, raw_rx, out_tx, SubscriptionState::new(':'));
        handler.instruments.insert(0, stub_eth_perp_instrument());
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        mark_subscription_inflight(
            &mut handler,
            LighterWsChannel::Candle {
                market_index: 0,
                resolution: LighterCandleResolution::OneMinute,
            },
            Some(response_tx),
        );

        raw_tx
            .send((7, Message::Text(WS_CANDLE_SUBSCRIBED.into())))
            .expect("typed subscribed frame");
        drop(raw_tx);
        drop(cmd_tx);

        let next = tokio::time::timeout(Duration::from_secs(2), handler.next())
            .await
            .expect("handler did not process typed subscribed frame");

        assert!(next.is_none());
        assert_eq!(response_rx.await.unwrap(), Ok(()));
        assert!(handler.inflight_subs.is_empty());
        assert!(handler.subscription_attempts.is_empty());
        assert_eq!(handler.subscriptions.len(), 1);
    }

    #[tokio::test]
    async fn failed_subscribe_error_fails_every_waiter_for_the_generation() {
        let mut handler = make_handler_with_account();
        let (response_tx_1, response_rx_1) = tokio::sync::oneshot::channel();
        let (response_tx_2, response_rx_2) = tokio::sync::oneshot::channel();
        let channel = LighterWsChannel::MarketStats(LighterMarketSelection::Market(0));
        mark_subscription_inflight(&mut handler, channel.clone(), Some(response_tx_1));
        handler.queue_subscribe(channel, None, Some(response_tx_2));

        let (matched, msg) = handle_control_text(
            &mut handler,
            r#"{"type":"error","code":30012,"message":"failed to subscribe"}"#,
        );

        assert!(matched);
        assert!(msg.is_none());
        let error_1 = response_rx_1.await.unwrap().unwrap_err();
        let error_2 = response_rx_2.await.unwrap().unwrap_err();
        assert!(error_1.contains("market_stats:0"));
        assert!(error_1.contains("30012"));
        assert_eq!(error_1, error_2);
        assert!(handler.inflight_subs.is_empty());
        assert!(handler.subscription_attempts.is_empty());
        assert!(handler.subscriptions.is_empty());
    }

    #[tokio::test]
    async fn rate_limit_retry_exhaustion_fails_waiter_and_clears_intent() {
        let mut handler = make_handler_with_account();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let (topic, _) =
            mark_subscription_inflight(&mut handler, LighterWsChannel::Trade(7), Some(response_tx));
        handler
            .subscription_attempts
            .get_mut(&topic)
            .expect("subscription attempt")
            .retries = SUBSCRIBE_RETRY_MAX;

        let (matched, msg) = handle_control_text(
            &mut handler,
            r#"{"type":"error","code":30009,"message":"rate limit exceeded"}"#,
        );

        assert!(matched);
        assert!(msg.is_none());
        assert_eq!(
            response_rx.await.unwrap(),
            Err(
                "subscription trade:7 failed after 6 attempts: venue rejected the WebSocket \
                 subscribe with code 30009"
                    .to_string(),
            ),
        );
        assert!(handler.inflight_subs.is_empty());
        assert!(handler.subscription_attempts.is_empty());
        assert!(handler.subscriptions.is_empty());
    }

    #[tokio::test]
    async fn rate_limit_error_retries_each_inflight_topic_with_new_generation() {
        let mut handler = make_handler_with_account();
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.set_command_sender(cmd_tx);
        let (trade_tx, mut trade_rx) = tokio::sync::oneshot::channel();
        let (candle_tx, mut candle_rx) = tokio::sync::oneshot::channel();
        let (trade_topic, trade_generation) =
            mark_subscription_inflight(&mut handler, LighterWsChannel::Trade(7), Some(trade_tx));
        let (candle_topic, candle_generation) = mark_subscription_inflight(
            &mut handler,
            LighterWsChannel::Candle {
                market_index: 0,
                resolution: LighterCandleResolution::OneMinute,
            },
            Some(candle_tx),
        );

        let (matched, msg) = handle_control_text(
            &mut handler,
            r#"{"type":"error","code":30009,"message":"rate limit exceeded"}"#,
        );

        assert!(matched);
        assert!(msg.is_none());
        assert!(handler.inflight_subs.is_empty());
        assert!(handler.pending_subs.is_empty());
        let trade_retry = &handler.subscription_attempts[&trade_topic];
        let candle_retry = &handler.subscription_attempts[&candle_topic];
        assert_eq!(trade_retry.retries, 1);
        assert_eq!(candle_retry.retries, 1);
        assert_ne!(trade_retry.generation, trade_generation);
        assert_ne!(candle_retry.generation, candle_generation);
        assert_eq!(handler.subscription_retries.len(), 2);
        assert!(matches!(
            trade_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));
        assert!(matches!(
            candle_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty),
        ));

        handle_control_text(&mut handler, r#"{"type":"subscribed","channel":"trade:7"}"#);
        assert_eq!(handler.subscription_attempts.len(), 2);
        assert_eq!(handler.subscriptions.len(), 0);

        let retry_generations = [
            (
                trade_topic,
                handler.subscription_attempts[&trade_topic].generation,
            ),
            (
                candle_topic,
                handler.subscription_attempts[&candle_topic].generation,
            ),
        ];

        for (topic, generation) in retry_generations {
            handler.queue_subscription_retry(topic, generation);
            let queued = handler.pending_subs.pop_front().unwrap();
            assert_eq!(queued, (topic, generation));
            handler.inflight_subs.insert(topic, generation);
        }

        handle_control_text(&mut handler, r#"{"type":"subscribed","channel":"trade:7"}"#);
        handle_control_text(
            &mut handler,
            r#"{"type":"subscribed","channel":"candle:0:1m"}"#,
        );

        assert_eq!(trade_rx.await.unwrap(), Ok(()));
        assert_eq!(candle_rx.await.unwrap(), Ok(()));
        assert!(handler.subscription_attempts.is_empty());
        assert_eq!(handler.subscriptions.len(), 2);
    }
}
