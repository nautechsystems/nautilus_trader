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

//! WebSocket message handler for Hyperliquid.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque, btree_map::Entry},
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use nautilus_common::cache::fifo::FifoCache;
use nautilus_core::{AtomicTime, Params, nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::book::snapshot::SnapshotGate;
use nautilus_model::{
    data::{Bar, BarType, CustomData, Data, DataType},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    types::Price,
};
use nautilus_network::{
    RECONNECTED,
    error::SendError,
    retry::{RetryError, RetryManager, create_websocket_retry_manager},
    websocket::{SubscriptionState, WebSocketClient},
};
use rust_decimal::Decimal;
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    client::{AssetContextDataType, CloidCache},
    enums::HyperliquidWsChannel,
    error::HyperliquidWsError,
    l2::CompactBook,
    messages::{
        CandleData, ExecutionReport, HyperliquidWsMessage, HyperliquidWsRequest, NautilusWsMessage,
        PostRequest, SubscriptionRequest, WsActiveAssetCtxData, WsAllDexsAssetCtxsData,
        WsCompactBookData, WsMessageHeader, WsUserEventData,
    },
    parse::{
        parse_ws_asset_context, parse_ws_candle, parse_ws_fill_report, parse_ws_open_interest,
        parse_ws_order_book_deltas, parse_ws_order_book_depth, parse_ws_order_status_report,
        parse_ws_public_trade, parse_ws_quote_tick, parse_ws_trade_tick, parse_ws_twap_history_row,
        parse_ws_twap_slice_fill,
    },
    post::PostRouter,
    rate_limits::WebSocketRateLimits,
    trades::TradeStreamUses,
};
use crate::{
    common::{consts::HEARTBEAT_INTERVAL, parse::millis_to_nanos},
    data_types::{
        HyperliquidAllDexsAssetCtxs, HyperliquidAllMids, HyperliquidDexAssetCtx,
        HyperliquidImpactPrices,
    },
};

const HEARTBEAT_MESSAGE: &str = r#"{"method":"ping"}"#;
const CANDLE_COMPLETION_GRACE_MS: u64 = 2_000;

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
#[allow(private_interfaces)]
pub enum HandlerCommand {
    /// Installs the WebSocket client and acknowledges initialization.
    SetClient {
        client: WebSocketClient,
        initialized: tokio::sync::oneshot::Sender<()>,
    },
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Subscribe to the given subscriptions.
    Subscribe {
        subscriptions: Vec<SubscriptionRequest>,
    },
    /// Unsubscribe from the given subscriptions.
    Unsubscribe {
        subscriptions: Vec<SubscriptionRequest>,
    },
    /// Resubscribes without interleaving handler input between the two sends.
    Resubscribe { subscription: SubscriptionRequest },
    /// Writes a book subscription on the current connection, opening `gate` once written.
    ///
    /// With `replace`, sends the unsubscribe first so the venue restarts the stream with a
    /// snapshot. The write always runs, since the stream registry already counts it; `cancel`
    /// only keeps the gate closed.
    WriteBook {
        subscription: SubscriptionRequest,
        replace: bool,
        cancel: CancellationToken,
        gate: SnapshotGate,
        completion: tokio::sync::oneshot::Sender<Result<(), HyperliquidWsError>>,
    },
    /// Send a WebSocket post request.
    Post {
        id: u64,
        request: PostRequest,
        deadline: tokio::time::Instant,
        cancellation_token: CancellationToken,
    },
    /// Initialize the instruments cache with the given instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
    /// Add a bar type mapping for candle parsing.
    AddBarType { key: String, bar_type: BarType },
    /// Remove a bar type mapping.
    RemoveBarType { key: String },
    /// Update asset context subscriptions for a coin.
    UpdateAssetContextSubs {
        coin: Ustr,
        data_types: AHashSet<AssetContextDataType>,
    },
    /// Update the logical consumers of a `trades` stream for a coin.
    UpdateTradeSubs { coin: Ustr, uses: TradeStreamUses },
    /// Cache the ordered instrument IDs needed to normalize `allDexsAssetCtxs`.
    CacheAllDexAssetCtxsInstrumentIds(AHashMap<Ustr, Vec<Option<InstrumentId>>>),
    /// Cache spot fill coin mappings for instrument lookup.
    CacheSpotFillCoins(AHashMap<Ustr, Ustr>),
    /// Flag whether the book stream for `coin` should also publish native depth snapshots.
    SetDepthSub { coin: Ustr, subscribed: bool },
}

#[derive(Default)]
struct AssetContextCaches {
    mark_price: AHashMap<Ustr, Decimal>,
    index_price: AHashMap<Ustr, Decimal>,
    funding_rate: AHashMap<Ustr, Decimal>,
    open_interest: AHashMap<Ustr, Decimal>,
}

impl AssetContextCaches {
    fn clear(&mut self, coin: Ustr, data_type: AssetContextDataType) {
        match data_type {
            AssetContextDataType::MarkPrice => {
                self.mark_price.remove(&coin);
            }
            AssetContextDataType::IndexPrice => {
                self.index_price.remove(&coin);
            }
            AssetContextDataType::FundingRate => {
                self.funding_rate.remove(&coin);
            }
            AssetContextDataType::OpenInterest => {
                self.open_interest.remove(&coin);
            }
        }
    }

    fn clear_removed(
        &mut self,
        coin: Ustr,
        previous_data_types: Option<&AHashSet<AssetContextDataType>>,
        next_data_types: &AHashSet<AssetContextDataType>,
    ) {
        let Some(previous_data_types) = previous_data_types else {
            return;
        };

        for data_type in previous_data_types {
            if !next_data_types.contains(data_type) {
                self.clear(coin, *data_type);
            }
        }
    }
}

#[derive(Debug)]
struct AllMidsDataTypeCache {
    dexes: BTreeSet<Option<String>>,
    projected: Vec<DataType>,
}

impl Default for AllMidsDataTypeCache {
    fn default() -> Self {
        let mut cache = Self {
            dexes: BTreeSet::new(),
            projected: Vec::new(),
        };
        cache.rebuild();
        cache
    }
}

impl AllMidsDataTypeCache {
    fn apply(&mut self, subscription: &SubscriptionRequest, subscribed: bool) {
        let SubscriptionRequest::AllMids { dex } = subscription else {
            return;
        };
        let changed = if subscribed {
            self.dexes.insert(dex.clone())
        } else {
            self.dexes.remove(dex)
        };

        if changed {
            self.rebuild();
        }
    }

    fn as_slice(&self) -> &[DataType] {
        &self.projected
    }

    fn rebuild(&mut self) {
        self.projected.clear();
        if self.dexes.is_empty() {
            self.projected
                .push(DataType::new("HyperliquidAllMids", None, None));
            return;
        }

        self.projected.extend(self.dexes.iter().map(|dex| {
            let metadata = dex.as_ref().map(|dex| {
                Params::new([("dex".to_owned(), serde_json::Value::String(dex.clone()))].into())
            });
            DataType::new("HyperliquidAllMids", metadata, None)
        }));
    }
}

