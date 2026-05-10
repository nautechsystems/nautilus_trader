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

//! Live market data client implementation for the Polymarket adapter.

use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashSet;
use anyhow::Context;
use dashmap::DashMap;
use nautilus_common::{
    clients::DataClient,
    live::{get_runtime, runner::get_data_event_sender},
    messages::{
        DataEvent, DataResponse,
        data::{
            BookResponse, InstrumentResponse, InstrumentsResponse, RequestBookSnapshot,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBookDeltas,
            SubscribeInstruments, SubscribeQuotes, SubscribeTrades, TradesResponse,
            UnsubscribeBookDeltas, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    providers::InstrumentProvider,
};
use nautilus_core::{
    AtomicMap, AtomicSet,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data as NautilusData, InstrumentStatus, OrderBookDeltas_API, QuoteTick},
    enums::{BookType, MarketStatusAction},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::consts::POLYMARKET_VENUE,
    config::PolymarketDataClientConfig,
    filters::InstrumentFilter,
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient, parse::rebuild_instrument_with_tick_size,
        query::GetGammaMarketsParams,
    },
    providers::{PolymarketInstrumentProvider, extract_condition_id, fetch_instruments},
    websocket::{
        client::PolymarketWebSocketClient,
        messages::{MarketWsMessage, PolymarketQuotes, PolymarketWsMessage},
        parse::{
            parse_book_deltas, parse_book_snapshot, parse_quote_from_price_change,
            parse_quote_from_snapshot, parse_timestamp_ms, parse_trade_tick,
        },
    },
};

const GAMMA_CONDITION_ID_CHUNK: usize = 100;

fn resolve_token_id_from(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument_id: InstrumentId,
) -> anyhow::Result<String> {
    let loaded = instruments.load();
    let instrument = loaded
        .get(&instrument_id)
        .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
    Ok(instrument.raw_symbol().as_str().to_string())
}

