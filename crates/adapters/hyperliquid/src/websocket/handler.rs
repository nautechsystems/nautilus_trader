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
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use nautilus_common::cache::fifo::FifoCache;
use nautilus_core::{
    AtomicTime, MUTEX_POISONED, Params, nanos::UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{BarType, CustomData, Data, DataType},
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
    types::Price,
};
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{SubscriptionState, WebSocketClient},
};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    client::{AssetContextDataType, CloidCache},
    enums::HyperliquidWsChannel,
    error::HyperliquidWsError,
    messages::{
        CandleData, ExecutionReport, HyperliquidWsMessage, HyperliquidWsRequest, NautilusWsMessage,
        PostRequest, SubscriptionRequest, WsActiveAssetCtxData, WsUserEventData,
    },
    parse::{
        parse_ws_asset_context, parse_ws_candle, parse_ws_fill_report, parse_ws_order_book_deltas,
        parse_ws_order_book_depth10, parse_ws_order_status_report, parse_ws_quote_tick,
        parse_ws_trade_tick,
    },
    post::PostRouter,
};
use crate::data_types::HyperliquidAllMids;

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
#[allow(private_interfaces)]
pub enum HandlerCommand {
    /// Set the WebSocketClient for the handler to use.
    SetClient(WebSocketClient),
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
    /// Send a WebSocket post request.
    Post { id: u64, request: PostRequest },
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
    /// Cache spot fill coin mappings for instrument lookup.
    CacheSpotFillCoins(AHashMap<Ustr, Ustr>),
    /// Flag whether the `l2Book` stream for `coin` should also be emitted
    /// as [`NautilusWsMessage::Depth10`] snapshots.
    SetDepth10Sub { coin: Ustr, subscribed: bool },
}