pub(super) struct FeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    cmd_closed: bool,
    raw_closed: bool,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    account_id: Option<AccountId>,
    subscriptions: SubscriptionState,
    all_mids_data_types: AllMidsDataTypeCache,
    post_router: Arc<PostRouter>,
    rate_limits: Arc<WebSocketRateLimits>,
    client_id: u64,
    heartbeat: tokio::time::Interval,
    retry_manager: RetryManager<HyperliquidWsError>,
    retry_manager_post: RetryManager<PostSendError>,
    message_buffer: VecDeque<NautilusWsMessage>,
    instruments: AHashMap<Ustr, InstrumentAny>,
    cloid_cache: CloidCache,
    candles: CandleStreams,
    candle_expiry: Option<CandleExpiry>,
    asset_context_subs: AHashMap<Ustr, AHashSet<AssetContextDataType>>,
    trade_subs: AHashMap<Ustr, TradeStreamUses>,
    all_dex_asset_ctxs_instrument_ids: AHashMap<Ustr, Vec<Option<InstrumentId>>>,
    depth_subs: AHashSet<Ustr>,
    compact_books: AHashMap<Ustr, CompactBookStream>,
    processed_trade_ids: FifoCache<u64, 10_000>,
    processed_public_trade_ids: FifoCache<(Ustr, u64), 10_000>,
    asset_context_caches: AssetContextCaches,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    #[allow(
        clippy::too_many_arguments,
        reason = "constructs the handler from independent runtime channels and caches"
    )]
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        account_id: Option<AccountId>,
        subscriptions: SubscriptionState,
        cloid_cache: CloidCache,
        post_router: Arc<PostRouter>,
        rate_limits: Arc<WebSocketRateLimits>,
        client_id: u64,
    ) -> Self {
        let heartbeat = tokio::time::interval_at(
            tokio::time::Instant::now() + HEARTBEAT_INTERVAL,
            HEARTBEAT_INTERVAL,
        );
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            cmd_closed: false,
            raw_closed: false,
            out_tx,
            account_id,
            subscriptions,
            all_mids_data_types: AllMidsDataTypeCache::default(),
            post_router,
            rate_limits,
            client_id,
            heartbeat,
            retry_manager: create_websocket_retry_manager(),
            retry_manager_post: create_websocket_retry_manager(),
            message_buffer: VecDeque::new(),
            instruments: AHashMap::new(),
            cloid_cache,
            candles: CandleStreams::default(),
            candle_expiry: None,
            asset_context_subs: AHashMap::new(),
            trade_subs: AHashMap::new(),
            all_dex_asset_ctxs_instrument_ids: AHashMap::new(),
            depth_subs: AHashSet::new(),
            compact_books: AHashMap::new(),
            processed_trade_ids: FifoCache::new(),
            processed_public_trade_ids: FifoCache::new(),
            asset_context_caches: AssetContextCaches::default(),
        }
    }

    /// Send a message to the output channel.
    pub(super) fn send(&self, msg: NautilusWsMessage) -> Result<(), String> {
        self.out_tx
            .send(msg)
            .map_err(|e| format!("Failed to send message: {e}"))
    }

    /// Check if the handler has received a stop signal.
    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    async fn send_with_retry(&self, payload: String) -> anyhow::Result<()> {
        if let Some(client) = &self.client {
            let rate_key = self.rate_limits.message_key();
            self.retry_manager
                .invocation(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        async move {
                            client
                                .send_text(payload, Some(std::slice::from_ref(&rate_key)))
                                .await
                                .map_err(|e| {
                                    HyperliquidWsError::ClientError(format!("Send failed: {e}"))
                                })
                        }
                    },
                    should_retry_hyperliquid_error,
                    |e| create_hyperliquid_timeout_error(e.to_string()),
                )
                .execute()
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            Err(anyhow::anyhow!("No WebSocket client available"))
        }
    }

    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        if let Some(msg) = self.message_buffer.pop_front() {
            return Some(msg);
        }

        loop {
            if self.raw_closed && self.cmd_rx.is_empty() {
                log::debug!("Handler shutting down: input stream closed");
                return None;
            }

            if let Some(message) = self.emit_expired_candles() {
                return Some(message);
            }

            let candle_deadline = self
                .candles
                .deadline()
                .filter(|_| self.candle_expiry.is_none());

            tokio::select! {
                cmd = self.cmd_rx.recv(), if !self.cmd_closed => {
                    let Some(cmd) = cmd else {
                        self.cmd_closed = true;
                        continue;
                    };

                    match cmd {
                        HandlerCommand::SetClient { client, initialized } => {
                            log::debug!("Setting WebSocket client in handler");
                            self.client = Some(client);
                            let _ = initialized.send(());
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Handler received disconnect command");

                            if let Some(ref client) = self.client {
                                self.rate_limits.acquire_message().await;
                                client.disconnect().await;
                            }
                            self.signal.store(true, Ordering::SeqCst);
                            return None;
                        }
                        HandlerCommand::Subscribe { subscriptions } => {
                            self.subscribe(subscriptions).await;
                        }
                        HandlerCommand::Unsubscribe { subscriptions } => {
                            self.unsubscribe(subscriptions).await;
                        }
                        HandlerCommand::Resubscribe { subscription } => {
                            self.unsubscribe(vec![subscription.clone()]).await;
                            self.subscribe(vec![subscription]).await;
                        }
                        HandlerCommand::WriteBook {
                            subscription,
                            replace,
                            cancel,
                            gate,
                            completion,
                        } => {
                            // The registry already counts the stream, so skipping or interrupting
                            // the write could leave a stream that depth shares unsubscribed; a
                            // later queued unsubscribe settles the final state instead.
                            let result = self.write_book(subscription, replace).await;

                            if result.is_ok() && !cancel.is_cancelled() {
                                gate.open();
                            }
                            let _ = completion.send(result);
                        }
                        HandlerCommand::Post {
                            id,
                            request,
                            deadline,
                            cancellation_token,
                        } => {
                            if cancellation_token.is_cancelled()
                                || tokio::time::Instant::now() >= deadline
                            {
                                self.post_router
                                    .cancel_registration(id, &cancellation_token)
                                    .await;
                                continue;
                            }

                            let request = HyperliquidWsRequest::Post { id, request };
                            match serde_json::to_string(&request) {
                                Ok(payload) => {
                                    log::debug!("Sending post payload: id={id}");
                                    let result = if let Some(client) = &self.client {
                                        let rate_key = self.rate_limits.message_key();
                                        send_post_with_retry(
                                            &self.retry_manager_post,
                                            deadline,
                                            &cancellation_token,
                                            || {
                                                let payload = payload.clone();
                                                async move {
                                                    let connection_epoch =
                                                        client.connection_epoch();
                                                    client
                                                        .send_text_on_connection(
                                                            payload,
                                                            Some(std::slice::from_ref(&rate_key)),
                                                            connection_epoch,
                                                        )
                                                        .await
                                                        .map_err(PostSendError::Transport)
                                                }
                                            },
                                        )
                                        .await
                                        .map_err(|e| anyhow::anyhow!("{e}"))
                                    } else {
                                        Err(anyhow::anyhow!("No WebSocket client available"))
                                    };

                                    if let Err(e) = result {
                                        log::warn!("Error sending post request id={id}: {e}");
                                        self.post_router
                                            .cancel_registration(id, &cancellation_token)
                                            .await;
                                    }
                                }
                                Err(e) => {
                                    log::error!("Error serializing post request id={id}: {e}");
                                    self.post_router
                                        .cancel_registration(id, &cancellation_token)
                                        .await;
                                }
                            }
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                let coin = inst.raw_symbol().inner();
                                self.instruments.insert(coin, inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            let coin = inst.raw_symbol().inner();
                            self.instruments.insert(coin, inst);
                        }
                        HandlerCommand::AddBarType { key, bar_type } => {
                            self.candles.register(key, bar_type);
                        }
                        HandlerCommand::RemoveBarType { key } => {
                            self.candles.unregister(&key);
                        }
                        HandlerCommand::UpdateAssetContextSubs { coin, data_types } => {
                            let previous_data_types = self.asset_context_subs.get(&coin).cloned();
                            self.asset_context_caches.clear_removed(
                                coin,
                                previous_data_types.as_ref(),
                                &data_types,
                            );

                            if data_types.is_empty() {
                                self.asset_context_subs.remove(&coin);
                            } else {
                                self.asset_context_subs.insert(coin, data_types);
                            }
                        }
                        HandlerCommand::UpdateTradeSubs { coin, uses } => {
                            if uses.is_empty() {
                                self.trade_subs.remove(&coin);
                            } else {
                                self.trade_subs.insert(coin, uses);
                            }
                        }
                        HandlerCommand::CacheAllDexAssetCtxsInstrumentIds(mappings) => {
                            // merge by dex to match the client cache, so a partial
                            // mapping keeps the entries of dexes it did not cover
                            self.all_dex_asset_ctxs_instrument_ids.extend(mappings);
                        }
                        HandlerCommand::CacheSpotFillCoins(_) => {
                            // No longer needed - raw_symbol now contains the proper format
                        }
                        HandlerCommand::SetDepthSub { coin, subscribed } => {
                            if subscribed {
                                self.depth_subs.insert(coin);
                            } else {
                                self.depth_subs.remove(&coin);
                            }
                        }
                    }
                }

                raw_msg = self.raw_rx.recv(), if !self.raw_closed => {
                    let Some(raw_msg) = raw_msg else {
                        self.raw_closed = true;
                        continue;
                    };
                    if let Some(expiry) = &mut self.candle_expiry {
                        expiry.frames_remaining -= 1;
                    }

                    match raw_msg {
                        Message::Text(text) => {
                            if text == RECONNECTED {
                                self.compact_books.clear();
                                log::info!("Received RECONNECTED sentinel");
                                return Some(NautilusWsMessage::Reconnected);
                            }

                            let context = FrameContext {
                                client_id: self.client_id,
                                payload_bytes: text.len(),
                            };

                            match super::messages::decode_ws_message(&text) {
                                Ok(msg) => {
                                    if let HyperliquidWsMessage::Post { data } = msg {
                                        self.post_router.complete(data).await;
                                        continue;
                                    }

                                    if let HyperliquidWsMessage::SubscriptionResponse { data } = &msg {
                                        self.confirm_compact_request(&data.subscription, &data.method);
                                        let key = subscription_response_key(&data.subscription, &self.subscriptions);
                                        match data.method.as_str() {
                                            "subscribe" => self.subscriptions.confirm_subscribe(&key),
                                            "unsubscribe" => {
                                                let was_pending = self
                                                    .subscriptions
                                                    .pending_unsubscribe_topics()
                                                    .iter()
                                                    .any(|topic| topic == &key);
                                                self.subscriptions.confirm_unsubscribe(&key);

                                                if was_pending {
                                                    self.rate_limits.release_subscription(
                                                        self.client_id,
                                                        &key,
                                                    );
                                                }
                                            }
                                            _ => {
                                                context.log_conversion_error("subscription_response", log::Level::Warn);
                                            }
                                        }
                                        continue;
                                    }

                                    let ts_init = self.clock.get_time_ns();

                                    if let HyperliquidWsMessage::Candle { data } = msg {
                                        if let Some(bar) = self.candles.observe(data, &self.instruments, ts_init) {
                                            return Some(NautilusWsMessage::Candle(bar));
                                        }
                                        continue;
                                    }

                                    if let HyperliquidWsMessage::L2 { data } = msg {
                                        if let Some(message) = self.handle_compact_book(data, ts_init) {
                                            return Some(message);
                                        }
                                        continue;
                                    }

                                    if matches!(&msg, HyperliquidWsMessage::Error { .. })
                                        && self.compact_books.values().any(|stream| !stream.pending.is_empty())
                                    {
                                        // A rejection has no subscribe ACK to retire its pending operation
                                        self.invalidate_compact_books();
                                    }

                                    let nautilus_msgs = Self::parse_to_nautilus_messages(
                                        msg,
                                        context,
                                        &self.instruments,
                                        &self.cloid_cache,
                                        self.account_id,
                                        ts_init,
                                        &self.asset_context_subs,
                                        &self.trade_subs,
                                        &self.depth_subs,
                                        &mut self.processed_trade_ids,
                                        &mut self.processed_public_trade_ids,
                                        &mut self.asset_context_caches,
                                        &self.all_dex_asset_ctxs_instrument_ids,
                                        self.all_mids_data_types.as_slice(),
                                    );

                                    if !nautilus_msgs.is_empty() {
                                        let mut iter = nautilus_msgs.into_iter();
                                        let first = iter.next().unwrap();
                                        self.message_buffer.extend(iter);
                                        return Some(first);
                                    }
                                }
                                Err(e) => {
                                    context.log_decode_error(&text, &e);
                                    if serde_json::from_str::<WsMessageHeader<'_>>(&text)
                                        .is_ok_and(|header| header.channel == "l2")
                                    {
                                        self.invalidate_compact_books();
                                    }
                                }
                            }
                        }
                        Message::Ping(data) => {
                            if let Some(ref client) = self.client {
                                self.rate_limits.acquire_message().await;

                                if let Err(e) = client.send_pong(data.to_vec()).await {
                                    log::error!("Error sending pong: {e}");
                                }
                            }
                        }
                        Message::Close(_) => {
                            log::debug!("Received WebSocket close frame");
                            return None;
                        }
                        _ => {}
                    }
                }

                through = wait_for_candle_deadline(self.clock, candle_deadline), if candle_deadline.is_some() => {
                    // Drain only the frames already queued so an active feed cannot defer expiry indefinitely
                    self.candle_expiry = Some(CandleExpiry {
                        through,
                        frames_remaining: self.raw_rx.len(),
                    });
                }

                _ = self.heartbeat.tick() => {
                    if self.client.as_ref().is_some_and(WebSocketClient::is_active)
                        && let Err(e) = self.send_with_retry(
                            HEARTBEAT_MESSAGE.to_string(),
                        ).await
                    {
                        log::error!("Error sending WebSocket heartbeat: {e}");
                    }
                }

            }
        }
    }

    fn emit_expired_candles(&mut self) -> Option<NautilusWsMessage> {
        let CandleExpiry {
            through,
            frames_remaining: 0,
        } = self.candle_expiry?
        else {
            return None;
        };

        self.candle_expiry = None;
        let ts_init = self.clock.get_time_ns();
        self.candles.expire(
            through,
            &self.instruments,
            ts_init,
            &mut self.message_buffer,
        );
        self.message_buffer.pop_front()
    }

    fn prepare_compact_request(&mut self, request: &HyperliquidWsRequest) -> Option<Ustr> {
        let (subscription, subscribe) = match request {
            HyperliquidWsRequest::Subscribe { subscription } => (subscription, true),
            HyperliquidWsRequest::Unsubscribe { subscription } => (subscription, false),
            _ => return None,
        };
        let SubscriptionRequest::L2 { coin, .. } = subscription else {
            return None;
        };
        let stream = self.compact_books.entry(*coin).or_default();
        // Retire outputs already queued before this physical stream is replaced or removed
        stream.output_gate.lock().close();
        stream.output_gate = SnapshotGate::default();
        stream.book = None;
        stream.confirmed = false;
        stream.desired = subscribe;

        if subscribe {
            stream.generation = stream.generation.wrapping_add(1);
        }

        stream.pending.push_back((subscribe, stream.generation));
        Some(*coin)
    }

    fn compact_write_failed(&mut self, coin: Option<Ustr>) {
        if let Some(coin) = coin
            && let Some(stream) = self.compact_books.get_mut(&coin)
        {
            stream.pending.pop_back();
        }
    }

    fn confirm_compact_request(&mut self, subscription: &SubscriptionRequest, method: &str) {
        let SubscriptionRequest::L2 { coin, .. } = subscription else {
            return;
        };
        let Some(stream) = self.compact_books.get_mut(coin) else {
            return;
        };
        let subscribe = match method {
            "subscribe" => true,
            "unsubscribe" => false,
            _ => return,
        };
        let Some(&(expected, generation)) = stream.pending.front() else {
            return;
        };

        if subscribe != expected {
            return;
        }

        stream.pending.pop_front();

        // Old ACKs cannot admit frames into a replacement subscription's baseline
        if subscribe && generation == stream.generation && stream.desired {
            stream.confirmed = self.subscriptions.is_subscribed(&Ustr::from("l2"), coin);
        }

        if !stream.desired && stream.pending.is_empty() {
            self.compact_books.remove(coin);
        }
    }

    fn invalidate_compact_books(&mut self) {
        self.compact_books.clear();

        // A malformed compressed frame cannot reliably identify its affected coin
        if let Some(client) = &self.client {
            let _ = client.request_reconnect();
        }
    }

    fn handle_compact_book(
        &mut self,
        data: WsCompactBookData,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let (coin, is_snapshot) = match &data {
            WsCompactBookData::Snapshot(book) => (book.coin, true),
            WsCompactBookData::Update(update) => (update.coin, false),
        };
        let stream = self.compact_books.get_mut(&coin)?;

        if !stream.confirmed || !stream.desired {
            return None;
        }

        let instrument = self.instruments.get(&coin)?;

        let result = (|| -> anyhow::Result<Option<NautilusWsMessage>> {
            match data {
                WsCompactBookData::Snapshot(book) => stream.book = Some(CompactBook::new(book)),
                WsCompactBookData::Update(update) => {
                    let Some(book) = &mut stream.book else {
                        return Ok(None);
                    };
                    book.apply(update)?;
                }
            }

            let Some(book) = &stream.book else {
                return Ok(None);
            };
            let deltas = parse_ws_order_book_deltas(&book.data, instrument, ts_init)?;
            let depth = self
                .depth_subs
                .contains(&coin)
                .then(|| parse_ws_order_book_depth(&book.data, instrument, ts_init))
                .transpose()?
                .map(Box::new);
            Ok(Some(NautilusWsMessage::CompactBook {
                deltas,
                depth,
                is_snapshot,
                output_gate: stream.output_gate.clone(),
            }))
        })();

        match result {
            Ok(message) => message,
            Err(e) => {
                stream.book = None;
                stream.confirmed = false;
                log::warn!("Invalid compact book for {}: {e}", instrument.id());
                Some(NautilusWsMessage::BookInvalid {
                    instrument_id: instrument.id(),
                    output_gate: Some(stream.output_gate.clone()),
                })
            }
        }
    }

    async fn subscribe(&mut self, subscriptions: Vec<SubscriptionRequest>) {
        for subscription in subscriptions {
            let key = subscription_to_key(&subscription);
            self.subscriptions.mark_subscribe(&key);

            if let Err(e) = self
                .rate_limits
                .reserve_subscription(self.client_id, &subscription)
            {
                log::error!("Cannot subscribe to {key}: {e}");
                self.subscriptions.mark_unsubscribe(&key);
                self.subscriptions.confirm_unsubscribe(&key);
                continue;
            }

            self.all_mids_data_types.apply(&subscription, true);
            let request = HyperliquidWsRequest::Subscribe { subscription };
            match serde_json::to_string(&request) {
                Ok(payload) => {
                    log::debug!("Sending subscribe payload ({} bytes)", payload.len());
                    let compact = self.prepare_compact_request(&request);
                    if let Err(e) = self.send_with_retry(payload).await {
                        self.compact_write_failed(compact);
                        log::error!("Error subscribing to {key}: {e}");
                        self.subscriptions.mark_failure(&key);
                    }
                }
                Err(e) => {
                    log::error!("Error serializing subscription for {key}: {e}");
                    self.subscriptions.mark_failure(&key);
                }
            }
        }
    }

    async fn send_book_request(
        &mut self,
        request: &HyperliquidWsRequest,
        rate_keys: Option<&[Ustr]>,
        epoch: u64,
    ) -> Result<(), HyperliquidWsError> {
        let payload = serialize_request(request)?;
        let compact = self.prepare_compact_request(request);
        let result = match &self.client {
            Some(client) => client
                .send_text_on_connection(payload, rate_keys, epoch)
                .await
                .map_err(HyperliquidWsError::from),
            None => Err(HyperliquidWsError::ClientError(
                "No WebSocket client available".to_string(),
            )),
        };

        if result.is_err() {
            self.compact_write_failed(compact);
        }
        result
    }

    async fn write_book(
        &mut self,
        subscription: SubscriptionRequest,
        replace: bool,
    ) -> Result<(), HyperliquidWsError> {
        let epoch = self
            .client
            .as_ref()
            .ok_or_else(|| {
                HyperliquidWsError::ClientError("No WebSocket client available".to_string())
            })?
            .connection_epoch();

        let key = subscription_to_key(&subscription);
        let rate_key = self.rate_limits.message_key();
        let rate_keys = Some(std::slice::from_ref(&rate_key));

        if replace {
            // Keeps the stream desired for reconnect replay; its unsubscribe ack is then stale
            self.subscriptions.mark_failure(&key);

            let request = HyperliquidWsRequest::Unsubscribe {
                subscription: subscription.clone(),
            };
            self.send_book_request(&request, rate_keys, epoch).await?;
        } else {
            self.subscriptions.mark_subscribe(&key);

            if let Err(e) = self
                .rate_limits
                .reserve_subscription(self.client_id, &subscription)
            {
                self.subscriptions.mark_unsubscribe(&key);
                self.subscriptions.confirm_unsubscribe(&key);
                return Err(HyperliquidWsError::ClientError(e));
            }
        }

        let request = HyperliquidWsRequest::Subscribe { subscription };
        if let Err(e) = self.send_book_request(&request, rate_keys, epoch).await {
            self.subscriptions.mark_failure(&key);
            return Err(e);
        }

        Ok(())
    }

    async fn unsubscribe(&mut self, subscriptions: Vec<SubscriptionRequest>) {
        for subscription in subscriptions {
            let key = subscription_to_key(&subscription);
            self.subscriptions.mark_unsubscribe(&key);
            self.all_mids_data_types.apply(&subscription, false);

            let request = HyperliquidWsRequest::Unsubscribe { subscription };
            match serde_json::to_string(&request) {
                Ok(payload) => {
                    log::debug!("Sending unsubscribe payload ({} bytes)", payload.len());
                    let compact = self.prepare_compact_request(&request);
                    if let Err(e) = self.send_with_retry(payload).await {
                        self.compact_write_failed(compact);
                        log::error!("Error unsubscribing from {key}: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Error serializing unsubscription for {key}: {e}");
                }
            }
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn parse_to_nautilus_messages(
        msg: HyperliquidWsMessage,
        context: FrameContext,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        cloid_cache: &CloidCache,
        account_id: Option<AccountId>,
        ts_init: UnixNanos,
        asset_context_subs: &AHashMap<Ustr, AHashSet<AssetContextDataType>>,
        trade_subs: &AHashMap<Ustr, TradeStreamUses>,
        depth_subs: &AHashSet<Ustr>,
        processed_trade_ids: &mut FifoCache<u64, 10_000>,
        processed_public_trade_ids: &mut FifoCache<(Ustr, u64), 10_000>,
        asset_context_caches: &mut AssetContextCaches,
        all_dex_asset_ctxs_instrument_ids: &AHashMap<Ustr, Vec<Option<InstrumentId>>>,
        all_mids_data_types: &[DataType],
    ) -> Vec<NautilusWsMessage> {
        let mut result = Vec::new();

        match msg {
            HyperliquidWsMessage::ClearinghouseState { data } => {
                result.push(NautilusWsMessage::ClearinghouseState(
                    data.clearinghouse_state,
                ));
            }
            HyperliquidWsMessage::SpotState { data } => {
                result.push(NautilusWsMessage::SpotState(data.spot_state));
            }
            HyperliquidWsMessage::OrderUpdates { data } => {
                if let Some(account_id) = account_id
                    && let Some(msg) = Self::handle_order_updates(
                        &data,
                        context,
                        instruments,
                        cloid_cache,
                        account_id,
                        ts_init,
                    )
                {
                    result.push(msg);
                }
            }
            HyperliquidWsMessage::UserEvents { data } | HyperliquidWsMessage::User { data } => {
                // Process fills from userEvents channel (userFills channel is redundant)
                match data {
                    WsUserEventData::Fills { fills } => {
                        log::debug!("Received {} fill(s) from userEvents channel", fills.len());

                        if let Some(account_id) = account_id {
                            if let Some(msg) = Self::handle_user_fills(
                                &fills,
                                context,
                                instruments,
                                cloid_cache,
                                account_id,
                                ts_init,
                                processed_trade_ids,
                            ) {
                                log::debug!("Successfully created fill message");
                                result.push(msg);
                            } else {
                                log::debug!("handle_user_fills returned None (no new fills)");
                            }
                        } else {
                            log::warn!("Cannot process fills: account_id is None");
                        }
                    }
                    WsUserEventData::Liquidation { liquidation } => {
                        log::warn!(
                            "Liquidation event: lid={}, liquidator={}, liquidated_user={}, ntl_pos={}, account_value={}",
                            liquidation.lid,
                            liquidation.liquidator,
                            liquidation.liquidated_user,
                            liquidation.liquidated_ntl_pos,
                            liquidation.liquidated_account_value,
                        );
                    }
                    _ => {
                        log::debug!("Received non-fill user event: {data:?}");
                    }
                }
            }
            HyperliquidWsMessage::UserFills { data } => {
                // UserFills channel is redundant with userEvents, but handle it for
                // backwards compatibility if explicitly subscribed
                if let Some(account_id) = account_id
                    && let Some(msg) = Self::handle_user_fills(
                        &data.fills,
                        context,
                        instruments,
                        cloid_cache,
                        account_id,
                        ts_init,
                        processed_trade_ids,
                    )
                {
                    result.push(msg);
                }
            }
            HyperliquidWsMessage::Trades { data } => {
                result.extend(Self::handle_trades(
                    &data,
                    instruments,
                    trade_subs,
                    processed_public_trade_ids,
                    ts_init,
                ));
            }
            HyperliquidWsMessage::AllMids { data } => {
                let mut mids = std::collections::HashMap::with_capacity(
                    data.mids.len().min(instruments.len()),
                );

                for (coin, mid_str) in &data.mids {
                    if let Some(instrument) = instruments.get(coin) {
                        match mid_str.parse::<Price>() {
                            Ok(price) => {
                                mids.insert(instrument.id(), price);
                            }
                            Err(e) => {
                                log::warn!("Failed to parse mid price for {coin}: {e}");
                            }
                        }
                    } else {
                        log::debug!("No instrument found for coin: {coin}");
                    }
                }

                if !mids.is_empty() {
                    // Take instead of clone on the last subscriber
                    let last_idx = all_mids_data_types.len().saturating_sub(1);
                    for (i, data_type) in all_mids_data_types.iter().enumerate() {
                        let mids_for_this = if i == last_idx {
                            std::mem::take(&mut mids)
                        } else {
                            mids.clone()
                        };
                        let all_mids = HyperliquidAllMids::new(mids_for_this, ts_init, ts_init);
                        result.push(NautilusWsMessage::CustomData(Data::Custom(
                            CustomData::new(Arc::new(all_mids), data_type.clone()),
                        )));
                    }
                }
            }
            HyperliquidWsMessage::AllDexsAssetCtxs { data } => {
                if let Some(msg) = Self::handle_all_dexs_asset_ctxs(
                    data,
                    all_dex_asset_ctxs_instrument_ids,
                    ts_init,
                ) {
                    result.push(msg);
                }
            }
            HyperliquidWsMessage::Bbo { data } => {
                if let Some(msg) = Self::handle_bbo(&data, instruments, ts_init) {
                    result.push(msg);
                }
            }
            HyperliquidWsMessage::L2Book { data } => {
                result.extend(Self::handle_l2_book(
                    &data,
                    instruments,
                    depth_subs,
                    ts_init,
                ));
            }
            HyperliquidWsMessage::ActiveAssetCtx { data }
            | HyperliquidWsMessage::ActiveSpotAssetCtx { data } => {
                result.extend(Self::handle_asset_context(
                    &data,
                    instruments,
                    asset_context_subs,
                    asset_context_caches,
                    ts_init,
                ));
            }
            HyperliquidWsMessage::UserTwapHistory { data } => {
                result.extend(Self::handle_user_twap_history(
                    &data,
                    context,
                    instruments,
                    ts_init,
                ));
            }
            HyperliquidWsMessage::UserTwapSliceFills { data } => {
                result.extend(Self::handle_user_twap_slice_fills(
                    &data,
                    context,
                    instruments,
                    ts_init,
                ));
            }
            HyperliquidWsMessage::Error { data } => {
                log::warn!("Received error from Hyperliquid WebSocket: {data}");
            }
            // Ignore other message types (subscription confirmations, etc)
            _ => {}
        }

        result
    }

    fn handle_order_updates(
        data: &[super::messages::WsOrderData],
        context: FrameContext,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        cloid_cache: &CloidCache,
        account_id: AccountId,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut exec_reports = Vec::new();

        for order_update in data {
            let instrument = instruments.get(&order_update.order.coin);

            if let Some(instrument) = instrument {
                match parse_ws_order_status_report(order_update, instrument, account_id, ts_init) {
                    Ok(mut report) => {
                        // Resolve cloid to real client_order_id if cached
                        if let Some(cloid) = &order_update.order.cloid {
                            let cloid_ustr = Ustr::from(cloid.as_str());
                            let resolved = cloid_cache.lock().get(&cloid_ustr).copied();

                            if let Some(real_client_order_id) = resolved {
                                log::debug!("Resolved cloid {cloid} -> {real_client_order_id}");
                                report.client_order_id = Some(real_client_order_id);
                            }
                        }
                        exec_reports.push(ExecutionReport::Order(report));
                    }
                    Err(_) => {
                        context.log_conversion_error("order_update", log::Level::Error);
                    }
                }
            } else {
                context.log_conversion_error("order_instrument", log::Level::Debug);
            }
        }

        if exec_reports.is_empty() {
            None
        } else {
            Some(NautilusWsMessage::ExecutionReports(exec_reports))
        }
    }

    fn handle_user_fills(
        fills: &[super::messages::WsFillData],
        context: FrameContext,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        cloid_cache: &CloidCache,
        account_id: AccountId,
        ts_init: UnixNanos,
        processed_trade_ids: &mut FifoCache<u64, 10_000>,
    ) -> Option<NautilusWsMessage> {
        let mut exec_reports = Vec::new();

        for fill in fills {
            if processed_trade_ids.contains(&fill.tid) {
                log::debug!("Skipping duplicate fill: tid={}", fill.tid);
                continue;
            }

            let instrument = instruments.get(&fill.coin);
            if let Some(instrument) = instrument {
                match parse_ws_fill_report(fill, instrument, account_id, ts_init) {
                    Ok(mut report) => {
                        // Mark processed only after successful parse
                        processed_trade_ids.add(fill.tid);

                        if let Some(cloid) = &fill.cloid {
                            let cloid_ustr = Ustr::from(cloid.as_str());
                            let resolved = cloid_cache.lock().get(&cloid_ustr).copied();

                            if let Some(real_client_order_id) = resolved {
                                log::debug!(
                                    "Resolved fill cloid {cloid} -> {real_client_order_id}"
                                );
                                report.client_order_id = Some(real_client_order_id);
                            }
                        }
                        log::debug!(
                            "Parsed fill report: venue_order_id={:?}, trade_id={:?}",
                            report.venue_order_id,
                            report.trade_id
                        );
                        exec_reports.push(ExecutionReport::Fill(report));
                    }
                    Err(_) => {
                        context.log_conversion_error("fill", log::Level::Error);
                    }
                }
            } else {
                // Not marked as processed so fill is retried if instrument loads later
                context.log_conversion_error("fill_instrument", log::Level::Warn);
            }
        }

        if exec_reports.is_empty() {
            None
        } else {
            Some(NautilusWsMessage::ExecutionReports(exec_reports))
        }
    }

    fn handle_trades(
        data: &[super::messages::WsTradeData],
        instruments: &AHashMap<Ustr, InstrumentAny>,
        trade_subs: &AHashMap<Ustr, TradeStreamUses>,
        processed_public_trade_ids: &mut FifoCache<(Ustr, u64), 10_000>,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut trade_ticks = Vec::new();
        let mut public_trades = Vec::new();

        for trade in data {
            if let Some(instrument) = instruments.get(&trade.coin) {
                let uses = trade_subs.get(&trade.coin).copied().unwrap_or_default();

                if uses.ticks {
                    match parse_ws_trade_tick(trade, instrument, ts_init) {
                        Ok(tick) => trade_ticks.push(tick),
                        Err(e) => {
                            log::error!("Error parsing trade tick: {e}");
                        }
                    }
                }

                if uses.public_trades {
                    let trade_key = (trade.coin, trade.tid);
                    if processed_public_trade_ids.contains(&trade_key) {
                        log::debug!(
                            "Skipping replayed public trade: coin={}, tid={}",
                            trade.coin,
                            trade.tid
                        );
                        continue;
                    }

                    match parse_ws_public_trade(trade, instrument, ts_init) {
                        Ok(trade) => {
                            processed_public_trade_ids.add(trade_key);
                            public_trades.push(trade);
                        }
                        Err(e) => {
                            log::error!("Error parsing public trade: {e}");
                        }
                    }
                }
            } else {
                log::debug!("No instrument found for coin: {}", trade.coin);
            }
        }

        let mut result = Vec::with_capacity(1 + public_trades.len());
        if !trade_ticks.is_empty() {
            result.push(NautilusWsMessage::Trades(trade_ticks));
        }
        result.extend(public_trades.into_iter().map(|trade| {
            let instrument_id = trade.instrument_id;
            NautilusWsMessage::CustomData(Data::Custom(CustomData::new(
                Arc::new(trade),
                Self::public_trade_data_type(instrument_id),
            )))
        }));
        result
    }

    fn handle_bbo(
        data: &super::messages::WsBboData,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        if let Some(instrument) = instruments.get(&data.coin) {
            match parse_ws_quote_tick(data, instrument, ts_init) {
                Ok(quote_tick) => Some(NautilusWsMessage::Quote(quote_tick)),
                Err(e) => {
                    log::error!("Error parsing quote tick: {e}");
                    None
                }
            }
        } else {
            log::debug!("No instrument found for coin: {}", data.coin);
            None
        }
    }

    fn handle_l2_book(
        data: &super::messages::WsBookData,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        depth_subs: &AHashSet<Ustr>,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut out = Vec::new();

        let Some(instrument) = instruments.get(&data.coin) else {
            log::debug!("No instrument found for coin: {}", data.coin);
            return out;
        };

        match parse_ws_order_book_deltas(data, instrument, ts_init) {
            Ok(deltas) => out.push(NautilusWsMessage::Deltas(deltas)),
            Err(e) => {
                log::error!("Error parsing order book deltas: {e}");
                out.push(NautilusWsMessage::BookInvalid {
                    instrument_id: instrument.id(),
                    output_gate: None,
                });
            }
        }

        if depth_subs.contains(&data.coin) {
            match parse_ws_order_book_depth(data, instrument, ts_init) {
                Ok(depth) => out.push(NautilusWsMessage::Depth(Box::new(depth))),
                Err(e) => log::error!("Error parsing order book depth: {e}"),
            }
        }

        out
    }

    fn handle_asset_context(
        data: &WsActiveAssetCtxData,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        asset_context_subs: &AHashMap<Ustr, AHashSet<AssetContextDataType>>,
        asset_context_caches: &mut AssetContextCaches,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut result = Vec::new();

        let coin = match data {
            WsActiveAssetCtxData::Perp { coin, .. } => coin,
            WsActiveAssetCtxData::Spot { coin, .. } => coin,
        };

        if let Some(instrument) = instruments.get(coin) {
            let (mark_px, oracle_px, funding, open_interest) = match data {
                WsActiveAssetCtxData::Perp { ctx, .. } => (
                    &ctx.shared.mark_px,
                    Some(&ctx.oracle_px),
                    Some(&ctx.funding),
                    Some(&ctx.open_interest),
                ),
                WsActiveAssetCtxData::Spot { ctx, .. } => (&ctx.shared.mark_px, None, None, None),
            };

            let mark_changed = asset_context_caches.mark_price.get(coin) != Some(mark_px);
            let index_changed =
                oracle_px.is_some_and(|px| asset_context_caches.index_price.get(coin) != Some(px));
            let funding_changed = funding
                .is_some_and(|rate| asset_context_caches.funding_rate.get(coin) != Some(rate));
            let open_interest_changed = open_interest
                .is_some_and(|value| asset_context_caches.open_interest.get(coin) != Some(value));

            let subscribed_types = asset_context_subs.get(coin);

            if mark_changed || index_changed || funding_changed {
                match parse_ws_asset_context(data, instrument, ts_init) {
                    Ok((mark_price, index_price, funding_rate)) => {
                        if mark_changed
                            && subscribed_types
                                .is_some_and(|s| s.contains(&AssetContextDataType::MarkPrice))
                        {
                            asset_context_caches.mark_price.insert(*coin, *mark_px);
                            result.push(NautilusWsMessage::MarkPrice(mark_price));
                        }

                        if index_changed
                            && subscribed_types
                                .is_some_and(|s| s.contains(&AssetContextDataType::IndexPrice))
                        {
                            if let Some(px) = oracle_px {
                                asset_context_caches.index_price.insert(*coin, *px);
                            }

                            if let Some(index) = index_price {
                                result.push(NautilusWsMessage::IndexPrice(index));
                            }
                        }

                        if funding_changed
                            && subscribed_types
                                .is_some_and(|s| s.contains(&AssetContextDataType::FundingRate))
                        {
                            if let Some(rate) = funding {
                                asset_context_caches.funding_rate.insert(*coin, *rate);
                            }

                            if let Some(funding) = funding_rate {
                                result.push(NautilusWsMessage::FundingRate(funding));
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Error parsing asset context: {e}");
                    }
                }
            }

            if let Some(value) = open_interest
                && open_interest_changed
                && subscribed_types.is_some_and(|s| s.contains(&AssetContextDataType::OpenInterest))
            {
                match parse_ws_open_interest(*value, instrument, ts_init) {
                    Ok(open_interest_data) => {
                        asset_context_caches.open_interest.insert(*coin, *value);

                        let data_type =
                            Self::open_interest_data_type(open_interest_data.instrument_id);
                        result.push(NautilusWsMessage::CustomData(Data::Custom(
                            CustomData::new(Arc::new(open_interest_data), data_type),
                        )));
                    }
                    Err(e) => {
                        log::error!("Error parsing open interest: {e}");
                    }
                }
            }
        } else {
            log::debug!("No instrument found for coin: {coin}");
        }

        result
    }

    fn handle_all_dexs_asset_ctxs(
        data: WsAllDexsAssetCtxsData,
        all_dex_asset_ctxs_instrument_ids: &AHashMap<Ustr, Vec<Option<InstrumentId>>>,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut entries = Vec::new();

        for (dex, ctxs) in data.ctxs {
            let dex_key = Ustr::from(dex.as_str());
            let Some(instrument_ids) = all_dex_asset_ctxs_instrument_ids.get(&dex_key) else {
                log::warn!("Missing Hyperliquid allDexsAssetCtxs mapping for dex='{dex}'");
                continue;
            };

            if ctxs.len() != instrument_ids.len() {
                // Mapping is rebuilt on each instrument refresh, instrument request, and data
                // client connect, so a count change means the universe drifted since the last
                // build and positional alignment can no longer be trusted until the next one.
                log::warn!(
                    "Hyperliquid allDexsAssetCtxs count mismatch for dex='{dex}': received {} contexts but cached {} instrument IDs (the next instrument refresh, instrument request, or data client connect rebuilds the mapping)",
                    ctxs.len(),
                    instrument_ids.len()
                );
            }

            for (index, ctx) in ctxs.into_iter().enumerate() {
                let Some(Some(instrument_id)) = instrument_ids.get(index).copied() else {
                    log::warn!(
                        "Missing Hyperliquid allDexsAssetCtxs instrument mapping for dex='{dex}' index={index}"
                    );
                    continue;
                };

                match Self::normalize_all_dex_asset_ctx_entry(&dex, instrument_id, ctx) {
                    Ok(entry) => entries.push(entry),
                    Err(e) => {
                        log::warn!(
                            "Failed to normalize Hyperliquid allDexsAssetCtxs entry dex='{dex}' index={index}: {e}"
                        );
                    }
                }
            }
        }

        if entries.is_empty() {
            return None;
        }

        let payload = HyperliquidAllDexsAssetCtxs::new(entries, ts_init, ts_init);
        let data_type = DataType::new("HyperliquidAllDexsAssetCtxs", None, None);
        Some(NautilusWsMessage::CustomData(Data::Custom(
            CustomData::new(Arc::new(payload), data_type),
        )))
    }

    fn normalize_all_dex_asset_ctx_entry(
        dex: &str,
        instrument_id: InstrumentId,
        ctx: super::messages::PerpsAssetCtx,
    ) -> anyhow::Result<HyperliquidDexAssetCtx> {
        let mark_price = Price::from_decimal(ctx.shared.mark_px).map_err(anyhow::Error::msg)?;
        let oracle_price = Price::from_decimal(ctx.oracle_px).map_err(anyhow::Error::msg)?;
        let prev_day_price =
            Price::from_decimal(ctx.shared.prev_day_px).map_err(anyhow::Error::msg)?;
        let mid_price = ctx
            .shared
            .mid_px
            .map(|value| Price::from_decimal(value).map_err(anyhow::Error::msg))
            .transpose()?;
        let funding_rate = ctx.funding;
        let open_interest = ctx.open_interest;
        let premium = ctx.premium;
        let day_ntl_volume = ctx.shared.day_ntl_vlm;
        let day_base_volume = ctx
            .shared
            .day_base_vlm
            .ok_or_else(|| anyhow::anyhow!("missing dayBaseVlm"))?;
        let impact_prices = match ctx.shared.impact_pxs {
            Some(values) => match values.as_slice() {
                [bid, ask] => Some(HyperliquidImpactPrices {
                    bid: Price::from_decimal(*bid).map_err(anyhow::Error::msg)?,
                    ask: Price::from_decimal(*ask).map_err(anyhow::Error::msg)?,
                }),
                other => {
                    anyhow::bail!("expected 2 impact prices, received {}", other.len());
                }
            },
            None => None,
        };

        Ok(HyperliquidDexAssetCtx {
            dex: dex.to_string(),
            instrument_id,
            mark_price,
            oracle_price,
            prev_day_price,
            mid_price,
            impact_prices,
            funding_rate,
            open_interest,
            premium,
            day_ntl_volume,
            day_base_volume,
        })
    }

    fn open_interest_data_type(instrument_id: InstrumentId) -> DataType {
        let metadata = Params::new(
            [(
                "instrument_id".to_string(),
                serde_json::Value::String(instrument_id.to_string()),
            )]
            .into(),
        );
        DataType::new(
            "HyperliquidOpenInterest",
            Some(metadata),
            Some(instrument_id.to_string()),
        )
    }

    fn public_trade_data_type(instrument_id: InstrumentId) -> DataType {
        let metadata = Params::new(
            [(
                "instrument_id".to_string(),
                serde_json::Value::String(instrument_id.to_string()),
            )]
            .into(),
        );
        DataType::new(
            "HyperliquidPublicTrade",
            Some(metadata),
            Some(instrument_id.to_string()),
        )
    }

    fn handle_user_twap_history(
        data: &super::messages::WsUserTwapHistoryData,
        context: FrameContext,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let is_snapshot = data.is_snapshot.unwrap_or(false);
        let mut result = Vec::with_capacity(data.history.len());

        for row in &data.history {
            let instrument = instruments.get(&row.state.coin);
            match parse_ws_twap_history_row(row, &data.user, is_snapshot, instrument, ts_init) {
                Ok(payload) => {
                    let user = payload.user.clone();
                    result.push(NautilusWsMessage::CustomData(Data::Custom(
                        CustomData::new(Arc::new(payload), Self::twap_history_data_type(&user)),
                    )));
                }
                Err(_) => {
                    context.log_conversion_error("twap_history", log::Level::Error);
                }
            }
        }

        result
    }

    fn handle_user_twap_slice_fills(
        data: &super::messages::WsUserTwapSliceFillsData,
        context: FrameContext,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let is_snapshot = data.is_snapshot.unwrap_or(false);
        let mut result = Vec::with_capacity(data.twap_slice_fills.len());

        for item in &data.twap_slice_fills {
            let instrument = instruments.get(&item.fill.coin);
            match parse_ws_twap_slice_fill(item, &data.user, is_snapshot, instrument, ts_init) {
                Ok(payload) => {
                    let user = payload.user.clone();
                    result.push(NautilusWsMessage::CustomData(Data::Custom(
                        CustomData::new(Arc::new(payload), Self::twap_slice_fill_data_type(&user)),
                    )));
                }
                Err(_) => {
                    context.log_conversion_error("twap_slice_fill", log::Level::Error);
                }
            }
        }

        result
    }

    fn twap_history_data_type(user: &str) -> DataType {
        let metadata = Params::new(
            [(
                "user".to_string(),
                serde_json::Value::String(user.to_string()),
            )]
            .into(),
        );
        DataType::new(
            "HyperliquidTwapHistory",
            Some(metadata),
            Some(user.to_string()),
        )
    }

    fn twap_slice_fill_data_type(user: &str) -> DataType {
        let metadata = Params::new(
            [(
                "user".to_string(),
                serde_json::Value::String(user.to_string()),
            )]
            .into(),
        );
        DataType::new(
            "HyperliquidTwapSliceFill",
            Some(metadata),
            Some(user.to_string()),
        )
    }
}

#[derive(Clone, Copy)]
struct CandleExpiry {
    through: UnixNanos,
    frames_remaining: usize,
}

#[derive(Default)]
struct CandleStreams {
    streams: AHashMap<String, CandleStream>,
    deadlines: BTreeMap<UnixNanos, AHashSet<String>>,
}

impl CandleStreams {
    fn register(&mut self, key: String, bar_type: BarType) {
        self.streams
            .entry(key)
            .and_modify(|stream| stream.bar_type = bar_type)
            .or_insert(CandleStream {
                bar_type,
                pending: None,
                last_emitted: None,
            });
    }

    fn unregister(&mut self, key: &str) {
        if let Some(stream) = self.streams.remove(key)
            && let Some(pending) = stream.pending
        {
            Self::remove_deadline(&mut self.deadlines, key, pending.deadline);
        }
    }

    fn deadline(&self) -> Option<UnixNanos> {
        self.deadlines
            .first_key_value()
            .map(|(&deadline, _)| deadline)
    }

    fn observe(
        &mut self,
        data: CandleData,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        ts_init: UnixNanos,
    ) -> Option<Bar> {
        let key = format!("candle:{}:{}", data.s, data.i);
        let stream = self.streams.get_mut(&key)?;
        let instrument = instruments.get(&data.s)?;

        if stream
            .last_emitted
            .is_some_and(|close| data.close_time <= close)
        {
            return None;
        }

        if let Some(pending) = &mut stream.pending {
            match data.close_time.cmp(&pending.data.close_time) {
                std::cmp::Ordering::Less => return None,
                std::cmp::Ordering::Equal => {
                    pending.data = data;
                    return None;
                }
                std::cmp::Ordering::Greater => {}
            }
        }

        // Hyperliquid reports the interval's last millisecond inclusively
        let Some(deadline) = data
            .close_time
            .checked_add(1 + CANDLE_COMPLETION_GRACE_MS)
            .and_then(|millis| millis_to_nanos(millis).ok())
        else {
            log::warn!(
                "Candle deadline is out of range for {key}: {}",
                data.close_time
            );
            return None;
        };

        let previous = stream.pending.replace(PendingCandle { data, deadline });
        if let Some(previous) = &previous {
            Self::remove_deadline(&mut self.deadlines, &key, previous.deadline);
        }

        self.deadlines.entry(deadline).or_default().insert(key);

        previous.and_then(|pending| stream.emit(&pending.data, instrument, ts_init))
    }

    fn expire(
        &mut self,
        through: UnixNanos,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        ts_init: UnixNanos,
        output: &mut VecDeque<NautilusWsMessage>,
    ) {
        while let Some(entry) = self.deadlines.first_entry()
            && *entry.key() <= through
        {
            for key in entry.remove() {
                if let Some(stream) = self.streams.get_mut(&key)
                    && let Some(pending) = stream.pending.take()
                    && let Some(instrument) = instruments.get(&pending.data.s)
                    && let Some(bar) = stream.emit(&pending.data, instrument, ts_init)
                {
                    output.push_back(NautilusWsMessage::Candle(bar));
                }
            }
        }
    }

    fn remove_deadline(
        deadlines: &mut BTreeMap<UnixNanos, AHashSet<String>>,
        key: &str,
        deadline: UnixNanos,
    ) {
        if let Entry::Occupied(mut entry) = deadlines.entry(deadline) {
            entry.get_mut().remove(key);
            if entry.get().is_empty() {
                entry.remove();
            }
        }
    }
}

struct CandleStream {
    bar_type: BarType,
    pending: Option<PendingCandle>,
    last_emitted: Option<u64>,
}

impl CandleStream {
    fn emit(
        &mut self,
        data: &CandleData,
        instrument: &InstrumentAny,
        ts_init: UnixNanos,
    ) -> Option<Bar> {
        match parse_ws_candle(data, instrument, &self.bar_type, ts_init) {
            Ok(bar) => {
                self.last_emitted = Some(data.close_time);
                Some(bar)
            }
            Err(e) => {
                log::error!("Error parsing closed candle: {e}");
                None
            }
        }
    }
}

struct PendingCandle {
    data: CandleData,
    deadline: UnixNanos,
}

async fn wait_for_candle_deadline(clock: &AtomicTime, deadline: Option<UnixNanos>) -> UnixNanos {
    let Some(deadline) = deadline else {
        return std::future::pending().await;
    };

    loop {
        let now = clock.get_time_ns();
        match deadline.duration_since(&now) {
            Some(delay) if !delay.is_zero() => tokio::time::sleep(delay.into()).await,
            _ => return now,
        }
    }
}

#[derive(Debug, Default)]
struct CompactBookStream {
    book: Option<CompactBook>,
    desired: bool,
    confirmed: bool,
    generation: u64,
    pending: VecDeque<(bool, u64)>,
    output_gate: SnapshotGate,
}

impl Drop for CompactBookStream {
    fn drop(&mut self) {
        self.output_gate.lock().close();
    }
}

#[derive(Clone, Copy)]
struct FrameContext {
    client_id: u64,
    payload_bytes: usize,
}

impl FrameContext {
    fn log_decode_error(self, text: &str, e: &serde_json::Error) {
        // Error Display can echo private values, including unknown tags and invalid fields
        let channel = serde_json::from_str::<FrameHeader>(text)
            .map_or("unknown", |header| header.channel.as_str());

        log::error!(
            "Error parsing WebSocket message: channel={channel}, client_id={}, payload_bytes={}, category={:?}, line={}, column={}",
            self.client_id,
            self.payload_bytes,
            e.classify(),
            e.line(),
            e.column(),
        );
    }

    fn log_conversion_error(self, class: &'static str, level: log::Level) {
        log::log!(
            level,
            "Error parsing WebSocket message: class={class}, client_id={}, payload_bytes={}, category=conversion",
            self.client_id,
            self.payload_bytes,
        );
    }
}

#[derive(Deserialize)]
struct FrameHeader {
    channel: HyperliquidWsChannel,
}

#[derive(Debug, thiserror::Error)]
enum PostSendError {
    #[error(transparent)]
    Transport(SendError),
    #[error(transparent)]
    Retry(RetryError),
    #[error("Post deadline expired")]
    Deadline,
}

async fn send_post_with_retry<F, Fut>(
    retry_manager: &RetryManager<PostSendError>,
    deadline: tokio::time::Instant,
    cancellation_token: &CancellationToken,
    send: F,
) -> Result<(), PostSendError>
where
    F: Fn() -> Fut + Clone,
    Fut: Future<Output = Result<(), PostSendError>>,
{
    let invocation = retry_manager
        .invocation(
            "websocket_post_send",
            || {
                let send = send.clone();
                async move { send_post_before_deadline(deadline, cancellation_token, send).await }
            },
            should_retry_post_send,
            PostSendError::Retry,
        )
        .cancellation_token(cancellation_token)
        .execute();
    tokio::pin!(invocation);

    tokio::select! {
        biased;
        () = tokio::time::sleep_until(deadline) => {
            Err(PostSendError::Deadline)
        }
        result = &mut invocation => result,
    }
}

async fn send_post_before_deadline<F, Fut>(
    deadline: tokio::time::Instant,
    cancellation_token: &CancellationToken,
    send: F,
) -> Result<(), PostSendError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), PostSendError>>,
{
    if cancellation_token.is_cancelled() {
        return Err(PostSendError::Retry(RetryError::Canceled));
    }

    if tokio::time::Instant::now() >= deadline {
        return Err(PostSendError::Deadline);
    }

    send().await
}

fn should_retry_post_send(error: &PostSendError) -> bool {
    matches!(
        error,
        PostSendError::Transport(SendError::Timeout | SendError::ConnectionChanged)
    )
}

fn subscription_response_key(
    sub: &SubscriptionRequest,
    subscriptions: &SubscriptionState,
) -> String {
    let key = subscription_to_key(sub);

    let SubscriptionRequest::SpotState { user, .. } = sub else {
        return key;
    };

    // Spot acknowledgements omit isPortfolioMargin; retain the tracked request's flag
    let prefix = format!("{}:{user}:", HyperliquidWsChannel::SpotState.as_str());
    subscriptions
        .all_topics()
        .into_iter()
        .find(|topic| topic.starts_with(&prefix))
        .unwrap_or(key)
}

pub(super) fn subscription_to_key(sub: &SubscriptionRequest) -> String {
    match sub {
        SubscriptionRequest::AllMids { dex } => {
            if let Some(dex_name) = dex {
                format!("{}:{dex_name}", HyperliquidWsChannel::AllMids.as_str())
            } else {
                HyperliquidWsChannel::AllMids.as_str().to_string()
            }
        }
        SubscriptionRequest::AllDexsAssetCtxs => {
            HyperliquidWsChannel::AllDexsAssetCtxs.as_str().to_string()
        }
        SubscriptionRequest::Notification { user } => {
            format!("{}:{user}", HyperliquidWsChannel::Notification.as_str())
        }
        SubscriptionRequest::WebData2 { user } => {
            format!("{}:{user}", HyperliquidWsChannel::WebData2.as_str())
        }
        SubscriptionRequest::ClearinghouseState { user } => {
            format!(
                "{}:{user}",
                HyperliquidWsChannel::ClearinghouseState.as_str()
            )
        }
        SubscriptionRequest::SpotState {
            user,
            is_portfolio_margin,
        } => {
            format!(
                "{}:{user}:{is_portfolio_margin}",
                HyperliquidWsChannel::SpotState.as_str()
            )
        }
        SubscriptionRequest::Candle { coin, interval } => {
            format!(
                "{}:{coin}:{}",
                HyperliquidWsChannel::Candle.as_str(),
                interval.as_str()
            )
        }
        SubscriptionRequest::L2Book { coin, .. } => {
            format!("{}:{coin}", HyperliquidWsChannel::L2Book.as_str())
        }
        SubscriptionRequest::L2 { coin, .. } => {
            format!("{}:{coin}", HyperliquidWsChannel::L2.as_str())
        }
        SubscriptionRequest::Trades { coin } => {
            format!("{}:{coin}", HyperliquidWsChannel::Trades.as_str())
        }
        SubscriptionRequest::OrderUpdates { user } => {
            format!("{}:{user}", HyperliquidWsChannel::OrderUpdates.as_str())
        }
        SubscriptionRequest::UserEvents { user } => {
            format!("{}:{user}", HyperliquidWsChannel::UserEvents.as_str())
        }
        SubscriptionRequest::UserFills { user, .. } => {
            format!("{}:{user}", HyperliquidWsChannel::UserFills.as_str())
        }
        SubscriptionRequest::UserFundings { user } => {
            format!("{}:{user}", HyperliquidWsChannel::UserFundings.as_str())
        }
        SubscriptionRequest::UserNonFundingLedgerUpdates { user } => {
            format!(
                "{}:{user}",
                HyperliquidWsChannel::UserNonFundingLedgerUpdates.as_str()
            )
        }
        SubscriptionRequest::ActiveAssetCtx { coin } => {
            format!("{}:{coin}", HyperliquidWsChannel::ActiveAssetCtx.as_str())
        }
        SubscriptionRequest::ActiveSpotAssetCtx { coin } => {
            format!(
                "{}:{coin}",
                HyperliquidWsChannel::ActiveSpotAssetCtx.as_str()
            )
        }
        SubscriptionRequest::ActiveAssetData { user, coin } => {
            format!(
                "{}:{user}:{coin}",
                HyperliquidWsChannel::ActiveAssetData.as_str()
            )
        }
        SubscriptionRequest::UserTwapSliceFills { user } => {
            format!(
                "{}:{user}",
                HyperliquidWsChannel::UserTwapSliceFills.as_str()
            )
        }
        SubscriptionRequest::UserTwapHistory { user } => {
            format!("{}:{user}", HyperliquidWsChannel::UserTwapHistory.as_str())
        }
        SubscriptionRequest::Bbo { coin } => {
            format!("{}:{coin}", HyperliquidWsChannel::Bbo.as_str())
        }
    }
}

/// Determines whether a Hyperliquid WebSocket error should trigger a retry.
pub(crate) fn should_retry_hyperliquid_error(error: &HyperliquidWsError) -> bool {
    match error {
        HyperliquidWsError::TungsteniteError(_) => true,
        HyperliquidWsError::ClientError(msg) => {
            let msg_lower = msg.to_lowercase();
            msg_lower.contains("timeout")
                || msg_lower.contains("timed out")
                || msg_lower.contains("connection")
                || msg_lower.contains("network")
        }
        _ => false,
    }
}

fn serialize_request(request: &HyperliquidWsRequest) -> Result<String, HyperliquidWsError> {
    serde_json::to_string(request)
        .map_err(|e| HyperliquidWsError::MessageSerialization(e.to_string()))
}

/// Creates a timeout error for Hyperliquid retry logic.
pub(crate) fn create_hyperliquid_timeout_error(msg: String) -> HyperliquidWsError {
    HyperliquidWsError::ClientError(msg)
}

#[cfg(test)]
pub(super) mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Arc, LazyLock, Once,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use ahash::{AHashMap, AHashSet};
    use log::{Level, LevelFilter, Log, Metadata, Record};
    use nautilus_common::cache::fifo::FifoCacheMap;
    use nautilus_core::{AtomicTime, nanos::UnixNanos};
    use nautilus_live::book::snapshot::SnapshotGate;
    use nautilus_model::{
        data::{Bar, BarType, Data},
        identifiers::{ClientOrderId, InstrumentId, Symbol},
        instruments::{CryptoPerpetual, Instrument, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use nautilus_network::{
        RECONNECTED,
        error::SendError,
        retry::{RetryConfig, RetryError, RetryManager},
        websocket::SubscriptionState,
    };
    use parking_lot::Mutex;
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use serde_json::json;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_util::sync::CancellationToken;
    use ustr::Ustr;

    use super::{
        super::{
            client::{AssetContextDataType, CLOID_CACHE_CAPACITY, CloidCache},
            error::HyperliquidWsError,
            messages::{
                CandleData, HyperliquidWsMessage, HyperliquidWsRequest, NautilusWsMessage,
                PerpsAssetCtx, PostRequest, SharedAssetCtx, SpotAssetCtx, SubscriptionRequest,
                WsActiveAssetCtxData, WsAllDexsAssetCtxsData, WsBookData, WsLevelData,
            },
            post::PostRouter,
            rate_limits::WebSocketRateLimits,
        },
        AllMidsDataTypeCache, AssetContextCaches, CandleExpiry, CandleStreams, FeedHandler,
        HandlerCommand, PostSendError, send_post_before_deadline, send_post_with_retry,
        should_retry_post_send,
    };
    use crate::{
        common::consts::{
            HYPERLIQUID_VENUE, HYPERLIQUID_WS_MESSAGES_PER_MINUTE, HYPERLIQUID_WS_SUBSCRIPTIONS_MAX,
        },
        data_types::{HyperliquidAllDexsAssetCtxs, HyperliquidOpenInterest},
    };

    const SECRET_MARKER: &str = "OUTBOUND_SECRET_MARKER";
    // Hyperliquid mainnet candleSnapshot data wrapped in the WebSocket candle envelope
    const CANDLE_FRAME: &str = include_str!("../../test_data/ws_candle.json");
    const CANDLE_KEY: &str = "candle:BTC:1m";
    const CANDLE_DEADLINE_NS: u64 = 1_795_656_902_000_000_000;

    static CANDLE_EXPIRED_CLOCK: LazyLock<AtomicTime> =
        LazyLock::new(|| AtomicTime::new(false, UnixNanos::from(CANDLE_DEADLINE_NS + 1)));

    struct LogCapture {
        messages: Mutex<Vec<String>>,
    }

    static LOG_CAPTURE: LogCapture = LogCapture {
        messages: Mutex::new(Vec::new()),
    };

    static LOG_INSTALL: Once = Once::new();
    static LOG_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    impl LogCapture {
        fn clear(&self) {
            self.messages.lock().clear();
        }

        fn messages(&self) -> Vec<String> {
            self.messages.lock().clone()
        }
    }

    impl Log for LogCapture {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() <= Level::Debug
                && metadata.target().starts_with("nautilus_hyperliquid::")
        }

        fn log(&self, record: &Record<'_>) {
            if self.enabled(record.metadata()) {
                let message = record.args().to_string();
                self.messages.lock().push(message);
            }
        }

        fn flush(&self) {}
    }

    #[rstest]
    fn all_mids_cache_projects_subscriptions_without_scanning_every_websocket_message() {
        let mut cache = AllMidsDataTypeCache::default();

        assert_eq!(cache.as_slice().len(), 1);
        assert!(cache.as_slice()[0].metadata().is_none());

        cache.apply(
            &SubscriptionRequest::AllMids {
                dex: Some("xyz".to_owned()),
            },
            true,
        );
        assert_eq!(cache.as_slice().len(), 1);
        assert_eq!(
            cache.as_slice()[0]
                .metadata()
                .and_then(|metadata| metadata.get_str("dex")),
            Some("xyz"),
        );

        cache.apply(&SubscriptionRequest::AllMids { dex: None }, true);
        assert_eq!(cache.as_slice().len(), 2);

        cache.apply(
            &SubscriptionRequest::AllMids {
                dex: Some("xyz".to_owned()),
            },
            false,
        );
        assert_eq!(cache.as_slice().len(), 1);
        assert_eq!(cache.as_slice()[0].type_name(), "HyperliquidAllMids");
        assert!(cache.as_slice()[0].metadata().is_none());

        cache.apply(&SubscriptionRequest::AllMids { dex: None }, false);
        assert_eq!(cache.as_slice().len(), 1);
        assert_eq!(cache.as_slice()[0].type_name(), "HyperliquidAllMids");
        assert!(cache.as_slice()[0].metadata().is_none());
    }

    fn btc_perp() -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(
            CryptoPerpetual::builder()
                .instrument_id(InstrumentId::new(
                    Symbol::new("BTC-PERP"),
                    *HYPERLIQUID_VENUE,
                ))
                .raw_symbol(Symbol::new("BTC-PERP"))
                .base_currency(Currency::from("BTC"))
                .quote_currency(Currency::from("USDC"))
                .settlement_currency(Currency::from("USDC"))
                .is_inverse(false)
                .price_precision(2)
                .size_precision(3)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn one_level_book() -> WsBookData {
        WsBookData {
            coin: Ustr::from("BTC"),
            levels: [
                vec![WsLevelData {
                    px: dec!(100.00),
                    sz: dec!(1.0),
                    n: 1,
                }],
                vec![WsLevelData {
                    px: dec!(100.01),
                    sz: dec!(1.0),
                    n: 1,
                }],
            ],
            time: 1_700_000_000_000,
        }
    }

    fn compact_handler() -> FeedHandler {
        let (_, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let mut handler = FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            SubscriptionState::new(':'),
            Arc::new(Mutex::new(FifoCacheMap::new())),
            PostRouter::new(),
            Arc::new(WebSocketRateLimits::new()),
            1,
        );
        handler.instruments.insert(Ustr::from("BTC"), btc_perp());
        handler
    }

    #[rstest]
    fn candle_expiry_publishes_latest_revision_once_at_deadline() {
        let (mut candles, instruments, first) = candle_stream();

        let latest = CandleData {
            c: dec!(60704.0),
            v: dec!(11.456),
            ..first
        };

        let before = UnixNanos::from(CANDLE_DEADLINE_NS - 1);
        let deadline = UnixNanos::from(CANDLE_DEADLINE_NS);
        let emitted_at = UnixNanos::from(CANDLE_DEADLINE_NS + 10);
        let mut output = VecDeque::new();

        assert!(candles.observe(first, &instruments, before).is_none());
        assert!(
            candles
                .observe(latest.clone(), &instruments, before)
                .is_none()
        );
        assert_eq!(candles.deadline(), Some(deadline));
        candles.expire(before, &instruments, before, &mut output);
        assert!(output.is_empty());

        candles.expire(deadline, &instruments, emitted_at, &mut output);

        let Some(NautilusWsMessage::Candle(bar)) = output.pop_front() else {
            panic!("expected a candle at its deadline");
        };

        assert_eq!(bar, expected_candle("60704.0", "11.456", emitted_at));

        assert!(candles.observe(latest, &instruments, emitted_at).is_none());
        assert_eq!(candles.deadline(), None);
        candles.expire(emitted_at, &instruments, emitted_at, &mut output);
        assert!(output.is_empty());
    }

    #[rstest]
    fn candle_rollover_ignores_older_updates_and_disarms_previous_deadline() {
        let (mut candles, instruments, first) = candle_stream();

        let older = CandleData {
            t: first.t - 60_000,
            close_time: first.close_time - 60_000,
            ..first
        };

        let next = CandleData {
            t: first.t + 60_000,
            close_time: first.close_time + 60_000,
            ..first
        };

        let rollover_at = UnixNanos::from(1_795_656_900_500_000_000);
        let mut output = VecDeque::new();

        assert!(
            candles
                .observe(first.clone(), &instruments, rollover_at)
                .is_none()
        );
        assert!(candles.observe(older, &instruments, rollover_at).is_none());

        let bar = candles.observe(next, &instruments, rollover_at).unwrap();
        assert_eq!(bar, expected_candle("60702.0", "23.642", rollover_at));
        assert!(candles.observe(first, &instruments, rollover_at).is_none());
        let first_deadline = UnixNanos::from(CANDLE_DEADLINE_NS);
        candles.expire(first_deadline, &instruments, first_deadline, &mut output);
        assert!(output.is_empty());
        assert_eq!(
            candles.deadline(),
            Some(UnixNanos::from(1_795_656_962_000_000_000))
        );
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn candle_expiry_consumes_failed_conversion(#[case] missing_instrument: bool) {
        let (mut candles, mut instruments, mut candle) = candle_stream();
        if !missing_instrument {
            candle.t = u64::MAX;
        }

        let deadline = UnixNanos::from(CANDLE_DEADLINE_NS);
        assert!(candles.observe(candle, &instruments, deadline).is_none());

        if missing_instrument {
            instruments.clear();
        }

        let mut output = VecDeque::new();

        candles.expire(deadline, &instruments, deadline, &mut output);

        assert_eq!(candles.deadline(), None);
        assert!(output.is_empty());
        assert!(candles.streams[CANDLE_KEY].pending.is_none());
    }

    #[rstest]
    fn candle_expiry_discards_invalid_ohlc_and_accepts_a_later_revision() {
        let (mut candles, instruments, valid) = candle_stream();

        let invalid = CandleData {
            c: dec!(60730.0),
            ..valid
        };

        let deadline = UnixNanos::from(CANDLE_DEADLINE_NS);
        let mut output = VecDeque::new();

        assert!(candles.observe(invalid, &instruments, deadline).is_none());
        candles.expire(deadline, &instruments, deadline, &mut output);
        assert!(output.is_empty());
        assert_eq!(candles.deadline(), None);

        assert!(candles.observe(valid, &instruments, deadline).is_none());
        candles.expire(deadline, &instruments, deadline, &mut output);

        let Some(NautilusWsMessage::Candle(bar)) = output.pop_front() else {
            panic!("expected a valid revision to publish after the malformed candle");
        };

        assert_eq!(bar, expected_candle("60702.0", "23.642", deadline));
        assert!(output.is_empty());
    }

    #[rstest]
    fn candle_unsubscribe_clears_pending_and_emitted_state() {
        let (mut candles, instruments, first) = candle_stream();

        let next = CandleData {
            t: first.t + 60_000,
            close_time: first.close_time + 60_000,
            ..first
        };

        let now = UnixNanos::from(CANDLE_DEADLINE_NS);
        assert!(candles.observe(first.clone(), &instruments, now).is_none());
        assert!(candles.observe(next.clone(), &instruments, now).is_some());

        candles.unregister(CANDLE_KEY);
        assert!(candles.observe(next, &instruments, now).is_none());
        assert_eq!(candles.deadline(), None);

        candles.register(CANDLE_KEY.to_string(), candle_bar_type());
        assert!(candles.observe(first, &instruments, now).is_none());
        assert_eq!(candles.deadline(), Some(now));
    }

    #[rstest]
    fn candle_expiry_publishes_all_intervals_at_shared_deadline() {
        let (mut candles, instruments, minute) = candle_stream();
        let five_min_type = BarType::from("BTC-PERP.HYPERLIQUID-5-MINUTE-LAST-EXTERNAL");
        candles.register("candle:BTC:5m".to_string(), five_min_type);

        let five_min = CandleData {
            t: minute.t - 240_000,
            i: Ustr::from("5m"),
            ..minute
        };

        let deadline = UnixNanos::from(CANDLE_DEADLINE_NS);
        let mut output = VecDeque::new();

        assert!(candles.observe(minute, &instruments, deadline).is_none());
        assert!(candles.observe(five_min, &instruments, deadline).is_none());
        candles.expire(deadline, &instruments, deadline, &mut output);

        let mut bars: Vec<_> = output
            .into_iter()
            .map(|message| {
                let NautilusWsMessage::Candle(bar) = message else {
                    panic!("expected a candle");
                };

                bar
            })
            .collect();

        bars.sort_by_key(|bar| bar.ts_event);
        let one_min_bar = expected_candle("60702.0", "23.642", deadline);

        let five_min_bar = Bar {
            bar_type: five_min_type,
            ts_event: UnixNanos::from(1_795_656_600_000_000_000),
            ..one_min_bar
        };

        assert_eq!(bars, [five_min_bar, one_min_bar]);
        assert_eq!(candles.deadline(), None);
    }

    #[rstest]
    #[case(u64::MAX)]
    #[case(u64::MAX / 1_000_000)]
    fn candle_deadline_rejects_overflow(#[case] close_time: u64) {
        let (mut candles, instruments, mut candle) = candle_stream();
        candle.close_time = close_time;
        let now = UnixNanos::from(CANDLE_DEADLINE_NS);

        assert!(candles.observe(candle, &instruments, now).is_none());
        assert_eq!(candles.deadline(), None);
    }

    #[rstest]
    #[tokio::test]
    async fn candle_handler_expires_after_queued_revisions_and_deduplicates_after_reconnect() {
        let mut handler = compact_handler();
        handler.clock = &CANDLE_EXPIRED_CLOCK;
        handler
            .candles
            .register(CANDLE_KEY.to_string(), candle_bar_type());
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.raw_rx = raw_rx;
        let mut revision: serde_json::Value = serde_json::from_str(CANDLE_FRAME).unwrap();
        revision["data"]["c"] = json!("60704.0");
        revision["data"]["v"] = json!("11.456");
        raw_tx.send(Message::Text(CANDLE_FRAME.into())).unwrap();
        raw_tx
            .send(Message::Text(revision.to_string().into()))
            .unwrap();

        let message = tokio::time::timeout(Duration::from_secs(1), handler.next())
            .await
            .unwrap();

        let Some(NautilusWsMessage::Candle(bar)) = message else {
            panic!("expected latest candle without a next-interval frame");
        };

        assert_eq!(
            bar,
            expected_candle("60704.0", "11.456", CANDLE_EXPIRED_CLOCK.get_time_ns())
        );

        for frame in [RECONNECTED, CANDLE_FRAME, RECONNECTED] {
            raw_tx.send(Message::Text(frame.into())).unwrap();
        }

        for _ in 0..2 {
            let message = tokio::time::timeout(Duration::from_secs(1), handler.next())
                .await
                .unwrap();
            assert!(matches!(message, Some(NautilusWsMessage::Reconnected)));
        }

        assert_eq!(handler.candles.deadline(), None);
    }

    #[rstest]
    #[tokio::test]
    async fn candle_handler_expires_before_processing_frames_beyond_the_snapshot() {
        let mut handler = compact_handler();
        handler.clock = &CANDLE_EXPIRED_CLOCK;
        let (candles, _, first) = candle_stream();
        handler.candles = candles;
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.raw_rx = raw_rx;
        let deadline = UnixNanos::from(CANDLE_DEADLINE_NS);
        assert!(
            handler
                .candles
                .observe(first, &handler.instruments, deadline)
                .is_none()
        );

        let mut revision: serde_json::Value = serde_json::from_str(CANDLE_FRAME).unwrap();
        revision["data"]["c"] = json!("60704.0");
        raw_tx
            .send(Message::Text(revision.to_string().into()))
            .unwrap();
        handler.candle_expiry = Some(CandleExpiry {
            through: deadline,
            frames_remaining: handler.raw_rx.len(),
        });

        revision["data"]["c"] = json!("60706.0");
        raw_tx
            .send(Message::Text(revision.to_string().into()))
            .unwrap();

        let message = tokio::time::timeout(Duration::from_secs(1), handler.next())
            .await
            .unwrap();

        let Some(NautilusWsMessage::Candle(bar)) = message else {
            panic!("expected a candle after the frames pending at the deadline");
        };

        assert_eq!(
            bar,
            expected_candle("60704.0", "23.642", CANDLE_EXPIRED_CLOCK.get_time_ns())
        );
        assert_eq!(handler.raw_rx.len(), 1);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn candle_handler_wakes_at_a_future_deadline_with_no_further_frames() {
        static CLOCK: LazyLock<AtomicTime> = LazyLock::new(|| {
            AtomicTime::new(false, UnixNanos::from(CANDLE_DEADLINE_NS - 2_000_000_000))
        });

        let mut handler = compact_handler();
        handler.clock = &CLOCK;
        handler
            .candles
            .register(CANDLE_KEY.to_string(), candle_bar_type());
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.raw_rx = raw_rx;
        raw_tx.send(Message::Text(CANDLE_FRAME.into())).unwrap();

        let wake_at = tokio::time::Instant::now() + Duration::from_secs(2);

        let message = {
            let next = handler.next();
            tokio::pin!(next);
            assert!(futures_util::poll!(next.as_mut()).is_pending());

            CLOCK.set_time(UnixNanos::from(CANDLE_DEADLINE_NS - 1_000_000));
            tokio::time::advance(Duration::from_millis(1_999)).await;
            assert!(futures_util::poll!(next.as_mut()).is_pending());

            CLOCK.set_time(UnixNanos::from(CANDLE_DEADLINE_NS));
            tokio::time::advance(Duration::from_millis(1)).await;
            tokio::time::timeout(Duration::from_secs(1), next)
                .await
                .unwrap()
        };

        let timer_tick = Duration::from_millis(1);
        assert!((wake_at..=wake_at + timer_tick).contains(&tokio::time::Instant::now()));

        let Some(NautilusWsMessage::Candle(bar)) = message else {
            panic!("expected the timer to publish a candle from the idle feed");
        };

        assert_eq!(
            bar,
            expected_candle("60702.0", "23.642", UnixNanos::from(CANDLE_DEADLINE_NS))
        );

        let next = handler.next();
        tokio::pin!(next);
        assert!(futures_util::poll!(next.as_mut()).is_pending());
    }

    fn candle_stream() -> (CandleStreams, AHashMap<Ustr, InstrumentAny>, CandleData) {
        let HyperliquidWsMessage::Candle { data } = serde_json::from_str(CANDLE_FRAME).unwrap()
        else {
            panic!("expected candle fixture");
        };

        let mut candles = CandleStreams::default();
        candles.register(CANDLE_KEY.to_string(), candle_bar_type());
        let instruments = AHashMap::from([(Ustr::from("BTC"), btc_perp())]);
        (candles, instruments, data)
    }

    fn candle_bar_type() -> BarType {
        BarType::from("BTC-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL")
    }

    fn expected_candle(close: &str, volume: &str, ts_init: UnixNanos) -> Bar {
        Bar::new(
            candle_bar_type(),
            Price::from("60705.0"),
            Price::from("60728.0"),
            Price::from("60663.0"),
            Price::from(close),
            Quantity::from(volume),
            UnixNanos::from(1_795_656_840_000_000_000),
            ts_init,
        )
    }

    #[rstest]
    #[tokio::test]
    async fn compact_replacement_waits_for_current_ack_and_new_baseline() {
        use super::super::messages::{WsCompactBookData, WsCompactBookUpdate};

        let mut handler = compact_handler();
        let sub = SubscriptionRequest::L2 {
            coin: "BTC".into(),
            n_sig_figs: Some(5),
            mantissa: Some(2),
        };
        let subscribe = HyperliquidWsRequest::Subscribe {
            subscription: sub.clone(),
        };
        let unsubscribe = HyperliquidWsRequest::Unsubscribe {
            subscription: sub.clone(),
        };
        handler.subscriptions.mark_subscribe("l2:BTC");
        handler.prepare_compact_request(&subscribe);
        handler.prepare_compact_request(&unsubscribe);
        handler.prepare_compact_request(&subscribe);

        // The first subscribe ACK belongs to the stream replaced before confirmation
        handler.confirm_compact_request(&sub, "subscribe");
        assert!(
            handler
                .handle_compact_book(
                    WsCompactBookData::Snapshot(one_level_book()),
                    UnixNanos::default()
                )
                .is_none()
        );
        handler.confirm_compact_request(&sub, "unsubscribe");
        handler.confirm_compact_request(&sub, "subscribe");
        let update: WsCompactBookUpdate = serde_json::from_value(json!({
            "c":"BTC","t":1_700_000_000_000_u64,"r":[[],[]],
            "l":[[{"p":"100","s":"2"}],[]],
        }))
        .unwrap();
        assert!(
            handler
                .handle_compact_book(
                    WsCompactBookData::Update(update.clone()),
                    UnixNanos::default()
                )
                .is_none()
        );
        let snapshot = handler.handle_compact_book(
            WsCompactBookData::Snapshot(one_level_book()),
            UnixNanos::default(),
        );
        assert!(
            matches!(&snapshot, Some(NautilusWsMessage::CompactBook { is_snapshot: true, deltas, .. }) if deltas.deltas.len() == 3)
        );
        let updated =
            handler.handle_compact_book(WsCompactBookData::Update(update), UnixNanos::default());
        assert!(
            matches!(&updated, Some(NautilusWsMessage::CompactBook { is_snapshot: false, deltas, .. }) if deltas.deltas[1].order.size == Quantity::from("2"))
        );

        handler.prepare_compact_request(&unsubscribe);
        assert!(
            handler
                .handle_compact_book(
                    WsCompactBookData::Snapshot(one_level_book()),
                    UnixNanos::default()
                )
                .is_none()
        );
        handler.confirm_compact_request(&sub, "unsubscribe");
        assert!(!handler.compact_books.contains_key(&Ustr::from("BTC")));
    }

    #[rstest]
    #[case::replacement(false)]
    #[case::reconnect(true)]
    #[tokio::test]
    async fn compact_replacement_retires_already_queued_snapshot(#[case] reconnect: bool) {
        use nautilus_common::live::dst::time::Instant;

        use super::super::messages::WsCompactBookData;
        use crate::book::sync::BookSyncTracker;

        let mut handler = compact_handler();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        handler.out_tx = tx;
        let sub = SubscriptionRequest::L2 {
            coin: "BTC".into(),
            n_sig_figs: None,
            mantissa: None,
        };
        let subscribe = HyperliquidWsRequest::Subscribe {
            subscription: sub.clone(),
        };
        handler.subscriptions.mark_subscribe("l2:BTC");
        handler.prepare_compact_request(&subscribe);
        handler.confirm_compact_request(&sub, "subscribe");
        let old = handler
            .handle_compact_book(
                WsCompactBookData::Snapshot(one_level_book()),
                UnixNanos::default(),
            )
            .unwrap();
        handler.send(old).unwrap();
        let invalid = serde_json::from_value(json!({
            "c":"BTC","t":1_700_000_000_001_u64,"r":[[9],[]],"l":[[],[]],
        }))
        .unwrap();
        let error = handler
            .handle_compact_book(WsCompactBookData::Update(invalid), UnixNanos::default())
            .unwrap();
        handler.send(error).unwrap();

        // A delta subscription replaces an existing depth stream before the old output is read
        let tracker = BookSyncTracker::default();
        let id = btc_perp().id();
        let stream_use = crate::websocket::book::BookStreamUse::Deltas;
        let registration = crate::websocket::book::BookStreamRegistry::default().register(
            "BTC".into(),
            stream_use,
            Default::default(),
        );
        let (cancel, gate) = tracker
            .register_compact(
                id,
                stream_use,
                registration,
                CancellationToken::new(),
                Instant::now(),
                |cancel, gate| Ok((cancel, gate)),
            )
            .unwrap()
            .unwrap();

        if reconnect {
            handler.compact_books.clear();
        } else {
            handler.prepare_compact_request(&HyperliquidWsRequest::Unsubscribe {
                subscription: sub.clone(),
            });
        }

        handler.prepare_compact_request(&subscribe);
        gate.open();
        let NautilusWsMessage::CompactBook { output_gate, .. } = rx.recv().await.unwrap() else {
            panic!("expected compact book");
        };
        assert!(output_gate.lock().is_closed());
        let NautilusWsMessage::BookInvalid {
            output_gate: Some(error_gate),
            ..
        } = rx.recv().await.unwrap()
        else {
            panic!("expected compact conversion error");
        };
        assert!(error_gate.lock().is_closed());
        assert!(!cancel.is_cancelled());

        handler.confirm_compact_request(&sub, "unsubscribe");
        handler.confirm_compact_request(&sub, "subscribe");
        let fresh = handler
            .handle_compact_book(
                WsCompactBookData::Snapshot(one_level_book()),
                UnixNanos::default(),
            )
            .unwrap();
        let NautilusWsMessage::CompactBook {
            is_snapshot,
            output_gate,
            ..
        } = fresh
        else {
            panic!("expected replacement snapshot");
        };
        assert!(!output_gate.lock().is_closed());
        let mut published = false;
        tracker.record_compact(id, is_snapshot, Instant::now(), |deltas, _| {
            published = deltas;
        });
        assert!(published);
        assert!(cancel.is_cancelled());
    }

    fn btc_active_spot_asset_ctx() -> WsActiveAssetCtxData {
        WsActiveAssetCtxData::Spot {
            coin: Ustr::from("BTC"),
            ctx: SpotAssetCtx {
                shared: SharedAssetCtx {
                    day_ntl_vlm: dec!(1000000.0),
                    prev_day_px: dec!(49000.0),
                    mark_px: dec!(50000.0),
                    mid_px: Some(dec!(50001.0)),
                    impact_pxs: None,
                    day_base_vlm: Some(dec!(100.0)),
                },
                circulating_supply: dec!(19000000.0),
            },
        }
    }

    fn btc_active_asset_ctx(open_interest: Decimal) -> WsActiveAssetCtxData {
        WsActiveAssetCtxData::Perp {
            coin: Ustr::from("BTC"),
            ctx: PerpsAssetCtx {
                shared: SharedAssetCtx {
                    day_ntl_vlm: dec!(1000000.0),
                    prev_day_px: dec!(49000.0),
                    mark_px: dec!(50000.0),
                    mid_px: Some(dec!(50001.0)),
                    impact_pxs: Some(vec![dec!(50000.0), dec!(50002.0)]),
                    day_base_vlm: Some(dec!(100.0)),
                },
                funding: dec!(0.0001),
                open_interest,
                oracle_px: dec!(50005.0),
                premium: Some(dec!(-0.0001)),
            },
        }
    }

    fn sample_all_dexs_asset_ctxs() -> WsAllDexsAssetCtxsData {
        let raw = include_str!("../../test_data/ws_all_dexs_asset_ctxs.json");
        let msg: super::super::messages::HyperliquidWsMessage =
            serde_json::from_str(raw).expect("expected valid allDexsAssetCtxs fixture");

        let super::super::messages::HyperliquidWsMessage::AllDexsAssetCtxs { data } = msg else {
            panic!("expected allDexsAssetCtxs fixture message");
        };

        let default_entry = data
            .ctxs
            .iter()
            .find(|(dex, _)| dex.is_empty())
            .and_then(|(dex, ctxs)| ctxs.first().cloned().map(|ctx| (dex.clone(), vec![ctx])))
            .expect("expected default dex sample");
        let xyz_entry = data
            .ctxs
            .iter()
            .find(|(dex, _)| dex == "xyz")
            .and_then(|(dex, ctxs)| ctxs.first().cloned().map(|ctx| (dex.clone(), vec![ctx])))
            .expect("expected xyz dex sample");

        WsAllDexsAssetCtxsData {
            ctxs: vec![default_entry, xyz_entry],
        }
    }

    #[tokio::test]
    async fn post_send_failure_cancels_router_waiter() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let post_router = PostRouter::new();
        let cloid_cache: CloidCache = Arc::new(Mutex::new(FifoCacheMap::<
            Ustr,
            ClientOrderId,
            CLOID_CACHE_CAPACITY,
        >::new()));
        let mut handler = FeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            SubscriptionState::new(':'),
            cloid_cache,
            Arc::clone(&post_router),
            Arc::new(WebSocketRateLimits::new()),
            1,
        );

        let id = 99;
        let cancellation_token = CancellationToken::new();
        let rx = post_router
            .register_with_cancellation(id, &cancellation_token)
            .await
            .unwrap();

        let task = tokio::spawn(async move { handler.next().await });

        cmd_tx
            .send(HandlerCommand::Post {
                id,
                request: PostRequest::Info {
                    payload: json!({"type": "userRateLimit", "user": "0x123"}),
                },
                deadline: tokio::time::Instant::now() + Duration::from_secs(1),
                cancellation_token,
            })
            .unwrap();
        drop(cmd_tx);
        drop(raw_tx);

        let closed = tokio::time::timeout(Duration::from_millis(100), rx)
            .await
            .expect("post waiter should close without waiting for post timeout");
        assert!(closed.is_err(), "post router cancel must close the waiter");
        let _rx = post_router
            .register(id)
            .await
            .expect("post id should be reusable after cancellation");
        assert!(task.await.unwrap().is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn write_book_opens_gate_only_after_completed_write() {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();

        let cloid_cache: CloidCache = Arc::new(Mutex::new(FifoCacheMap::<
            Ustr,
            ClientOrderId,
            CLOID_CACHE_CAPACITY,
        >::new()));

        let mut handler = FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            SubscriptionState::new(':'),
            cloid_cache,
            PostRouter::new(),
            Arc::new(WebSocketRateLimits::new()),
            1,
        );

        let subscription = || SubscriptionRequest::L2Book {
            coin: Ustr::from("BTC"),
            n_sig_figs: None,
            mantissa: None,
        };

        let failed_gate = SnapshotGate::default();
        failed_gate.lock().close();
        let (failed_tx, failed_rx) = tokio::sync::oneshot::channel();
        let cancelled_gate = SnapshotGate::default();
        cancelled_gate.lock().close();
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel();

        // No client is set, so the write fails before any send
        cmd_tx
            .send(HandlerCommand::WriteBook {
                subscription: subscription(),
                replace: false,
                cancel: CancellationToken::new(),
                gate: failed_gate.clone(),
                completion: failed_tx,
            })
            .unwrap();

        cmd_tx
            .send(HandlerCommand::WriteBook {
                subscription: subscription(),
                replace: true,
                cancel: cancelled,
                gate: cancelled_gate.clone(),
                completion: cancelled_tx,
            })
            .unwrap();

        drop(cmd_tx);
        drop(raw_tx);

        let next = handler.next().await;
        let failed = failed_rx.await.unwrap();
        let attempted = cancelled_rx.await.unwrap();

        assert!(next.is_none());
        assert!(matches!(
            failed,
            Err(HyperliquidWsError::ClientError(message))
                if message == "No WebSocket client available"
        ));
        assert!(failed_gate.lock().is_closed());
        assert!(
            matches!(
                attempted,
                Err(HyperliquidWsError::ClientError(message))
                    if message == "No WebSocket client available"
            ),
            "a cancelled write still runs",
        );
        assert!(cancelled_gate.lock().is_closed());
    }

    fn retry_manager_with_backoff() -> RetryManager<PostSendError> {
        RetryManager::new(RetryConfig {
            max_retries: 1,
            initial_delay_ms: 1_000,
            max_delay_ms: 1_000,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: None,
            immediate_first: false,
            max_elapsed_ms: None,
        })
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn expired_post_deadline_prevents_first_send() {
        let cancellation_token = CancellationToken::new();
        let sends = Arc::new(AtomicUsize::new(0));
        let send_count = Arc::clone(&sends);

        let error = send_post_before_deadline(
            tokio::time::Instant::now(),
            &cancellation_token,
            move || {
                let send_count = Arc::clone(&send_count);
                async move {
                    send_count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .await
        .unwrap_err();

        assert!(matches!(error, PostSendError::Deadline));
        assert_eq!(sends.load(Ordering::SeqCst), 0);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn post_deadline_while_waiting_for_message_quota_prevents_send() {
        let manager = retry_manager_with_backoff();
        let cancellation_token = CancellationToken::new();
        let limits = Arc::new(WebSocketRateLimits::new());
        let rate_key = limits.message_key();
        for _ in 0..HYPERLIQUID_WS_MESSAGES_PER_MINUTE {
            assert!(limits.messages.check_key(&rate_key).is_ok());
        }
        let sends = Arc::new(AtomicUsize::new(0));
        let send_count = Arc::clone(&sends);
        let send_limits = Arc::clone(&limits);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(10);

        let error = send_post_with_retry(&manager, deadline, &cancellation_token, move || {
            let send_count = Arc::clone(&send_count);
            let send_limits = Arc::clone(&send_limits);
            async move {
                send_limits.acquire_message().await;
                send_count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .await
        .unwrap_err();

        assert!(matches!(error, PostSendError::Deadline));
        assert_eq!(sends.load(Ordering::SeqCst), 0);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn post_deadline_during_backoff_prevents_retry() {
        let manager = retry_manager_with_backoff();
        let cancellation_token = CancellationToken::new();
        let sends = Arc::new(AtomicUsize::new(0));
        let send_count = Arc::clone(&sends);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(100);

        let error = send_post_with_retry(&manager, deadline, &cancellation_token, move || {
            let send_count = Arc::clone(&send_count);
            async move {
                send_count.fetch_add(1, Ordering::SeqCst);
                Err(PostSendError::Transport(SendError::Timeout))
            }
        })
        .await
        .unwrap_err();

        assert!(matches!(error, PostSendError::Deadline));
        assert_eq!(sends.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn post_deadline_after_send_starts_preserves_unknown_outcome() {
        let manager = retry_manager_with_backoff();
        let cancellation_token = CancellationToken::new();
        let sends = Arc::new(AtomicUsize::new(0));
        let send_count = Arc::clone(&sends);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(100);

        let error = send_post_with_retry(&manager, deadline, &cancellation_token, move || {
            let send_count = Arc::clone(&send_count);
            async move {
                send_count.fetch_add(1, Ordering::SeqCst);
                std::future::pending::<Result<(), PostSendError>>().await
            }
        })
        .await
        .unwrap_err();

        assert!(matches!(error, PostSendError::Deadline));
        assert_eq!(sends.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn post_cancellation_stops_started_send() {
        let manager = retry_manager_with_backoff();
        let cancellation_token = CancellationToken::new();
        let task_cancellation_token = cancellation_token.clone();
        let started = Arc::new(tokio::sync::Notify::new());
        let task_started = Arc::clone(&started);
        let sends = Arc::new(AtomicUsize::new(0));
        let task_sends = Arc::clone(&sends);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        let task = tokio::spawn(async move {
            send_post_with_retry(&manager, deadline, &task_cancellation_token, move || {
                let task_started = Arc::clone(&task_started);
                let task_sends = Arc::clone(&task_sends);
                async move {
                    task_sends.fetch_add(1, Ordering::SeqCst);
                    task_started.notify_one();
                    std::future::pending::<Result<(), PostSendError>>().await
                }
            })
            .await
        });

        started.notified().await;
        cancellation_token.cancel();
        let error = task.await.unwrap().unwrap_err();

        assert!(matches!(error, PostSendError::Retry(RetryError::Canceled)));
        assert_eq!(sends.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    #[case(SendError::Timeout, true)]
    #[case(SendError::ConnectionChanged, true)]
    #[case(SendError::WriteTimeout, false)]
    #[case(SendError::BrokenPipe("transport failed".to_string()), false)]
    #[case(SendError::Closed, false)]
    #[case(SendError::InvalidInput("invalid payload".to_string()), false)]
    fn post_send_retries_only_before_writing(#[case] error: SendError, #[case] expected: bool) {
        assert_eq!(
            should_retry_post_send(&PostSendError::Transport(error)),
            expected
        );
    }

    #[tokio::test]
    async fn malformed_private_frame_logs_omit_payloads() {
        let _capture = capture_logs().await;

        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../../test_data/ws_malformed_private_frames.json"
        ))
        .unwrap();
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let cloid_cache: CloidCache = Arc::new(Mutex::new(FifoCacheMap::new()));

        let mut handler = FeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            Some("HYPERLIQUID-PRIVATE_ACCOUNT_SENTINEL".into()),
            SubscriptionState::new(':'),
            cloid_cache,
            PostRouter::new(),
            Arc::new(WebSocketRateLimits::new()),
            937,
        );
        let instrument = btc_perp();
        handler.instruments.insert(Ustr::from("BTC"), instrument);
        let mut expected = Vec::new();

        for case in fixtures["decode"].as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            let e = super::super::messages::decode_ws_message(text).unwrap_err();
            let channel = case["channel"].as_str().unwrap();
            expected.push(format!(
                "Error parsing WebSocket message: channel={channel}, client_id=937, payload_bytes={}, category={:?}, line={}, column={}",
                text.len(), e.classify(), e.line(), e.column(),
            ));
            raw_tx.send(Message::Text(text.to_owned().into())).unwrap();
        }

        for case in fixtures["conversion"].as_array().unwrap() {
            let text = case["message"].to_string();
            super::super::messages::decode_ws_message(&text).unwrap();
            let class = case["class"].as_str().unwrap();
            expected.push(format!(
                "Error parsing WebSocket message: class={class}, client_id=937, payload_bytes={}, category=conversion",
                text.len(),
            ));
            raw_tx.send(Message::Text(text.into())).unwrap();
        }

        raw_tx
            .send(Message::Text(nautilus_network::RECONNECTED.into()))
            .unwrap();
        drop(raw_tx);
        drop(cmd_tx);

        let next = tokio::time::timeout(Duration::from_secs(5), handler.next())
            .await
            .unwrap();
        let messages = LOG_CAPTURE.messages();

        let errors: Vec<_> = messages
            .iter()
            .filter(|message| {
                message.starts_with("Error parsing WebSocket message:")
                    && message.contains("client_id=937,")
            })
            .cloned()
            .collect();

        assert!(matches!(next, Some(NautilusWsMessage::Reconnected)));
        assert_eq!(errors, expected);

        for secret in fixtures["secrets"].as_array().unwrap() {
            let secret = secret.as_str().unwrap();
            assert!(
                messages.iter().all(|message| !message.contains(secret)),
                "private value leaked: {secret}"
            );
        }

        assert!(errors.iter().all(|message| message.len() < 240));
    }

    #[rstest]
    #[tokio::test]
    async fn outbound_subscription_logs_omit_payload_bodies() {
        let _capture = capture_logs().await;

        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let post_router = PostRouter::new();
        let cloid_cache: CloidCache = Arc::new(Mutex::new(FifoCacheMap::<
            Ustr,
            ClientOrderId,
            CLOID_CACHE_CAPACITY,
        >::new()));
        let mut handler = FeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            SubscriptionState::new(':'),
            cloid_cache,
            post_router,
            Arc::new(WebSocketRateLimits::new()),
            1,
        );
        let subscription = SubscriptionRequest::Notification {
            user: SECRET_MARKER.to_string(),
        };
        let subscribe_len = serde_json::to_string(&HyperliquidWsRequest::Subscribe {
            subscription: subscription.clone(),
        })
        .unwrap()
        .len();
        let unsubscribe_len = serde_json::to_string(&HyperliquidWsRequest::Unsubscribe {
            subscription: subscription.clone(),
        })
        .unwrap()
        .len();

        LOG_CAPTURE.clear();

        cmd_tx
            .send(HandlerCommand::Subscribe {
                subscriptions: vec![subscription.clone()],
            })
            .unwrap();
        cmd_tx
            .send(HandlerCommand::Unsubscribe {
                subscriptions: vec![subscription],
            })
            .unwrap();
        drop(cmd_tx);
        drop(raw_tx);

        assert!(handler.next().await.is_none());

        let messages: Vec<_> = LOG_CAPTURE
            .messages()
            .into_iter()
            .filter(|message| message.starts_with("Sending "))
            .collect();

        assert!(
            messages
                .iter()
                .all(|message| !message.contains(SECRET_MARKER)),
            "outbound logs exposed the secret marker: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|message| message
                    == &format!("Sending subscribe payload ({subscribe_len} bytes)")),
            "subscribe metadata missing or inaccurate: {messages:?}"
        );
        assert!(
            messages.iter().any(|message| {
                message == &format!("Sending unsubscribe payload ({unsubscribe_len} bytes)")
            }),
            "unsubscribe metadata missing or inaccurate: {messages:?}"
        );
    }

    pub(in crate::websocket) async fn capture_logs() -> tokio::sync::MutexGuard<'static, ()> {
        let guard = LOG_LOCK.lock().await;
        LOG_INSTALL.call_once(|| {
            log::set_logger(&LOG_CAPTURE).expect("test logger already installed");
            log::set_max_level(LevelFilter::Debug);
        });

        LOG_CAPTURE.clear();
        guard
    }

    pub(in crate::websocket) fn captured_logs() -> Vec<String> {
        LOG_CAPTURE.messages()
    }

    #[tokio::test]
    async fn unsubscribe_confirmation_releases_shared_reservation() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new(':');
        let limits = Arc::new(WebSocketRateLimits::new());
        let released = SubscriptionRequest::Trades {
            coin: Ustr::from("RELEASED"),
        };
        let released_key = super::subscription_to_key(&released);
        subscriptions.mark_subscribe(&released_key);
        subscriptions.confirm_subscribe(&released_key);
        subscriptions.mark_unsubscribe(&released_key);
        assert!(limits.reserve_subscription(1, &released).unwrap());

        for index in 1..HYPERLIQUID_WS_SUBSCRIPTIONS_MAX {
            assert!(
                limits
                    .reserve_subscription(
                        1,
                        &SubscriptionRequest::Trades {
                            coin: Ustr::from(&format!("COIN-{index}")),
                        },
                    )
                    .unwrap()
            );
        }
        assert!(
            limits
                .reserve_subscription(
                    2,
                    &SubscriptionRequest::Trades {
                        coin: Ustr::from("BEFORE-ACK"),
                    },
                )
                .is_err()
        );

        let cloid_cache: CloidCache = Arc::new(Mutex::new(FifoCacheMap::<
            Ustr,
            ClientOrderId,
            CLOID_CACHE_CAPACITY,
        >::new()));
        let mut handler = FeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            None,
            subscriptions,
            cloid_cache,
            PostRouter::new(),
            Arc::clone(&limits),
            1,
        );
        raw_tx
            .send(Message::Text(
                json!({
                    "channel": "subscriptionResponse",
                    "data": {
                        "method": "unsubscribe",
                        "subscription": {
                            "type": "trades",
                            "coin": "RELEASED",
                        },
                    },
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        drop(raw_tx);
        drop(cmd_tx);

        assert!(handler.next().await.is_none());
        assert!(
            limits
                .reserve_subscription(
                    2,
                    &SubscriptionRequest::Trades {
                        coin: Ustr::from("AFTER-ACK"),
                    },
                )
                .unwrap()
        );
    }

    #[rstest]
    fn handle_l2_book_emits_deltas_only_when_not_in_depth_subs() {
        let mut instruments = AHashMap::new();
        instruments.insert(Ustr::from("BTC"), btc_perp());
        let depth_subs = AHashSet::<Ustr>::new();

        let msgs = FeedHandler::handle_l2_book(
            &one_level_book(),
            &instruments,
            &depth_subs,
            UnixNanos::default(),
        );

        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], NautilusWsMessage::Deltas(_)));
    }

    #[rstest]
    fn handle_l2_book_emits_deltas_and_depth_when_coin_in_subs() {
        let mut instruments = AHashMap::new();
        instruments.insert(Ustr::from("BTC"), btc_perp());
        let mut depth_subs = AHashSet::<Ustr>::new();
        depth_subs.insert(Ustr::from("BTC"));

        let msgs = FeedHandler::handle_l2_book(
            &one_level_book(),
            &instruments,
            &depth_subs,
            UnixNanos::default(),
        );

        assert_eq!(msgs.len(), 2);
        assert!(matches!(msgs[0], NautilusWsMessage::Deltas(_)));
        assert!(matches!(msgs[1], NautilusWsMessage::Depth(_)));
    }

    #[rstest]
    fn handle_l2_book_reports_unparsable_frame_as_invalid_book() {
        let instrument = btc_perp();
        let instrument_id = instrument.id();
        let mut instruments = AHashMap::new();
        instruments.insert(Ustr::from("BTC"), instrument);
        let mut book = one_level_book();
        book.time = u64::MAX;

        let msgs = FeedHandler::handle_l2_book(
            &book,
            &instruments,
            &AHashSet::<Ustr>::new(),
            UnixNanos::default(),
        );

        assert_eq!(msgs.len(), 1);
        assert!(matches!(
            msgs[0],
            NautilusWsMessage::BookInvalid { instrument_id: id, .. } if id == instrument_id
        ));
    }

    #[rstest]
    fn handle_l2_book_returns_empty_when_instrument_unknown() {
        let instruments = AHashMap::<Ustr, InstrumentAny>::new();
        let depth_subs = AHashSet::<Ustr>::new();

        let msgs = FeedHandler::handle_l2_book(
            &one_level_book(),
            &instruments,
            &depth_subs,
            UnixNanos::default(),
        );

        assert!(msgs.is_empty());
    }

    #[rstest]
    fn handle_asset_context_emits_open_interest_custom_data_when_subscribed() {
        let instrument = btc_perp();
        let instrument_id = instrument.id();
        let mut instruments = AHashMap::new();
        instruments.insert(Ustr::from("BTC"), instrument);

        let mut asset_context_subs = AHashMap::new();
        asset_context_subs.insert(
            Ustr::from("BTC"),
            AHashSet::from_iter([AssetContextDataType::OpenInterest]),
        );

        let mut asset_context_caches = AssetContextCaches::default();

        let msgs = FeedHandler::handle_asset_context(
            &btc_active_asset_ctx(dec!(100000.0)),
            &instruments,
            &asset_context_subs,
            &mut asset_context_caches,
            UnixNanos::default(),
        );

        assert_eq!(msgs.len(), 1);

        match &msgs[0] {
            NautilusWsMessage::CustomData(Data::Custom(custom)) => {
                let open_interest = custom
                    .data
                    .as_any()
                    .downcast_ref::<HyperliquidOpenInterest>()
                    .expect("expected HyperliquidOpenInterest");
                assert_eq!(open_interest.instrument_id, instrument_id);
                assert_eq!(open_interest.open_interest.to_string(), "100000.0");
                assert_eq!(
                    custom
                        .data_type
                        .metadata()
                        .and_then(|metadata| metadata.get_str("instrument_id"))
                        .map(ToString::to_string),
                    Some(instrument_id.to_string()),
                );
            }
            other => panic!("unexpected message type: {other:?}"),
        }
    }

    #[rstest]
    fn handle_all_dexs_asset_ctxs_emits_normalized_custom_data() {
        let mapping = AHashMap::from_iter([
            (
                Ustr::from(""),
                vec![Some(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))],
            ),
            (
                Ustr::from("xyz"),
                vec![Some(InstrumentId::from("xyz:XYZ100-USD-PERP.HYPERLIQUID"))],
            ),
        ]);

        let msg = FeedHandler::handle_all_dexs_asset_ctxs(
            sample_all_dexs_asset_ctxs(),
            &mapping,
            UnixNanos::default(),
        )
        .expect("expected custom data");

        match msg {
            NautilusWsMessage::CustomData(Data::Custom(custom)) => {
                let payload = custom
                    .data
                    .as_any()
                    .downcast_ref::<HyperliquidAllDexsAssetCtxs>()
                    .expect("expected HyperliquidAllDexsAssetCtxs");
                let btc_impact_prices = payload.entries[0].impact_prices.as_ref().unwrap();
                let xyz_impact_prices = payload.entries[1].impact_prices.as_ref().unwrap();

                assert_eq!(payload.entries.len(), 2);
                assert_eq!(
                    payload.entries[0].instrument_id,
                    InstrumentId::from("BTC-USD-PERP.HYPERLIQUID")
                );
                assert_eq!(payload.entries[1].dex, "xyz");
                assert_eq!(
                    payload.entries[1].instrument_id,
                    InstrumentId::from("xyz:XYZ100-USD-PERP.HYPERLIQUID")
                );
                assert_eq!(payload.entries[0].mark_price.to_string(), "77562.0");
                assert_eq!(payload.entries[1].day_base_volume.to_string(), "5135.2458");
                assert_eq!(btc_impact_prices.bid.as_decimal(), dec!(77558.0));
                assert_eq!(btc_impact_prices.ask.as_decimal(), dec!(77559.0));
                assert_eq!(xyz_impact_prices.bid.as_decimal(), dec!(29854.0));
                assert_eq!(xyz_impact_prices.ask.as_decimal(), dec!(29855.0));
            }
            other => panic!("expected custom data, found {other:?}"),
        }
    }

    #[rstest]
    fn handle_all_dexs_asset_ctxs_preserves_index_alignment_when_mappings_are_missing() {
        let data = WsAllDexsAssetCtxsData {
            ctxs: vec![(
                String::new(),
                vec![
                    PerpsAssetCtx {
                        shared: SharedAssetCtx {
                            day_ntl_vlm: dec!(1516669192.1953897476),
                            prev_day_px: dec!(76317.0),
                            mark_px: dec!(77562.0),
                            mid_px: Some(dec!(77558.5)),
                            impact_pxs: Some(vec![dec!(77558.0), dec!(77559.0)]),
                            day_base_vlm: Some(dec!(19707.77457)),
                        },
                        funding: dec!(-0.0000015186),
                        open_interest: dec!(27353.17682),
                        oracle_px: dec!(77605.0),
                        premium: Some(dec!(-0.0005927453)),
                    },
                    PerpsAssetCtx {
                        shared: SharedAssetCtx {
                            day_ntl_vlm: dec!(591989409.9392402172),
                            prev_day_px: dec!(2094.6),
                            mark_px: dec!(2123.7),
                            mid_px: Some(dec!(2123.95)),
                            impact_pxs: Some(vec![dec!(2123.65), dec!(2124.0)]),
                            day_base_vlm: Some(dec!(281686.8234999999)),
                        },
                        funding: dec!(0.0000125),
                        open_interest: dec!(605822.2557999999),
                        oracle_px: dec!(2124.6),
                        premium: Some(dec!(-0.0002824061)),
                    },
                ],
            )],
        };

        let mapping = AHashMap::from_iter([(
            Ustr::from(""),
            vec![None, Some(InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"))],
        )]);

        let msg = FeedHandler::handle_all_dexs_asset_ctxs(data, &mapping, UnixNanos::default())
            .expect("expected custom data");

        match msg {
            NautilusWsMessage::CustomData(Data::Custom(custom)) => {
                let payload = custom
                    .data
                    .as_any()
                    .downcast_ref::<HyperliquidAllDexsAssetCtxs>()
                    .expect("expected HyperliquidAllDexsAssetCtxs");
                assert_eq!(payload.entries.len(), 1);
                assert_eq!(
                    payload.entries[0].instrument_id,
                    InstrumentId::from("ETH-USD-PERP.HYPERLIQUID")
                );
                assert_eq!(payload.entries[0].mark_price.to_string(), "2123.7");
            }
            other => panic!("expected custom data, found {other:?}"),
        }
    }

    #[rstest]
    fn handle_asset_context_skips_open_interest_for_spot_payload() {
        let instrument = btc_perp();
        let mut instruments = AHashMap::new();
        instruments.insert(Ustr::from("BTC"), instrument);

        let mut asset_context_subs = AHashMap::new();
        asset_context_subs.insert(
            Ustr::from("BTC"),
            AHashSet::from_iter([AssetContextDataType::OpenInterest]),
        );

        let mut asset_context_caches = AssetContextCaches::default();

        let msgs = FeedHandler::handle_asset_context(
            &btc_active_spot_asset_ctx(),
            &instruments,
            &asset_context_subs,
            &mut asset_context_caches,
            UnixNanos::default(),
        );

        assert!(msgs.is_empty());
        assert!(asset_context_caches.open_interest.is_empty());
    }

    #[rstest]
    fn handle_asset_context_suppresses_unchanged_open_interest() {
        let instrument = btc_perp();
        let mut instruments = AHashMap::new();
        instruments.insert(Ustr::from("BTC"), instrument);

        let mut asset_context_subs = AHashMap::new();
        asset_context_subs.insert(
            Ustr::from("BTC"),
            AHashSet::from_iter([AssetContextDataType::OpenInterest]),
        );

        let mut asset_context_caches = AssetContextCaches::default();

        let first = FeedHandler::handle_asset_context(
            &btc_active_asset_ctx(dec!(100000.0)),
            &instruments,
            &asset_context_subs,
            &mut asset_context_caches,
            UnixNanos::default(),
        );
        let second = FeedHandler::handle_asset_context(
            &btc_active_asset_ctx(dec!(100000.0)),
            &instruments,
            &asset_context_subs,
            &mut asset_context_caches,
            UnixNanos::default(),
        );

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
    }

    #[rstest]
    fn asset_context_caches_clear_removed_data_types() {
        let coin = Ustr::from("BTC");
        let mut caches = AssetContextCaches::default();
        caches.mark_price.insert(coin, dec!(98455.5));
        caches.index_price.insert(coin, dec!(98460.0));
        caches.funding_rate.insert(coin, dec!(0.0001));
        caches.open_interest.insert(coin, dec!(1500.0));

        let previous_data_types = AHashSet::from_iter([
            AssetContextDataType::MarkPrice,
            AssetContextDataType::IndexPrice,
            AssetContextDataType::FundingRate,
            AssetContextDataType::OpenInterest,
        ]);
        let next_data_types = AHashSet::from_iter([
            AssetContextDataType::MarkPrice,
            AssetContextDataType::FundingRate,
        ]);

        caches.clear_removed(coin, Some(&previous_data_types), &next_data_types);

        assert_eq!(caches.mark_price.get(&coin).copied(), Some(dec!(98455.5)));
        assert!(caches.index_price.get(&coin).is_none());
        assert_eq!(caches.funding_rate.get(&coin).copied(), Some(dec!(0.0001)));
        assert!(caches.open_interest.get(&coin).is_none());
    }
}