// Reconciles the WS subscription for `instrument_id` with the union of caller
// intents. Holds `ws_sub_mutex` across the async WS send so concurrent
// subscribe/unsubscribe calls arrive at the WS handler in mutex-release order;
// that makes the final wire state consistent with the last writer.
#[allow(
    clippy::too_many_arguments,
    reason = "shared state comes in as Arc refs"
)]
async fn sync_ws_subscription_async(
    instrument_id: InstrumentId,
    token_id_str: String,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    ws_open_tokens: Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    ws: crate::websocket::client::WsSubscriptionHandle,
) {
    let token_id = Ustr::from(token_id_str.as_str());
    let _guard = ws_sub_mutex.lock().await;

    let wants_subscribe = active_quote_subs.contains(&instrument_id)
        || active_delta_subs.contains(&instrument_id)
        || active_trade_subs.contains(&instrument_id);
    let is_open = ws_open_tokens.contains(&token_id);

    if wants_subscribe && !is_open {
        ws_open_tokens.insert(token_id);

        if let Err(e) = ws.subscribe_market(vec![token_id_str]).await {
            log::error!("Failed to subscribe to market data: {e:?}");
            // Roll back tracked WS state so a retry can take effect.
            ws_open_tokens.remove(&token_id);
        }
    } else if !wants_subscribe && is_open {
        ws_open_tokens.remove(&token_id);

        if let Err(e) = ws.unsubscribe_market(vec![token_id_str]).await {
            log::error!("Failed to unsubscribe from market data: {e:?}");
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TokenMeta {
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
}

// Inserts `instrument` into the live instrument cache and updates the
// `token_meta` routing index in one step. Every path that populates the live
// cache must go through here so WS messages can always resolve token_id back
// to an InstrumentId.
fn cache_instrument(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    instrument: &InstrumentAny,
) {
    let instrument_id = instrument.id();
    token_meta.insert(
        Ustr::from(instrument.raw_symbol().as_str()),
        TokenMeta {
            instrument_id,
            price_precision: instrument.price_precision(),
            size_precision: instrument.size_precision(),
        },
    );
    instruments.insert(instrument_id, instrument.clone());
}

struct WsMessageContext {
    clock: &'static AtomicTime,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    token_meta: Arc<DashMap<Ustr, TokenMeta>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    gamma_client: PolymarketGammaHttpClient,
    filters: Vec<Arc<dyn InstrumentFilter>>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    subscribe_new_markets: bool,
    new_market_filter: Option<Arc<dyn InstrumentFilter>>,
    cancellation_token: CancellationToken,
}

/// Polymarket data client for live market data streaming.
///
/// Integrates with the Nautilus DataEngine to provide:
/// - Real-time order book snapshots and deltas via WebSocket
/// - Quote ticks synthesized from book data
/// - Trade ticks from last trade price messages
/// - Automatic instrument discovery from the Gamma API
#[derive(Debug)]
pub struct PolymarketDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: PolymarketDataClientConfig,
    provider: PolymarketInstrumentProvider,
    clob_public_client: PolymarketClobPublicClient,
    data_api_client: PolymarketDataApiHttpClient,
    ws_client: PolymarketWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: Arc<DashMap<Ustr, TokenMeta>>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    ws_open_tokens: Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    pending_auto_loads: Arc<StdMutex<AHashSet<InstrumentId>>>,
    auto_load_scheduled: Arc<AtomicBool>,
}

impl PolymarketDataClient {
    /// Creates a new [`PolymarketDataClient`].
    pub fn new(
        client_id: ClientId,
        config: PolymarketDataClientConfig,
        gamma_client: PolymarketGammaHttpClient,
        clob_public_client: PolymarketClobPublicClient,
        data_api_client: PolymarketDataApiHttpClient,
        ws_client: PolymarketWebSocketClient,
    ) -> Self {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let provider = PolymarketInstrumentProvider::new(gamma_client);

        Self {
            clock,
            client_id,
            config,
            provider,
            clob_public_client,
            data_api_client,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            token_meta: Arc::new(DashMap::new()),
            order_books: Arc::new(DashMap::new()),
            last_quotes: Arc::new(DashMap::new()),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_delta_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            ws_open_tokens: Arc::new(AtomicSet::new()),
            ws_sub_mutex: Arc::new(tokio::sync::Mutex::new(())),
            pending_auto_loads: Arc::new(StdMutex::new(AHashSet::new())),
            auto_load_scheduled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns a reference to the client configuration.
    #[must_use]
    pub fn config(&self) -> &PolymarketDataClientConfig {
        &self.config
    }

    /// Returns the venue for this data client.
    #[must_use]
    pub fn venue(&self) -> Venue {
        *POLYMARKET_VENUE
    }

    /// Returns a reference to the instrument provider.
    #[must_use]
    pub fn provider(&self) -> &PolymarketInstrumentProvider {
        &self.provider
    }

    /// Adds an instrument filter on the underlying provider.
    pub fn add_instrument_filter(&mut self, filter: Arc<dyn InstrumentFilter>) {
        self.provider.add_filter(filter);
    }

    /// Returns `true` when the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn resolve_token_id(&self, instrument_id: InstrumentId) -> anyhow::Result<String> {
        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
        Ok(instrument.raw_symbol().as_str().to_string())
    }

    // Spawns an async task that reconciles the WS subscription for
    // `instrument_id`. The task holds `ws_sub_mutex` across the wire send so
    // concurrent subscribe/unsubscribe calls deliver commands to the WS handler
    // in a consistent order with the final `active_*_subs` state.
    fn sync_ws_subscription(&self, instrument_id: InstrumentId) {
        let token_id_str = match self.resolve_token_id(instrument_id) {
            Ok(s) => s,
            Err(_) => return,
        };
        let active_quote_subs = self.active_quote_subs.clone();
        let active_delta_subs = self.active_delta_subs.clone();
        let active_trade_subs = self.active_trade_subs.clone();
        let ws_open_tokens = self.ws_open_tokens.clone();
        let ws_sub_mutex = self.ws_sub_mutex.clone();
        let ws = self.ws_client.clone_subscription_handle();

        get_runtime().spawn(sync_ws_subscription_async(
            instrument_id,
            token_id_str,
            active_quote_subs,
            active_delta_subs,
            active_trade_subs,
            ws_open_tokens,
            ws_sub_mutex,
            ws,
        ));
    }

    fn queue_pending_load(&self, instrument_id: InstrumentId) {
        {
            let mut pending = self
                .pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned");
            pending.insert(instrument_id);
        }

        self.ensure_auto_load_task();
    }

    fn drop_pending_if_unwanted(&self, instrument_id: InstrumentId) {
        if self.active_quote_subs.contains(&instrument_id)
            || self.active_delta_subs.contains(&instrument_id)
            || self.active_trade_subs.contains(&instrument_id)
        {
            return;
        }
        let mut pending = self
            .pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned");
        pending.remove(&instrument_id);
    }

    fn ensure_auto_load_task(&self) {
        if self
            .auto_load_scheduled
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let pending = self.pending_auto_loads.clone();
        let scheduled = self.auto_load_scheduled.clone();
        let debounce_ms = self.config.auto_load_debounce_ms;
        let http = self.provider.http_client().clone();
        let filters = self.provider.filters();
        let instruments = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let active_quote_subs = self.active_quote_subs.clone();
        let active_delta_subs = self.active_delta_subs.clone();
        let active_trade_subs = self.active_trade_subs.clone();
        let ws_open_tokens = self.ws_open_tokens.clone();
        let ws_sub_mutex = self.ws_sub_mutex.clone();
        let ws_client = self.ws_client.clone_subscription_handle();
        let data_sender = self.data_sender.clone();
        let cancellation = self.cancellation_token.clone();

        get_runtime().spawn(async move {
            // Loop until the pending map is quiescent. Each iteration runs one
            // debounce window, then snapshots, fetches, and applies. A chunk
            // failure or a late-arriving miss keeps us in the loop; we exit
            // (releasing `scheduled`) only once `pending` is empty. This means a
            // transient Gamma failure is retried on the next debounce without
            // relying on some unrelated future miss to trigger it.
            loop {
                tokio::select! {
                    () = tokio::time::sleep(tokio::time::Duration::from_millis(debounce_ms)) => {}
                    () = cancellation.cancelled() => {
                        scheduled.store(false, Ordering::Release);
                        return;
                    }
                }

                let ids: Vec<InstrumentId> = {
                    let guard = pending.lock().expect("pending_auto_loads mutex poisoned");
                    guard.iter().copied().collect()
                };

                if ids.is_empty() {
                    scheduled.store(false, Ordering::Release);
                    return;
                }

                log::info!("Auto-loading {} missing instrument(s): {ids:?}", ids.len());

                let mut condition_ids: Vec<String> = ids
                    .iter()
                    .filter_map(|id| extract_condition_id(id).ok())
                    .collect();
                condition_ids.sort();
                condition_ids.dedup();

                if condition_ids.is_empty() {
                    log::error!("Auto-load aborted: no condition_ids could be extracted");
                    // Drop the stranded entries so we do not loop forever.
                    let mut guard = pending.lock().expect("pending_auto_loads mutex poisoned");
                    for id in &ids {
                        guard.remove(id);
                    }
                    continue;
                }

                // Gamma rejects condition_id queries larger than ~100, so chunk
                // the request and merge the results. This matches the provider's
                // own `_load_ids_using_gamma_markets` chunking policy.
                let mut loaded: Vec<InstrumentAny> =
                    Vec::with_capacity(condition_ids.len().min(GAMMA_CONDITION_ID_CHUNK));
                let mut chunk_failed = false;

                for chunk in condition_ids.chunks(GAMMA_CONDITION_ID_CHUNK) {
                    let params = GetGammaMarketsParams {
                        condition_ids: Some(chunk.join(",")),
                        ..Default::default()
                    };

                    match http.request_instruments_by_params(params).await {
                        Ok(insts) => loaded.extend(insts),
                        Err(e) => {
                            log::error!(
                                "Auto-load batch failed for chunk of {} condition_id(s): {e:?}",
                                chunk.len()
                            );
                            chunk_failed = true;
                            break;
                        }
                    }
                }

                if chunk_failed {
                    // Leave entries in `pending` and loop around; the next
                    // iteration retries after another debounce window.
                    continue;
                }

                for inst in loaded {
                    if !filters.iter().all(|f| f.accept(&inst)) {
                        log::debug!("Auto-loaded instrument {} filtered out", inst.id());
                        continue;
                    }

                    cache_instrument(&instruments, &token_meta, &inst);

                    let instrument_id = inst.id();
                    if let Err(e) = data_sender.send(DataEvent::Instrument(inst)) {
                        log::error!("Failed to emit auto-loaded instrument {instrument_id}: {e}");
                    }
                }

                for instrument_id in ids {
                    // Pop the pending entry under the lock; if `unsubscribe_*`
                    // already cleared it, skip.
                    let was_pending = {
                        let mut guard = pending.lock().expect("pending_auto_loads mutex poisoned");
                        guard.remove(&instrument_id)
                    };

                    if !was_pending {
                        continue;
                    }

                    let Ok(token_id) = resolve_token_id_from(&instruments, instrument_id) else {
                        log::error!("Auto-load did not return instrument {instrument_id}");
                        continue;
                    };

                    // Reconcile WS state with whichever `active_*_subs` still
                    // hold intent. A concurrent unsubscribe makes this a no-op.
                    sync_ws_subscription_async(
                        instrument_id,
                        token_id,
                        active_quote_subs.clone(),
                        active_delta_subs.clone(),
                        active_trade_subs.clone(),
                        ws_open_tokens.clone(),
                        ws_sub_mutex.clone(),
                        ws_client.clone(),
                    )
                    .await;
                }
            }
        });
    }

    async fn bootstrap_instruments(&mut self) -> anyhow::Result<()> {
        self.provider.load_all(None).await?;

        let all_instruments = self.provider.store().list_all();
        let total = all_instruments.len();
        for instrument in all_instruments {
            cache_instrument(&self.instruments, &self.token_meta, instrument);
            let instrument_id = instrument.id();

            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::warn!("Failed to publish instrument {instrument_id}: {e}");
            }
        }

        log::info!("Published all {total} instruments to data engine");
        Ok(())
    }

    fn spawn_message_handler(
        &mut self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>,
    ) {
        let cancellation = self.cancellation_token.clone();

        for (token_id, instrument) in self.provider.build_token_map() {
            self.token_meta.insert(
                token_id,
                TokenMeta {
                    instrument_id: instrument.id(),
                    price_precision: instrument.price_precision(),
                    size_precision: instrument.size_precision(),
                },
            );
        }

        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            gamma_client: self.provider.http_client().clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            cancellation_token: cancellation.clone(),
        };

        let handle = get_runtime().spawn(async move {
            log::debug!("Polymarket message handler started");

            loop {
                tokio::select! {
                    maybe_msg = rx.recv() => {
                        match maybe_msg {
                            Some(msg) => Self::handle_ws_message(msg, &ctx),
                            None => {
                                log::debug!("WebSocket message channel closed");
                                break;
                            }
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("Polymarket message handler cancelled");
                        break;
                    }
                }
            }

            log::debug!("Polymarket message handler ended");
        });

        self.tasks.push(handle);
    }

    fn handle_ws_message(message: PolymarketWsMessage, ctx: &WsMessageContext) {
        match message {
            PolymarketWsMessage::Market(market_msg) => {
                Self::handle_market_message(market_msg, ctx);
            }
            PolymarketWsMessage::User(_) => {
                log::debug!("Ignoring user message on data client");
            }
            PolymarketWsMessage::Reconnected => {
                log::info!("Polymarket WS reconnected");
            }
        }
    }

    fn handle_market_message(message: MarketWsMessage, ctx: &WsMessageContext) {
        match message {
            MarketWsMessage::Book(snap) => {
                let token_id = Ustr::from(snap.asset_id.as_str());
                let meta = match ctx.token_meta.get(&token_id) {
                    Some(m) => *m,
                    None => {
                        log::debug!("No instrument for token_id {token_id}");
                        return;
                    }
                };
                let instrument_id = meta.instrument_id;
                let ts_init = ctx.clock.get_time_ns();

                if ctx.active_delta_subs.contains(&instrument_id) {
                    match parse_book_snapshot(
                        &snap,
                        instrument_id,
                        meta.price_precision,
                        meta.size_precision,
                        ts_init,
                    ) {
                        Ok(deltas) => {
                            let mut book = ctx
                                .order_books
                                .entry(instrument_id)
                                .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

                            if let Err(e) = book.apply_deltas(&deltas) {
                                log::error!(
                                    "Failed to apply book snapshot for {instrument_id}: {e}"
                                );
                            }

                            let data: NautilusData = OrderBookDeltas_API::new(deltas).into();
                            if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                                log::error!("Failed to emit book deltas: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse book snapshot: {e}"),
                    }
                }

                if ctx.active_quote_subs.contains(&instrument_id) {
                    match parse_quote_from_snapshot(
                        &snap,
                        instrument_id,
                        meta.price_precision,
                        meta.size_precision,
                        ts_init,
                    ) {
                        Ok(Some(quote)) => {
                            Self::emit_quote_if_changed(ctx, instrument_id, quote);
                        }
                        Ok(None) => {}
                        Err(e) => log::error!("Failed to parse quote from snapshot: {e}"),
                    }
                }
            }

            MarketWsMessage::PriceChange(quotes) => {
                let ts_init = ctx.clock.get_time_ns();
                let ts_event = match parse_timestamp_ms(&quotes.timestamp) {
                    Ok(ts) => ts,
                    Err(e) => {
                        log::error!("Failed to parse price change timestamp: {e}");
                        return;
                    }
                };

                // Each change may belong to a different asset, so resolve per-change
                for change in &quotes.price_changes {
                    let token_id = Ustr::from(change.asset_id.as_str());
                    let meta = match ctx.token_meta.get(&token_id) {
                        Some(m) => *m,
                        None => {
                            log::debug!("No instrument for token_id {token_id}");
                            continue;
                        }
                    };
                    let instrument_id = meta.instrument_id;

                    if ctx.active_delta_subs.contains(&instrument_id) {
                        let per_asset = PolymarketQuotes {
                            market: quotes.market,
                            price_changes: vec![change.clone()],
                            timestamp: quotes.timestamp.clone(),
                        };

                        match parse_book_deltas(
                            &per_asset,
                            instrument_id,
                            meta.price_precision,
                            meta.size_precision,
                            ts_init,
                        ) {
                            Ok(deltas) => {
                                if let Some(mut book) = ctx.order_books.get_mut(&instrument_id)
                                    && let Err(e) = book.apply_deltas(&deltas)
                                {
                                    log::error!(
                                        "Failed to apply book deltas for {instrument_id}: {e}"
                                    );
                                }

                                let data: NautilusData = OrderBookDeltas_API::new(deltas).into();

                                if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to emit book deltas: {e}");
                                }
                            }
                            Err(e) => log::error!("Failed to parse book deltas: {e}"),
                        }
                    }

                    if ctx.active_quote_subs.contains(&instrument_id) {
                        // Clone and drop guard before emit to avoid DashMap deadlock
                        let last_quote = ctx.last_quotes.get(&instrument_id).map(|r| *r);

                        match parse_quote_from_price_change(
                            change,
                            instrument_id,
                            meta.price_precision,
                            meta.size_precision,
                            last_quote.as_ref(),
                            ts_event,
                            ts_init,
                        ) {
                            Ok(Some(quote)) => {
                                Self::emit_quote_if_changed(ctx, instrument_id, quote);
                            }
                            Ok(None) => {} // Missing best_bid/best_ask
                            Err(e) => {
                                log::error!("Failed to parse quote from price change: {e}");
                            }
                        }
                    }
                }
            }

            MarketWsMessage::LastTradePrice(trade) => {
                let token_id = Ustr::from(trade.asset_id.as_str());
                let meta = match ctx.token_meta.get(&token_id) {
                    Some(m) => *m,
                    None => {
                        log::debug!("No instrument for token_id {token_id}");
                        return;
                    }
                };
                let instrument_id = meta.instrument_id;

                if ctx.active_trade_subs.contains(&instrument_id) {
                    let ts_init = ctx.clock.get_time_ns();

                    match parse_trade_tick(
                        &trade,
                        instrument_id,
                        meta.price_precision,
                        meta.size_precision,
                        ts_init,
                    ) {
                        Ok(tick) => {
                            if let Err(e) = ctx
                                .data_sender
                                .send(DataEvent::Data(NautilusData::Trade(tick)))
                            {
                                log::error!("Failed to emit trade tick: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse trade tick: {e}"),
                    }
                }
            }

            MarketWsMessage::TickSizeChange(change) => {
                log::info!(
                    "Tick size changed for {}: {} -> {}",
                    change.asset_id,
                    change.old_tick_size,
                    change.new_tick_size
                );

                let token_id = Ustr::from(change.asset_id.as_str());
                let meta = match ctx.token_meta.get(&token_id) {
                    Some(m) => *m,
                    None => {
                        log::error!("No instrument for token_id {token_id}");
                        return;
                    }
                };

                let tick_size: rust_decimal::Decimal = match change.new_tick_size.parse() {
                    Ok(d) => d,
                    Err(e) => {
                        log::error!(
                            "Failed to parse new tick size '{}': {e}",
                            change.new_tick_size
                        );
                        return;
                    }
                };
                let new_price_precision = tick_size.scale() as u8;

                // Update hot-path precision
                ctx.token_meta.insert(
                    token_id,
                    TokenMeta {
                        price_precision: new_price_precision,
                        ..meta
                    },
                );

                // Rebuild and emit the full instrument to update cache.
                let instruments = ctx.instruments.load();
                if let Some(existing) = instruments.get(&meta.instrument_id) {
                    let ts_init = ctx.clock.get_time_ns();

                    match rebuild_instrument_with_tick_size(
                        existing,
                        &change.new_tick_size,
                        ts_init,
                        ts_init,
                    ) {
                        Ok(rebuilt) => {
                            ctx.instruments.insert(rebuilt.id(), rebuilt.clone());
                            if let Err(e) = ctx.data_sender.send(DataEvent::Instrument(rebuilt)) {
                                log::error!("Failed to emit rebuilt instrument: {e}");
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to rebuild instrument for tick size change: {e}");
                        }
                    }
                }
            }

            MarketWsMessage::NewMarket(nm) => {
                if !ctx.subscribe_new_markets {
                    log::trace!("Ignoring new market event (subscribe_new_markets=false)");
                    return;
                }

                if let Some(ref nf) = ctx.new_market_filter
                    && !nf.accept_new_market(&nm)
                {
                    log::debug!("New market slug={} rejected by new_market_filter", nm.slug);
                    return;
                }

                let gamma_client = ctx.gamma_client.clone();
                let filters = ctx.filters.clone();
                let token_meta = ctx.token_meta.clone();
                let instruments = ctx.instruments.clone();
                let data_sender = ctx.data_sender.clone();
                let clock = ctx.clock;
                let cancellation = ctx.cancellation_token.clone();
                let slug = nm.slug;
                let active = nm.active;

                get_runtime().spawn(async move {
                    let fetch = gamma_client
                        .request_instruments_by_slugs_with_retry(vec![slug.clone()]);

                    let result = tokio::select! {
                        r = fetch => r,
                        () = cancellation.cancelled() => {
                            log::debug!("New market fetch for '{slug}' cancelled during shutdown");
                            return;
                        }
                    };

                    match result {
                        Ok(new_instruments) => {
                            for inst in new_instruments {
                                if cancellation.is_cancelled() {
                                    log::debug!("New market processing cancelled during shutdown");
                                    return;
                                }

                                if !filters.iter().all(|f| f.accept(&inst)) {
                                    log::debug!("New market instrument {} filtered out", inst.id());
                                    continue;
                                }

                                cache_instrument(&instruments, &token_meta, &inst);

                                let instrument_id = inst.id();
                                if let Err(e) = data_sender.send(DataEvent::Instrument(inst)) {
                                    log::error!(
                                        "Failed to emit new market instrument {instrument_id}: {e}"
                                    );
                                }

                                // Emit instrument status based on WS active flag
                                let ts_now = clock.get_time_ns();
                                let action = if active {
                                    MarketStatusAction::Trading
                                } else {
                                    MarketStatusAction::PreOpen
                                };
                                let status = InstrumentStatus::new(
                                    instrument_id,
                                    action,
                                    ts_now,
                                    ts_now,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                );

                                if let Err(e) =
                                    data_sender.send(DataEvent::InstrumentStatus(status))
                                {
                                    log::error!(
                                        "Failed to emit instrument status for {instrument_id}: {e}"
                                    );
                                }
                            }
                        }
                        Err(e) => log::warn!(
                            "Failed to fetch instruments for new market slug '{slug}' after retries: {e}"
                        ),
                    }
                });
            }

            MarketWsMessage::MarketResolved(resolved) => {
                log::info!(
                    "Market resolved: {} winner={} ({})",
                    resolved.market,
                    resolved.winning_asset_id,
                    resolved.winning_outcome
                );

                let ts_init = ctx.clock.get_time_ns();
                let reason = Ustr::from(&format!(
                    "Winner: {} ({})",
                    resolved.winning_asset_id, resolved.winning_outcome
                ));

                for asset_id in &resolved.assets_ids {
                    let token_id = Ustr::from(asset_id.as_str());
                    if let Some(meta) = ctx.token_meta.get(&token_id) {
                        let status = InstrumentStatus::new(
                            meta.instrument_id,
                            MarketStatusAction::Close,
                            ts_init,
                            ts_init,
                            Some(reason),
                            None,
                            Some(false),
                            None,
                            None,
                        );

                        if let Err(e) = ctx.data_sender.send(DataEvent::InstrumentStatus(status)) {
                            log::error!(
                                "Failed to emit instrument status for {}: {e}",
                                meta.instrument_id
                            );
                        }
                    }
                }
            }

            MarketWsMessage::BestBidAsk(bba) => {
                log::trace!(
                    "best_bid_ask for {}: bid={} ask={}",
                    bba.asset_id,
                    bba.best_bid,
                    bba.best_ask
                );
            }
        }
    }

    fn emit_quote_if_changed(
        ctx: &WsMessageContext,
        instrument_id: InstrumentId,
        quote: QuoteTick,
    ) {
        // Compare prices and sizes only; timestamps always differ between messages
        let emit = !matches!(
            ctx.last_quotes.get(&instrument_id),
            Some(existing) if existing.bid_price == quote.bid_price
                && existing.ask_price == quote.ask_price
                && existing.bid_size == quote.bid_size
                && existing.ask_size == quote.ask_size
        );

        if emit {
            ctx.last_quotes.insert(instrument_id, quote);
            if let Err(e) = ctx
                .data_sender
                .send(DataEvent::Data(NautilusData::Quote(quote)))
            {
                log::error!("Failed to emit quote tick: {e}");
            }
        }
    }

    async fn await_tasks_with_timeout(&mut self, timeout: tokio::time::Duration) {
        for handle in self.tasks.drain(..) {
            let _ = tokio::time::timeout(timeout, handle).await;
        }
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for PolymarketDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*POLYMARKET_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting Polymarket data client: {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Polymarket data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting Polymarket data client: {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();

        for handle in self.tasks.drain(..) {
            handle.abort();
        }
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        self.cancellation_token = CancellationToken::new();

        log::info!("Connecting Polymarket data client");

        log::info!("Bootstrapping instruments from Gamma API...");
        self.bootstrap_instruments().await?;
        log::info!(
            "Bootstrap complete, {} instruments loaded",
            self.instruments.load().len(),
        );

        self.ws_client.connect().await?;

        if self.config.subscribe_new_markets {
            log::info!("Subscribing to new markets...");
            self.ws_client.subscribe_market(vec![]).await?;
        }

        let rx = self
            .ws_client
            .take_message_receiver()
            .ok_or_else(|| anyhow::anyhow!("WS message receiver not available after connect"))?;

        self.spawn_message_handler(rx);

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected Polymarket data client");

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        log::info!("Disconnecting Polymarket data client");

        self.cancellation_token.cancel();
        self.await_tasks_with_timeout(tokio::time::Duration::from_secs(5))
            .await;

        self.ws_client.disconnect().await?;

        self.is_connected.store(false, Ordering::Relaxed);
        log::info!("Disconnected Polymarket data client");

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.provider.http_client().clone();
        let filters = self.provider.filters();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = *POLYMARKET_VENUE;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match fetch_instruments(&http, &filters).await {
                Ok(instruments) => {
                    log::info!("Fetched {} instruments from Gamma API", instruments.len());

                    for instrument in &instruments {
                        cache_instrument(&instruments_cache, &token_meta, instrument);
                    }

                    let response = DataResponse::Instruments(InstrumentsResponse::new(
                        request_id,
                        client_id,
                        venue,
                        instruments,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instruments response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to fetch instruments from Gamma API: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let http = self.provider.http_client().clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            let condition_id = match extract_condition_id(&instrument_id) {
                Ok(cid) => cid,
                Err(e) => {
                    log::error!("Failed to extract condition_id for {instrument_id}: {e}");
                    return;
                }
            };

            let query_params = GetGammaMarketsParams {
                condition_ids: Some(condition_id),
                ..Default::default()
            };

            let instrument = match http.request_instruments_by_params(query_params).await {
                Ok(instruments) => instruments.into_iter().find(|i| i.id() == instrument_id),
                Err(e) => {
                    log::error!("Failed to fetch instrument {instrument_id} from Gamma API: {e}");
                    return;
                }
            };

            if let Some(inst) = instrument {
                cache_instrument(&instruments_cache, &token_meta, &inst);

                // Publish onto the data bus so other clients (e.g. the exec
                // client's token map) can update from the same fetch.
                if let Err(e) = sender.send(DataEvent::Instrument(inst.clone())) {
                    log::warn!("Failed to publish instrument {instrument_id}: {e}");
                }

                let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                    request_id,
                    client_id,
                    instrument_id,
                    inst,
                    datetime_to_unix_nanos(start),
                    datetime_to_unix_nanos(end),
                    clock.get_time_ns(),
                    params,
                )));

                if let Err(e) = sender.send(DataEvent::Response(response)) {
                    log::error!("Failed to send instrument response: {e}");
                }
            } else {
                log::error!("Instrument {instrument_id} not found on Polymarket");
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;

        let token_id = instrument.raw_symbol().as_str().to_string();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let clob_client = self.clob_public_client.clone();
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match clob_client
                .request_book_snapshot(instrument_id, &token_id, price_precision, size_precision)
                .await
                .context("failed to request book snapshot from Polymarket")
            {
                Ok(book) => {
                    let response = DataResponse::Book(BookResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        book,
                        None,
                        None,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send book snapshot response: {e}");
                    }
                }
                Err(e) => log::error!("Book snapshot request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;

        let condition_id = extract_condition_id(&instrument_id)?;
        let token_id = instrument.raw_symbol().as_str().to_string();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let limit = request.limit.map(|n| n.get() as u32);

        let data_api_client = self.data_api_client.clone();
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);

        get_runtime().spawn(async move {
            match data_api_client
                .request_trade_ticks(
                    instrument_id,
                    &condition_id,
                    &token_id,
                    price_precision,
                    size_precision,
                    limit,
                )
                .await
                .context("failed to request trades from Polymarket Data API")
            {
                Ok(trades) => {
                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        trades,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send trades response: {e}");
                    }
                }
                Err(e) => log::error!("Trade request failed for {instrument_id}: {e:?}"),
            }
        });

        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("subscribe_instruments: subscribed individually via data subscription methods");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!(
                "Polymarket only supports L2_MBP order book deltas, received {:?}",
                cmd.book_type
            );
        }

        let instrument_id = cmd.instrument_id;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        // Mark intent before routing so unsubscribe can race-safely clear it.
        self.active_delta_subs.insert(instrument_id);
        self.order_books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        log::debug!("Subscribed to book deltas for {instrument_id}");
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        self.active_quote_subs.insert(instrument_id);

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        log::debug!("Subscribed to quotes for {instrument_id}");
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        self.active_trade_subs.insert(instrument_id);

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        log::debug!("Subscribed to trades for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_delta_subs.remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from book deltas for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_quote_subs.remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from quotes for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_trade_subs.remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from trades for {instrument_id}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::AssetClass,
        identifiers::{InstrumentId, Symbol},
        instruments::BinaryOption,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::websocket::{client::WsSubscriptionHandle, handler::HandlerCommand};

    fn make_handle() -> (
        WsSubscriptionHandle,
        tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        (WsSubscriptionHandle::from_sender(tx), rx)
    }

    type ActiveSet = Arc<AtomicSet<InstrumentId>>;
    type OpenTokens = Arc<AtomicSet<Ustr>>;
    type WsMutex = Arc<tokio::sync::Mutex<()>>;

    fn make_state() -> (ActiveSet, ActiveSet, ActiveSet, OpenTokens, WsMutex) {
        (
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(tokio::sync::Mutex::new(())),
        )
    }

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET")
    }

    fn token_ustr() -> Ustr {
        Ustr::from("0xCOND-0xTOKEN")
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_subscribes_when_intent_present_and_ws_closed() {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        // Intent: quotes subscribed.
        let inst = instrument_id();
        quotes.insert(inst);

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes.clone(),
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(open.contains(&token_ustr()));

        match rx.try_recv().expect("expected SubscribeMarket command") {
            HandlerCommand::SubscribeMarket(ids) => {
                assert_eq!(ids, vec![inst.symbol.as_str().to_string()]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_unsubscribes_when_intent_absent_and_ws_open() {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        // WS currently open, but no caller wants it anymore.
        let inst = instrument_id();
        open.insert(token_ustr());

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(!open.contains(&token_ustr()));

        match rx.try_recv().expect("expected UnsubscribeMarket command") {
            HandlerCommand::UnsubscribeMarket(ids) => {
                assert_eq!(ids, vec![inst.symbol.as_str().to_string()]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[rstest]
    #[case::intent_matches_open(true, true, false)]
    #[case::no_intent_not_open(false, false, false)]
    #[tokio::test]
    async fn sync_ws_no_op_when_state_already_matches(
        #[case] want: bool,
        #[case] is_open_initial: bool,
        #[case] expect_command: bool,
    ) {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();

        if want {
            quotes.insert(inst);
        }

        if is_open_initial {
            open.insert(token_ustr());
        }

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        // State is preserved either way.
        assert_eq!(open.contains(&token_ustr()), is_open_initial);
        assert_eq!(rx.try_recv().is_ok(), expect_command);
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_rolls_back_open_tokens_on_send_failure() {
        // Drop the receiver so the channel send fails.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        drop(rx);
        let ws = WsSubscriptionHandle::from_sender(tx);

        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();
        quotes.insert(inst);

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        // Send failed, so the tracked WS state must be rolled back.
        assert!(!open.contains(&token_ustr()));
    }

    #[rstest]
    #[case::any_kind(true, false, false)]
    #[case::another_kind(false, true, false)]
    #[case::third_kind(false, false, true)]
    #[tokio::test]
    async fn sync_ws_opens_for_any_active_kind(#[case] q: bool, #[case] d: bool, #[case] t: bool) {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();

        if q {
            quotes.insert(inst);
        }

        if d {
            deltas.insert(inst);
        }

        if t {
            trades.insert(inst);
        }

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(open.contains(&token_ustr()));
        assert!(matches!(
            rx.try_recv(),
            Ok(HandlerCommand::SubscribeMarket(_))
        ));
    }

    fn stub_instrument(
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
    ) -> InstrumentAny {
        let price_precision = price_increment.precision;
        let size_precision = size_increment.precision;
        InstrumentAny::BinaryOption(BinaryOption::new(
            InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str()),
            Symbol::new(raw_symbol),
            AssetClass::Alternative,
            Currency::pUSD(),
            UnixNanos::default(),
            UnixNanos::from(u64::MAX),
            price_precision,
            size_precision,
            price_increment,
            size_increment,
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

    #[rstest]
    #[case::p3_s2("token-a", Price::from("0.001"), Quantity::from("0.01"))]
    #[case::p5_s4("token-b", Price::from("0.00001"), Quantity::from("0.0001"))]
    fn cache_instrument_writes_both_maps(
        #[case] raw_symbol: &str,
        #[case] price_increment: Price,
        #[case] size_increment: Quantity,
    ) {
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());
        let inst = stub_instrument(raw_symbol, price_increment, size_increment);
        let expected_id = inst.id();
        let expected_token = Ustr::from(raw_symbol);
        let expected_price_precision = price_increment.precision;
        let expected_size_precision = size_increment.precision;

        cache_instrument(&instruments, &token_meta, &inst);

        let loaded = instruments.load();
        let cached = loaded
            .get(&expected_id)
            .expect("instrument inserted into live cache");
        assert_eq!(cached.id(), expected_id);
        assert_eq!(cached.raw_symbol().as_str(), raw_symbol);

        let meta = token_meta
            .get(&expected_token)
            .expect("token_meta inserted for raw_symbol");
        assert_eq!(meta.instrument_id, expected_id);
        assert_eq!(meta.price_precision, expected_price_precision);
        assert_eq!(meta.size_precision, expected_size_precision);
    }

    #[rstest]
    fn cache_instrument_overwrites_precisions_on_second_call() {
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());
        let raw_symbol = "token-overwrite";

        let first = stub_instrument(raw_symbol, Price::from("0.01"), Quantity::from("0.1"));
        cache_instrument(&instruments, &token_meta, &first);

        let second = stub_instrument(raw_symbol, Price::from("0.0001"), Quantity::from("0.001"));
        cache_instrument(&instruments, &token_meta, &second);

        let meta = token_meta
            .get(&Ustr::from(raw_symbol))
            .expect("token_meta present after overwrite");
        assert_eq!(meta.price_precision, 4);
        assert_eq!(meta.size_precision, 3);
        assert_eq!(token_meta.len(), 1);
        assert_eq!(instruments.load().len(), 1);
    }

    #[rstest]
    fn cache_instrument_maintains_dual_cache_invariant() {
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());

        let samples = [
            stub_instrument("token-1", Price::from("0.001"), Quantity::from("0.01")),
            stub_instrument("token-2", Price::from("0.0001"), Quantity::from("0.01")),
            stub_instrument("token-3", Price::from("0.00001"), Quantity::from("0.001")),
        ];

        for inst in &samples {
            cache_instrument(&instruments, &token_meta, inst);
        }

        let loaded = instruments.load();
        assert_eq!(loaded.len(), samples.len());
        for inst in loaded.values() {
            let token_id = Ustr::from(inst.raw_symbol().as_str());
            let meta = token_meta
                .get(&token_id)
                .unwrap_or_else(|| panic!("missing token_meta for {token_id}"));
            assert_eq!(meta.instrument_id, inst.id());
        }
    }
}