pub(super) struct FeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    account_id: Option<AccountId>,
    subscriptions: SubscriptionState,
    post_router: Arc<PostRouter>,
    retry_manager: RetryManager<HyperliquidWsError>,
    message_buffer: VecDeque<NautilusWsMessage>,
    instruments: AHashMap<Ustr, InstrumentAny>,
    cloid_cache: CloidCache,
    bar_types_cache: AHashMap<String, BarType>,
    bar_cache: AHashMap<String, CandleData>,
    asset_context_subs: AHashMap<Ustr, AHashSet<AssetContextDataType>>,
    depth10_subs: AHashSet<Ustr>,
    processed_trade_ids: FifoCache<u64, 10_000>,
    mark_price_cache: AHashMap<Ustr, String>,
    index_price_cache: AHashMap<Ustr, String>,
    funding_rate_cache: AHashMap<Ustr, String>,
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
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            out_tx,
            account_id,
            subscriptions,
            post_router,
            retry_manager: create_websocket_retry_manager(),
            message_buffer: VecDeque::new(),
            instruments: AHashMap::new(),
            cloid_cache,
            bar_types_cache: AHashMap::new(),
            bar_cache: AHashMap::new(),
            asset_context_subs: AHashMap::new(),
            depth10_subs: AHashSet::new(),
            processed_trade_ids: FifoCache::new(),
            mark_price_cache: AHashMap::new(),
            index_price_cache: AHashMap::new(),
            funding_rate_cache: AHashMap::new(),
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
            self.retry_manager
                .execute_with_retry(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        async move {
                            client.send_text(payload, None).await.map_err(|e| {
                                HyperliquidWsError::ClientError(format!("Send failed: {e}"))
                            })
                        }
                    },
                    should_retry_hyperliquid_error,
                    create_hyperliquid_timeout_error,
                )
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
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("Setting WebSocket client in handler");
                            self.client = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Handler received disconnect command");

                            if let Some(ref client) = self.client {
                                client.disconnect().await;
                            }
                            self.signal.store(true, Ordering::SeqCst);
                            return None;
                        }
                        HandlerCommand::Subscribe { subscriptions } => {
                            for subscription in subscriptions {
                                let key = subscription_to_key(&subscription);
                                self.subscriptions.mark_subscribe(&key);

                                let request = HyperliquidWsRequest::Subscribe { subscription };
                                match serde_json::to_string(&request) {
                                    Ok(payload) => {
                                        log::debug!("Sending subscribe payload: {payload}");
                                        if let Err(e) = self.send_with_retry(payload).await {
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
                        HandlerCommand::Unsubscribe { subscriptions } => {
                            for subscription in subscriptions {
                                let key = subscription_to_key(&subscription);
                                self.subscriptions.mark_unsubscribe(&key);

                                let request = HyperliquidWsRequest::Unsubscribe { subscription };
                                match serde_json::to_string(&request) {
                                    Ok(payload) => {
                                        log::debug!("Sending unsubscribe payload: {payload}");
                                        if let Err(e) = self.send_with_retry(payload).await {
                                            log::error!("Error unsubscribing from {key}: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("Error serializing unsubscription for {key}: {e}");
                                    }
                                }
                            }
                        }
                        HandlerCommand::Post { id, request } => {
                            let request = HyperliquidWsRequest::Post { id, request };
                            match serde_json::to_string(&request) {
                                Ok(payload) => {
                                    log::debug!("Sending post payload: id={id}");
                                    if let Err(e) = self.send_with_retry(payload).await {
                                        log::error!("Error sending post request id={id}: {e}");
                                        self.post_router.cancel(id).await;
                                    }
                                }
                                Err(e) => {
                                    log::error!("Error serializing post request id={id}: {e}");
                                    self.post_router.cancel(id).await;
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
                            self.bar_types_cache.insert(key, bar_type);
                        }
                        HandlerCommand::RemoveBarType { key } => {
                            self.bar_types_cache.remove(&key);
                            self.bar_cache.remove(&key);
                        }
                        HandlerCommand::UpdateAssetContextSubs { coin, data_types } => {
                            if data_types.is_empty() {
                                self.asset_context_subs.remove(&coin);
                            } else {
                                self.asset_context_subs.insert(coin, data_types);
                            }
                        }
                        HandlerCommand::CacheSpotFillCoins(_) => {
                            // No longer needed - raw_symbol now contains the proper format
                        }
                        HandlerCommand::SetDepth10Sub { coin, subscribed } => {
                            if subscribed {
                                self.depth10_subs.insert(coin);
                            } else {
                                self.depth10_subs.remove(&coin);
                            }
                        }
                    }
                }

                Some(raw_msg) = self.raw_rx.recv() => {
                    match raw_msg {
                        Message::Text(text) => {
                            if text == RECONNECTED {
                                log::info!("Received RECONNECTED sentinel");
                                return Some(NautilusWsMessage::Reconnected);
                            }

                            match serde_json::from_str::<HyperliquidWsMessage>(&text) {
                                Ok(msg) => {
                                    if let HyperliquidWsMessage::Post { data } = msg {
                                        self.post_router.complete(data).await;
                                        continue;
                                    }

                                    let ts_init = self.clock.get_time_ns();
                                    let all_mids_data_types =
                                        Self::all_mids_data_types(&self.subscriptions);

                                    let nautilus_msgs = Self::parse_to_nautilus_messages(
                                        msg,
                                        &self.instruments,
                                        &self.cloid_cache,
                                        &self.bar_types_cache,
                                        self.account_id,
                                        ts_init,
                                        &self.asset_context_subs,
                                        &self.depth10_subs,
                                        &mut self.processed_trade_ids,
                                        &mut self.mark_price_cache,
                                        &mut self.index_price_cache,
                                        &mut self.funding_rate_cache,
                                        &mut self.bar_cache,
                                        &all_mids_data_types,
                                    );

                                    if !nautilus_msgs.is_empty() {
                                        let mut iter = nautilus_msgs.into_iter();
                                        let first = iter.next().unwrap();
                                        self.message_buffer.extend(iter);
                                        return Some(first);
                                    }
                                }
                                Err(e) => {
                                    log::error!("Error parsing WebSocket message: {e}, text: {text}");
                                }
                            }
                        }
                        Message::Ping(data) => {
                            if let Some(ref client) = self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await {
                                log::error!("Error sending pong: {e}");
                            }
                        }
                        Message::Close(_) => {
                            log::info!("Received WebSocket close frame");
                            return None;
                        }
                        _ => {}
                    }
                }

                else => {
                    log::debug!("Handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn parse_to_nautilus_messages(
        msg: HyperliquidWsMessage,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        cloid_cache: &CloidCache,
        bar_types: &AHashMap<String, BarType>,
        account_id: Option<AccountId>,
        ts_init: UnixNanos,
        asset_context_subs: &AHashMap<Ustr, AHashSet<AssetContextDataType>>,
        depth10_subs: &AHashSet<Ustr>,
        processed_trade_ids: &mut FifoCache<u64, 10_000>,
        mark_price_cache: &mut AHashMap<Ustr, String>,
        index_price_cache: &mut AHashMap<Ustr, String>,
        funding_rate_cache: &mut AHashMap<Ustr, String>,
        bar_cache: &mut AHashMap<String, CandleData>,
        all_mids_data_types: &[DataType],
    ) -> Vec<NautilusWsMessage> {
        let mut result = Vec::new();

        match msg {
            HyperliquidWsMessage::OrderUpdates { data } => {
                if let Some(account_id) = account_id
                    && let Some(msg) = Self::handle_order_updates(
                        &data,
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
                        for fill in &fills {
                            log::debug!(
                                "Fill: oid={}, coin={}, side={:?}, sz={}, px={}",
                                fill.oid,
                                fill.coin,
                                fill.side,
                                fill.sz,
                                fill.px
                            );
                        }

                        if let Some(account_id) = account_id {
                            log::debug!("Processing fills with account_id={account_id}");

                            if let Some(msg) = Self::handle_user_fills(
                                &fills,
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
                if let Some(msg) = Self::handle_trades(&data, instruments, ts_init) {
                    result.push(msg);
                }
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
            HyperliquidWsMessage::Bbo { data } => {
                if let Some(msg) = Self::handle_bbo(&data, instruments, ts_init) {
                    result.push(msg);
                }
            }
            HyperliquidWsMessage::L2Book { data } => {
                result.extend(Self::handle_l2_book(
                    &data,
                    instruments,
                    depth10_subs,
                    ts_init,
                ));
            }
            HyperliquidWsMessage::Candle { data } => {
                if let Some(msg) =
                    Self::handle_candle(&data, instruments, bar_types, bar_cache, ts_init)
                {
                    result.push(msg);
                }
            }
            HyperliquidWsMessage::ActiveAssetCtx { data }
            | HyperliquidWsMessage::ActiveSpotAssetCtx { data } => {
                result.extend(Self::handle_asset_context(
                    &data,
                    instruments,
                    asset_context_subs,
                    mark_price_cache,
                    index_price_cache,
                    funding_rate_cache,
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
                            let resolved = cloid_cache
                                .lock()
                                .expect(MUTEX_POISONED)
                                .get(&cloid_ustr)
                                .copied();

                            if let Some(real_client_order_id) = resolved {
                                log::debug!("Resolved cloid {cloid} -> {real_client_order_id}");
                                report.client_order_id = Some(real_client_order_id);
                            }
                        }
                        exec_reports.push(ExecutionReport::Order(report));
                    }
                    Err(e) => {
                        log::error!("Error parsing order update: {e}");
                    }
                }
            } else {
                log::debug!("No instrument found for coin: {}", order_update.order.coin);
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
                log::debug!("Found instrument for fill coin={}", fill.coin);
                match parse_ws_fill_report(fill, instrument, account_id, ts_init) {
                    Ok(mut report) => {
                        // Mark processed only after successful parse
                        processed_trade_ids.add(fill.tid);

                        if let Some(cloid) = &fill.cloid {
                            let cloid_ustr = Ustr::from(cloid.as_str());
                            let resolved = cloid_cache
                                .lock()
                                .expect(MUTEX_POISONED)
                                .get(&cloid_ustr)
                                .copied();

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
                    Err(e) => {
                        log::error!("Error parsing fill: {e}");
                    }
                }
            } else {
                // Not marked as processed so fill is retried if instrument loads later
                log::warn!("No instrument found for fill coin={}", fill.coin);
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
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut trade_ticks = Vec::new();

        for trade in data {
            if let Some(instrument) = instruments.get(&trade.coin) {
                match parse_ws_trade_tick(trade, instrument, ts_init) {
                    Ok(tick) => trade_ticks.push(tick),
                    Err(e) => {
                        log::error!("Error parsing trade tick: {e}");
                    }
                }
            } else {
                log::debug!("No instrument found for coin: {}", trade.coin);
            }
        }

        if trade_ticks.is_empty() {
            None
        } else {
            Some(NautilusWsMessage::Trades(trade_ticks))
        }
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
        depth10_subs: &AHashSet<Ustr>,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut out = Vec::new();

        let Some(instrument) = instruments.get(&data.coin) else {
            log::debug!("No instrument found for coin: {}", data.coin);
            return out;
        };

        match parse_ws_order_book_deltas(data, instrument, ts_init) {
            Ok(deltas) => out.push(NautilusWsMessage::Deltas(deltas)),
            Err(e) => log::error!("Error parsing order book deltas: {e}"),
        }

        if depth10_subs.contains(&data.coin) {
            match parse_ws_order_book_depth10(data, instrument, ts_init) {
                Ok(depth) => out.push(NautilusWsMessage::Depth10(Box::new(depth))),
                Err(e) => log::error!("Error parsing order book depth10: {e}"),
            }
        }

        out
    }

    fn handle_candle(
        data: &CandleData,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        bar_types: &AHashMap<String, BarType>,
        bar_cache: &mut AHashMap<String, CandleData>,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let key = format!("candle:{}:{}", data.s, data.i);

        let mut closed_bar = None;

        if let Some(cached) = bar_cache.get(&key) {
            // Emit cached bar when close_time changes, indicating the previous period closed
            if cached.close_time != data.close_time {
                log::debug!(
                    "Bar period changed for {}: prev_close_time={}, new_close_time={}",
                    data.s,
                    cached.close_time,
                    data.close_time
                );
                closed_bar = Some(cached.clone());
            }
        }

        bar_cache.insert(key.clone(), data.clone());

        if let Some(closed_data) = closed_bar {
            if let Some(bar_type) = bar_types.get(&key) {
                if let Some(instrument) = instruments.get(&data.s) {
                    match parse_ws_candle(&closed_data, instrument, bar_type, ts_init) {
                        Ok(bar) => return Some(NautilusWsMessage::Candle(bar)),
                        Err(e) => {
                            log::error!("Error parsing closed candle: {e}");
                        }
                    }
                } else {
                    log::debug!("No instrument found for coin: {}", data.s);
                }
            } else {
                log::debug!("No bar type found for key: {key}");
            }
        }

        None
    }

    fn handle_asset_context(
        data: &WsActiveAssetCtxData,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        asset_context_subs: &AHashMap<Ustr, AHashSet<AssetContextDataType>>,
        mark_price_cache: &mut AHashMap<Ustr, String>,
        index_price_cache: &mut AHashMap<Ustr, String>,
        funding_rate_cache: &mut AHashMap<Ustr, String>,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut result = Vec::new();

        let coin = match data {
            WsActiveAssetCtxData::Perp { coin, .. } => coin,
            WsActiveAssetCtxData::Spot { coin, .. } => coin,
        };

        if let Some(instrument) = instruments.get(coin) {
            let (mark_px, oracle_px, funding) = match data {
                WsActiveAssetCtxData::Perp { ctx, .. } => (
                    &ctx.shared.mark_px,
                    Some(&ctx.oracle_px),
                    Some(&ctx.funding),
                ),
                WsActiveAssetCtxData::Spot { ctx, .. } => (&ctx.shared.mark_px, None, None),
            };

            let mark_changed = mark_price_cache.get(coin) != Some(mark_px);
            let index_changed = oracle_px.is_some_and(|px| index_price_cache.get(coin) != Some(px));
            let funding_changed =
                funding.is_some_and(|rate| funding_rate_cache.get(coin) != Some(rate));

            let subscribed_types = asset_context_subs.get(coin);

            if mark_changed || index_changed || funding_changed {
                match parse_ws_asset_context(data, instrument, ts_init) {
                    Ok((mark_price, index_price, funding_rate)) => {
                        if mark_changed
                            && subscribed_types
                                .is_some_and(|s| s.contains(&AssetContextDataType::MarkPrice))
                        {
                            mark_price_cache.insert(*coin, mark_px.clone());
                            result.push(NautilusWsMessage::MarkPrice(mark_price));
                        }

                        if index_changed
                            && subscribed_types
                                .is_some_and(|s| s.contains(&AssetContextDataType::IndexPrice))
                        {
                            if let Some(px) = oracle_px {
                                index_price_cache.insert(*coin, px.clone());
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
                                funding_rate_cache.insert(*coin, rate.clone());
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
        } else {
            log::debug!("No instrument found for coin: {coin}");
        }

        result
    }

    fn all_mids_data_types(subscriptions: &SubscriptionState) -> Vec<DataType> {
        let mut topics = subscriptions.all_topics();
        topics.sort_unstable();
        topics.dedup();

        let all_mids_channel = HyperliquidWsChannel::AllMids.as_str();
        let all_mids_prefix = format!("{all_mids_channel}:");
        let mut data_types = Vec::new();

        for topic in topics {
            if topic == all_mids_channel {
                data_types.push(DataType::new("HyperliquidAllMids", None, None));
            } else if let Some(dex) = topic.strip_prefix(&all_mids_prefix) {
                let mut metadata = Params::new();
                metadata.insert(
                    "dex".to_string(),
                    serde_json::Value::String(dex.to_string()),
                );
                data_types.push(DataType::new("HyperliquidAllMids", Some(metadata), None));
            }
        }

        if data_types.is_empty() {
            data_types.push(DataType::new("HyperliquidAllMids", None, None));
        }

        data_types
    }
}

pub(crate) fn subscription_to_key(sub: &SubscriptionRequest) -> String {
    match sub {
        SubscriptionRequest::AllMids { dex } => {
            if let Some(dex_name) = dex {
                format!("{}:{dex_name}", HyperliquidWsChannel::AllMids.as_str())
            } else {
                HyperliquidWsChannel::AllMids.as_str().to_string()
            }
        }
        SubscriptionRequest::Notification { user } => {
            format!("{}:{user}", HyperliquidWsChannel::Notification.as_str())
        }
        SubscriptionRequest::WebData2 { user } => {
            format!("{}:{user}", HyperliquidWsChannel::WebData2.as_str())
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

/// Creates a timeout error for Hyperliquid retry logic.
pub(crate) fn create_hyperliquid_timeout_error(msg: String) -> HyperliquidWsError {
    HyperliquidWsError::ClientError(msg)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex, atomic::AtomicBool},
        time::Duration,
    };

    use ahash::{AHashMap, AHashSet};
    use nautilus_common::cache::fifo::FifoCacheMap;
    use nautilus_core::nanos::UnixNanos;
    use nautilus_model::{
        identifiers::{ClientOrderId, InstrumentId, Symbol},
        instruments::{CryptoPerpetual, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use nautilus_network::websocket::SubscriptionState;
    use rstest::rstest;
    use serde_json::json;
    use ustr::Ustr;

    use super::{
        super::{
            client::{CLOID_CACHE_CAPACITY, CloidCache},
            messages::{NautilusWsMessage, PostRequest, WsBookData, WsLevelData},
            post::PostRouter,
        },
        FeedHandler, HandlerCommand,
    };
    use crate::common::consts::HYPERLIQUID_VENUE;

    fn btc_perp() -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            InstrumentId::new(Symbol::new("BTC-PERP"), *HYPERLIQUID_VENUE),
            Symbol::new("BTC-PERP"),
            Currency::from("BTC"),
            Currency::from("USDC"),
            Currency::from("USDC"),
            false,
            2,
            3,
            Price::from("0.01"),
            Quantity::from("0.001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn one_level_book() -> WsBookData {
        WsBookData {
            coin: Ustr::from("BTC"),
            levels: [
                vec![WsLevelData {
                    px: "100.00".to_string(),
                    sz: "1.0".to_string(),
                    n: 1,
                }],
                vec![WsLevelData {
                    px: "100.01".to_string(),
                    sz: "1.0".to_string(),
                    n: 1,
                }],
            ],
            time: 1_700_000_000_000,
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
        );

        let id = 99;
        let rx = post_router.register(id).await.unwrap();

        let task = tokio::spawn(async move { handler.next().await });

        cmd_tx
            .send(HandlerCommand::Post {
                id,
                request: PostRequest::Info {
                    payload: json!({"type": "userRateLimit", "user": "0x123"}),
                },
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
    fn handle_l2_book_emits_deltas_only_when_not_in_depth10_subs() {
        let mut instruments = AHashMap::new();
        instruments.insert(Ustr::from("BTC"), btc_perp());
        let depth10_subs = AHashSet::<Ustr>::new();

        let msgs = FeedHandler::handle_l2_book(
            &one_level_book(),
            &instruments,
            &depth10_subs,
            UnixNanos::default(),
        );

        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], NautilusWsMessage::Deltas(_)));
    }

    #[rstest]
    fn handle_l2_book_emits_deltas_and_depth10_when_coin_in_subs() {
        let mut instruments = AHashMap::new();
        instruments.insert(Ustr::from("BTC"), btc_perp());
        let mut depth10_subs = AHashSet::<Ustr>::new();
        depth10_subs.insert(Ustr::from("BTC"));

        let msgs = FeedHandler::handle_l2_book(
            &one_level_book(),
            &instruments,
            &depth10_subs,
            UnixNanos::default(),
        );

        assert_eq!(msgs.len(), 2);
        assert!(matches!(msgs[0], NautilusWsMessage::Deltas(_)));
        assert!(matches!(msgs[1], NautilusWsMessage::Depth10(_)));
    }

    #[rstest]
    fn handle_l2_book_returns_empty_when_instrument_unknown() {
        let instruments = AHashMap::<Ustr, InstrumentAny>::new();
        let depth10_subs = AHashSet::<Ustr>::new();

        let msgs = FeedHandler::handle_l2_book(
            &one_level_book(),
            &instruments,
            &depth10_subs,
            UnixNanos::default(),
        );

        assert!(msgs.is_empty());
    }
}
