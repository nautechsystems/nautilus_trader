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
//!
//! Tick-size changes are handled as book epoch transitions: the local order
//! book is dropped, incremental `price_change` deltas are gated through
//! `pending_snapshot_after_tick_change`, and the gate clears once the next
//! venue snapshot reseeds the book under the new precision. The quote arm of
//! `price_change` stays open through the gap because each payload carries
//! `best_bid` / `best_ask` on the new grid; `last_quotes` is preserved so the
//! unchanged side's size carries forward. See
//! `docs/integrations/polymarket.md` for the full description.

mod dispatch;
mod instruments;
mod requests;
mod subscriptions;

use std::{
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use nautilus_common::{
    clients::DataClient,
    live::{get_runtime, runner::get_data_event_sender},
    messages::{
        DataEvent,
        data::{
            RequestBookSnapshot, RequestCustomData, RequestInstrument, RequestInstruments,
            RequestTrades, SubscribeBookDeltas, SubscribeCustomData, SubscribeInstruments,
            SubscribeQuotes, SubscribeTrades, UnsubscribeBookDeltas, UnsubscribeCustomData,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    msgbus::{self, TypedHandler},
    providers::InstrumentProvider,
};
use nautilus_core::{
    AtomicMap, AtomicSet,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::QuoteTick,
    enums::BookType,
    events::PositionEvent,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use self::{
    dispatch::{WsMessageContext, handle_ws_message},
    instruments::{
        TokenMeta, cache_and_publish_instruments, cache_instrument, refresh_scoped_instruments,
    },
    requests::{
        request_book_snapshot, request_data, request_instrument, request_instruments,
        request_trades,
    },
    subscriptions::{resolve_token_id_from, sync_ws_subscription_async},
};
use crate::{
    common::consts::{GAMMA_CONDITION_IDS_BATCH_SIZE, POLYMARKET_VENUE},
    config::PolymarketDataClientConfig,
    data_types::register_polymarket_custom_data,
    filters::InstrumentFilter,
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient, query::GetGammaMarketsParams,
    },
    providers::{PolymarketInstrumentProvider, extract_condition_id},
    resolve::{
        ResolveBatchErrorMode, ResolveWatchEntry, ResolveWatchSelectionMode,
        collect_resolve_watch_selection, fetch_and_apply_resolutions_by_condition_ids,
        pause_resolve_watch_entries, update_resolve_watchlist_from_position_event,
    },
    rtds::{PolymarketRtdsFeed, is_supported_rtds_data_type},
    websocket::{client::PolymarketWebSocketClient, messages::PolymarketWsMessage},
};

const NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP: usize = 64;
pub(super) const NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS: usize = 1;
pub(super) const NEW_MARKET_EMPTY_RECHECK_DELAY: Duration = Duration::from_millis(500);
fn clamp_new_market_fetch_max_concurrency(value: usize) -> usize {
    value.clamp(1, NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP)
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
    resolve_poll_watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    resolve_watch_apply_mutex: Arc<StdMutex<()>>,
    pending_snapshot_after_tick_change: Arc<AtomicSet<InstrumentId>>,
    new_market_inflight_keys: Arc<DashMap<String, ()>>,
    new_market_fetch_semaphore: Arc<tokio::sync::Semaphore>,
    ws_open_tokens: Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    pending_auto_loads: Arc<StdMutex<AHashSet<InstrumentId>>>,
    auto_load_scheduled: Arc<AtomicBool>,
    position_event_handler: Option<TypedHandler<PositionEvent>>,
    rtds_feed: PolymarketRtdsFeed,
}

impl PolymarketDataClient {
    /// Creates a new [`PolymarketDataClient`].
    pub fn new(
        client_id: ClientId,
        mut config: PolymarketDataClientConfig,
        gamma_client: PolymarketGammaHttpClient,
        clob_public_client: PolymarketClobPublicClient,
        data_api_client: PolymarketDataApiHttpClient,
        ws_client: PolymarketWebSocketClient,
    ) -> Self {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let provider =
            PolymarketInstrumentProvider::new(gamma_client, config.instrument_config.clone());
        let configured_fetch_max_concurrency = config.new_market_fetch_max_concurrency;
        let fetch_max_concurrency =
            clamp_new_market_fetch_max_concurrency(configured_fetch_max_concurrency);

        if configured_fetch_max_concurrency == 0 {
            log::warn!(
                "PolymarketDataClientConfig.new_market_fetch_max_concurrency=0 is invalid, clamping to 1"
            );
        } else if configured_fetch_max_concurrency > NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP {
            log::warn!(
                "PolymarketDataClientConfig.new_market_fetch_max_concurrency={configured_fetch_max_concurrency} exceeds cap {NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP}, clamping",
            );
        }
        config.new_market_fetch_max_concurrency = fetch_max_concurrency;

        let rtds_url = config.rtds_url();
        let rtds_transport_backend = config.transport_backend;
        let rtds_data_sender = data_sender.clone();

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
            resolve_poll_watchlist: Arc::new(AtomicMap::new()),
            resolve_watch_apply_mutex: Arc::new(StdMutex::new(())),
            pending_snapshot_after_tick_change: Arc::new(AtomicSet::new()),
            new_market_inflight_keys: Arc::new(DashMap::new()),
            new_market_fetch_semaphore: Arc::new(tokio::sync::Semaphore::new(
                fetch_max_concurrency,
            )),
            ws_open_tokens: Arc::new(AtomicSet::new()),
            ws_sub_mutex: Arc::new(tokio::sync::Mutex::new(())),
            pending_auto_loads: Arc::new(StdMutex::new(AHashSet::new())),
            auto_load_scheduled: Arc::new(AtomicBool::new(false)),
            position_event_handler: None,
            rtds_feed: PolymarketRtdsFeed::new(
                rtds_url,
                rtds_transport_backend,
                clock,
                rtds_data_sender,
            ),
        }
    }

    /// Returns a reference to the client configuration.
    #[must_use]
    pub fn config(&self) -> &PolymarketDataClientConfig {
        &self.config
    }

    fn ensure_position_event_subscription(&mut self) {
        if self.position_event_handler.is_some() {
            return;
        }

        let watchlist = self.resolve_poll_watchlist.clone();
        let instruments = self.instruments.clone();
        let handler = TypedHandler::from(move |event: &PositionEvent| {
            update_resolve_watchlist_from_position_event(&watchlist, &instruments, event);
        });

        msgbus::subscribe_position_events("events.position.*".into(), handler.clone(), Some(10));
        self.position_event_handler = Some(handler);
    }

    fn clear_position_event_subscription(&mut self) {
        if let Some(handler) = self.position_event_handler.take() {
            msgbus::unsubscribe_position_events("events.position.*".into(), &handler);
        }
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
        resolve_token_id_from(&self.instruments, instrument_id)
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

    #[cfg(test)]
    fn handle_market_message(
        message: crate::websocket::messages::MarketWsMessage,
        ctx: &WsMessageContext,
    ) {
        dispatch::handle_market_message(message, ctx);
    }

    #[cfg(test)]
    fn new_market_dedupe_key(nm: &crate::websocket::messages::PolymarketNewMarket) -> String {
        dispatch::new_market_dedupe_key(nm)
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

    fn drop_local_book_state_if_unwanted(&self, instrument_id: InstrumentId) {
        // Stale book/quote leaks across resubscribes
        if self.active_quote_subs.contains(&instrument_id)
            || self.active_delta_subs.contains(&instrument_id)
        {
            return;
        }
        self.order_books.remove(&instrument_id);
        self.last_quotes.remove(&instrument_id);
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
        let max_retries = self.config.auto_load_max_retries;
        let base_secs = self.config.auto_load_retry_delay_initial_secs;
        let max_secs = self.config.auto_load_retry_delay_max_secs;
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
            // Coalesce concurrent misses into one Gamma call.
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(debounce_ms)) => {}
                () = cancellation.cancelled() => {
                    scheduled.store(false, Ordering::Release);
                    return;
                }
            }

            // Drain pending and release `scheduled` so new misses spawn a fresh
            // task in parallel rather than piggybacking on this batch's budget.
            let mut batch: AHashSet<InstrumentId> = {
                let mut guard = pending.lock().expect("pending_auto_loads mutex poisoned");
                let snapshot = guard.iter().copied().collect();
                guard.clear();
                snapshot
            };
            scheduled.store(false, Ordering::Release);

            if batch.is_empty() {
                return;
            }

            log::info!(
                "Auto-loading {} missing instrument(s): {batch:?}",
                batch.len(),
            );

            for attempt in 0..=max_retries {
                if cancellation.is_cancelled() {
                    return;
                }

                // Drop entries the user has since unsubscribed from.
                batch.retain(|id| {
                    active_quote_subs.contains(id)
                        || active_delta_subs.contains(id)
                        || active_trade_subs.contains(id)
                });

                if batch.is_empty() {
                    return;
                }

                let mut condition_ids: Vec<String> = batch
                    .iter()
                    .filter_map(|id| extract_condition_id(id).ok())
                    .collect();
                condition_ids.sort();
                condition_ids.dedup();

                if condition_ids.is_empty() {
                    log::error!(
                        "Auto-load aborted: no condition_ids could be extracted from {} entries",
                        batch.len(),
                    );
                    return;
                }

                // Gamma caps `condition_ids=` filters at ~100; chunk and merge.
                let mut loaded: Vec<InstrumentAny> = Vec::new();
                let mut transient: AHashSet<String> = AHashSet::new();
                let mut batch_returned_any = false;
                let mut chunk_failed = false;

                for chunk in condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
                    let params = GetGammaMarketsParams {
                        condition_ids: Some(chunk.join(",")),
                        ..Default::default()
                    };

                    match http
                        .request_instruments_by_params_with_transient(params)
                        .await
                    {
                        Ok((insts, trans)) => {
                            batch_returned_any |= !insts.is_empty() || !trans.is_empty();
                            loaded.extend(insts);
                            transient.extend(trans);
                        }
                        Err(e) => {
                            log::error!(
                                "Auto-load batch failed for chunk of {} condition_id(s): {e:?}",
                                chunk.len(),
                            );
                            chunk_failed = true;
                            break;
                        }
                    }
                }

                // A chunk failure leaves the batch's state unknown; count it
                // against the retry budget instead of dropping the subscription.
                let next_batch: AHashSet<InstrumentId> = if chunk_failed {
                    batch.clone()
                } else {
                    for inst in loaded {
                        if !filters.iter().all(|f| f.accept(&inst)) {
                            log::debug!("Auto-loaded instrument {} filtered out", inst.id());
                            continue;
                        }

                        cache_instrument(&instruments, &token_meta, &inst);

                        let instrument_id = inst.id();
                        if let Err(e) = data_sender.send(DataEvent::Instrument(inst)) {
                            log::error!(
                                "Failed to emit auto-loaded instrument {instrument_id}: {e}"
                            );
                        }
                    }

                    // Snapshot loaded keys so the arc-swap Guard does not span
                    // the WS reconciliation awaits below.
                    let loaded_ids: AHashSet<InstrumentId> = {
                        let cache = instruments.load();
                        batch
                            .iter()
                            .filter(|id| cache.contains_key(id))
                            .copied()
                            .collect()
                    };
                    let mut next: AHashSet<InstrumentId> = AHashSet::new();

                    for id in &batch {
                        let cid = match extract_condition_id(id) {
                            Ok(c) => c,
                            Err(_) => continue,
                        };

                        if loaded_ids.contains(id) {
                            if let Ok(token_id) = resolve_token_id_from(&instruments, *id) {
                                sync_ws_subscription_async(
                                    *id,
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
                        } else if transient.contains(&cid) {
                            // CLOB still hydrating: retry within the budget.
                            next.insert(*id);
                        } else {
                            // Absent from bulk response (same observable state as a
                            // 404 in the single-market path): also transient.
                            next.insert(*id);
                        }
                    }
                    next
                };

                if next_batch.is_empty() {
                    return;
                }

                if attempt >= max_retries {
                    let absent_reason = if batch_returned_any {
                        "Gamma returned no market for condition_id"
                    } else {
                        "Gamma returned no markets for batch query"
                    };

                    for id in &next_batch {
                        let reason = if chunk_failed {
                            "Gamma fetch failed"
                        } else if extract_condition_id(id)
                            .is_ok_and(|condition_id| transient.contains(&condition_id))
                        {
                            "no usable token_id (CLOB lifecycle race)"
                        } else {
                            absent_reason
                        };

                        log::error!(
                            "Cannot find instrument for {id}: {reason} after {max_retries} retries"
                        );
                    }
                    return;
                }

                let delay =
                    crate::common::retry::auto_load_retry_delay(attempt, base_secs, max_secs);
                let kind = if chunk_failed {
                    "chunk failure"
                } else {
                    "transient"
                };
                log::info!(
                    "Auto-load retry {}/{} for {} {kind} instrument(s) in {:.1}s",
                    attempt + 1,
                    max_retries,
                    next_batch.len(),
                    delay.as_secs_f64(),
                );

                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    () = cancellation.cancelled() => return,
                }

                batch = next_batch;
            }
        });
    }

    async fn bootstrap_instruments(&mut self) -> anyhow::Result<()> {
        self.provider.initialize(false).await?;

        let total = cache_and_publish_instruments(
            &self.instruments,
            &self.token_meta,
            &self.data_sender,
            self.provider
                .store()
                .list_all()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
        );

        log::info!("Published {total} Polymarket instruments to data engine");
        Ok(())
    }

    fn spawn_instrument_refresh_task(&mut self) {
        let Some(interval_mins) = self.config.update_instruments_interval_mins else {
            return;
        };

        if interval_mins == 0 || self.config.instrument_config.is_none() {
            return;
        }

        let interval = Duration::from_secs(interval_mins.saturating_mul(60));
        let cancellation = self.cancellation_token.clone();
        let http_client = self.provider.http_client().clone();
        let instrument_config = self.config.instrument_config.clone();
        let filters = self.provider.filters();
        let instruments_cache = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let data_sender = self.data_sender.clone();

        let handle = get_runtime().spawn(async move {
            log::debug!("Polymarket instrument refresh task started");

            loop {
                tokio::select! {
                    () = tokio::time::sleep(interval) => {}
                    () = cancellation.cancelled() => {
                        log::debug!("Polymarket instrument refresh task cancelled");
                        break;
                    }
                }

                match refresh_scoped_instruments(
                    http_client.clone(),
                    instrument_config.clone(),
                    filters.clone(),
                    &instruments_cache,
                    &token_meta,
                    &data_sender,
                )
                .await
                {
                    Ok(total) => {
                        if total > 0 {
                            log::info!(
                                "Refreshed {total} Polymarket instruments into the live cache"
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to refresh Polymarket instruments: {e}");
                    }
                }
            }

            log::debug!("Polymarket instrument refresh task ended");
        });

        self.tasks.push(handle);
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
            clob_public_client: self.clob_public_client.clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            new_market_inflight_keys: self.new_market_inflight_keys.clone(),
            new_market_fetch_semaphore: self.new_market_fetch_semaphore.clone(),
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
                            Some(msg) => handle_ws_message(msg, &ctx),
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

    fn spawn_resolve_poll_task(&mut self) {
        if !self.config.resolve_poll_enabled {
            log::info!("Polymarket resolve polling disabled");
            return;
        }

        let cancellation = self.cancellation_token.clone();
        let gamma_client = self.provider.http_client().clone();
        let clob_public_client = self.clob_public_client.clone();
        let clock = self.clock;
        let interval_secs = self.config.resolve_poll_interval_secs.max(1);
        let grace_secs = self.config.resolve_poll_grace_secs;
        let max_wait_secs = self.config.resolve_poll_max_wait_secs.max(grace_secs);

        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            gamma_client: gamma_client.clone(),
            clob_public_client: clob_public_client.clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            new_market_inflight_keys: self.new_market_inflight_keys.clone(),
            new_market_fetch_semaphore: self.new_market_fetch_semaphore.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            cancellation_token: cancellation.clone(),
        };

        let watchlist = self.resolve_poll_watchlist.clone();

        let handle = get_runtime().spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    _ = interval.tick() => {
                        let now_ns = clock.get_time_ns();
                        let snapshot = watchlist.load();
                        let watched_conditions = snapshot.len();
                        let watched_instruments = snapshot
                            .values()
                            .map(|entry| entry.tracked.len())
                            .sum::<usize>();
                        let selection = collect_resolve_watch_selection(
                            &snapshot,
                            now_ns,
                            grace_secs,
                            max_wait_secs,
                            ResolveWatchSelectionMode::AutoPoll,
                        );
                        drop(snapshot);

                        if !selection.pause_condition_ids.is_empty() {
                            log::warn!(
                                "Polymarket resolve poll paused {} timed-out condition(s) for manual recovery",
                                selection.pause_condition_ids.len(),
                            );
                        }

                        if !selection.condition_ids.is_empty()
                            || !selection.pause_condition_ids.is_empty()
                        {
                            log::info!(
                                "Polymarket resolve poll selected={} watched_conditions={} watched_instruments={} skipped_not_expired={} timed_out={} paused={} min_ready_in_secs={:?}",
                                selection.condition_ids.len(),
                                watched_conditions,
                                watched_instruments,
                                selection.skipped_not_expired,
                                selection.timed_out_watchlist,
                                selection.paused_watchlist,
                                selection.min_ready_in_secs,
                            );
                        } else if selection.timed_out_watchlist > 0
                            && selection.paused_watchlist > 0
                        {
                            log::debug!(
                                "Polymarket resolve poll waiting for manual recovery: timed_out={} paused={} watched_conditions={watched_conditions}",
                                selection.timed_out_watchlist,
                                selection.paused_watchlist,
                            );
                        }

                        pause_resolve_watch_entries(&watchlist, &selection.pause_condition_ids);

                        let _ = fetch_and_apply_resolutions_by_condition_ids(
                            &gamma_client,
                            &clob_public_client,
                            &ctx.resolve_context(),
                            &selection.condition_ids,
                            ResolveBatchErrorMode::Continue,
                        )
                        .await;
                    }
                }
            }
        });

        self.tasks.push(handle);
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
        self.ensure_position_event_subscription();
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Polymarket data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        self.clear_position_event_subscription();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting Polymarket data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);

        // Hard reset contract: discard all retained reconnect replay state from
        // the previous generation. Callers must rebuild instrument/data
        // subscriptions after connect().
        // Stop the WS handler path even when reset is called without a graceful disconnect.
        self.ws_client.abort();
        self.ws_client.clear_reconnect_state();
        self.rtds_feed.abort();
        self.resolve_poll_watchlist.store(ahash::AHashMap::new());
        self.clear_position_event_subscription();

        for handle in self.tasks.drain(..) {
            handle.abort();
        }

        self.instruments.store(AHashMap::new());
        self.token_meta.clear();
        self.order_books.clear();
        self.last_quotes.clear();

        self.active_quote_subs = Arc::new(AtomicSet::new());
        self.active_delta_subs = Arc::new(AtomicSet::new());
        self.active_trade_subs = Arc::new(AtomicSet::new());
        self.pending_snapshot_after_tick_change = Arc::new(AtomicSet::new());
        self.new_market_inflight_keys = Arc::new(DashMap::new());
        self.ws_open_tokens = Arc::new(AtomicSet::new());
        self.rtds_feed = PolymarketRtdsFeed::new(
            self.config.rtds_url(),
            self.config.transport_backend,
            self.clock,
            self.data_sender.clone(),
        );

        self.pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned")
            .clear();
        self.auto_load_scheduled.store(false, Ordering::Release);

        self.cancellation_token = CancellationToken::new();
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
        self.ensure_position_event_subscription();
        register_polymarket_custom_data();

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
        self.spawn_instrument_refresh_task();
        self.spawn_resolve_poll_task();

        if self.rtds_feed.has_subscriptions() {
            self.rtds_feed.connect().await?;
        }

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
        self.rtds_feed.disconnect().await;

        self.is_connected.store(false, Ordering::Relaxed);
        self.clear_position_event_subscription();
        log::info!("Disconnected Polymarket data client");

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn request_data(&self, request: RequestCustomData) -> anyhow::Result<()> {
        request_data(self, request);
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        request_instruments(self, request);
        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        request_instrument(self, request);
        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        request_book_snapshot(self, request)
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        request_trades(self, request)
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("subscribe_instruments: subscribed individually via data subscription methods");
        Ok(())
    }

    fn subscribe(&mut self, cmd: SubscribeCustomData) -> anyhow::Result<()> {
        if !is_supported_rtds_data_type(&cmd.data_type) {
            log::debug!(
                "Ignoring unsupported Polymarket custom data subscription: {}",
                cmd.data_type
            );
            return Ok(());
        }

        log::debug!(
            "Tracking Polymarket RTDS custom data subscription: {}",
            cmd.data_type
        );
        let Some(wire) = self.rtds_feed.track_subscribe(cmd.data_type)? else {
            return Ok(());
        };

        if !self.is_connected() {
            return Ok(());
        }

        let feed = self.rtds_feed.clone();
        get_runtime().spawn(async move {
            if let Err(e) = feed.subscribe_live(wire).await {
                log::error!("Failed to subscribe RTDS custom data: {e}");
            }
        });

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
        self.pending_snapshot_after_tick_change
            .remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.drop_local_book_state_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from book deltas for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_quote_subs.remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.drop_local_book_state_if_unwanted(instrument_id);
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

    fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        if !is_supported_rtds_data_type(&cmd.data_type) {
            log::debug!(
                "Ignoring unsupported Polymarket custom data unsubscription: {}",
                cmd.data_type
            );
            return Ok(());
        }

        log::debug!(
            "Tracking Polymarket RTDS custom data unsubscription: {}",
            cmd.data_type
        );
        let Some(wire) = self.rtds_feed.track_unsubscribe(&cmd.data_type)? else {
            return Ok(());
        };

        if !self.is_connected() {
            return Ok(());
        }

        let feed = self.rtds_feed.clone();
        get_runtime().spawn(async move {
            if let Err(e) = feed.unsubscribe_live(wire).await {
                log::error!("Failed to unsubscribe RTDS custom data: {e}");
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests;
