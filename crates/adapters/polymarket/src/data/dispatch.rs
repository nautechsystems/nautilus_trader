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

//! WebSocket market-message dispatch for the Polymarket data client.
//!
//! With `compute_effective_deltas` enabled, book snapshots emit only the net
//! diff when a maintained local book exists (an empty diff emits nothing).
//! Incremental `price_change` batches remain wire-faithful and keep that book
//! current. After an epoch reset, the next snapshot seeds the book unchanged.
//!
//! Tick-size changes are handled as book epoch transitions: the local order
//! book is dropped, incremental `price_change` deltas are gated through
//! `pending_snapshot_after_tick_change`, and the gate clears once the next
//! venue snapshot reseeds the book under the new precision. The quote arm of
//! `price_change` stays open through the gap because each payload carries
//! `best_bid` / `best_ask` on the new grid; `last_quotes` is preserved so the
//! unchanged side's size carries forward. See
//! `docs/integrations/polymarket.md` for the full description.
//!
//! A snapshot hash mismatch reuses the same book-delta gate until a later
//! valid snapshot arrives. The mismatched snapshot is not parsed, applied, or
//! emitted as a quote.

use std::sync::{Arc, Mutex as StdMutex, Weak};

use ahash::{AHashMap, AHashSet};
use dashmap::{DashMap, mapref::entry::Entry};
use nautilus_common::{live::task::TaskHandles, messages::DataEvent};
use nautilus_core::{AtomicMap, AtomicSet, time::AtomicTime};
use nautilus_model::{
    data::{Data as NautilusData, InstrumentStatus, OrderBookDeltas, QuoteTick},
    enums::{BookType, MarketStatusAction},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    NEW_MARKET_EMPTY_RECHECK_DELAY, NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS,
    effective_deltas::apply_snapshot_and_diff,
    instruments::{TokenMeta, apply_live_instrument},
    spawn_task,
};
use crate::{
    filters::InstrumentFilter,
    http::{
        gamma::PolymarketGammaHttpClient, parse::rebuild_instrument_with_tick_size,
        query::GetGammaMarketsParams,
    },
    resolve::{ResolveContext, ResolveWatchEntry, apply_condition_resolution},
    rtds::PolymarketRtdsFeed,
    websocket::{
        messages::{MarketWsMessage, PolymarketNewMarket, PolymarketQuote, PolymarketWsMessage},
        parse::{
            parse_book_deltas, parse_book_snapshot, parse_quote_from_best_bid_ask,
            parse_quote_from_price_change, parse_quote_from_snapshot, parse_timestamp_ms,
            parse_trade_tick, verify_book_snapshot_hash,
        },
    },
};

struct NewMarketInflightGuard {
    inflight_keys: Arc<DashMap<String, ()>>,
    key: String,
}

impl NewMarketInflightGuard {
    fn new(inflight_keys: Arc<DashMap<String, ()>>, key: String) -> Self {
        Self { inflight_keys, key }
    }
}

impl Drop for NewMarketInflightGuard {
    fn drop(&mut self) {
        self.inflight_keys.remove(&self.key);
    }
}

pub(super) struct WsMessageContext {
    pub(super) clock: &'static AtomicTime,
    pub(super) data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    pub(super) token_meta: Arc<DashMap<Ustr, TokenMeta>>,
    pub(super) instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    pub(super) gamma_client: PolymarketGammaHttpClient,
    pub(super) filters: Vec<Arc<dyn InstrumentFilter>>,
    pub(super) order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    pub(super) last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    pub(super) active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) closed_condition_ids: Arc<StdMutex<AHashSet<String>>>,
    pub(super) resolve_poll_watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    pub(super) resolve_watch_apply_mutex: Arc<StdMutex<()>>,
    pub(super) pending_snapshot_after_tick_change: Arc<AtomicSet<InstrumentId>>,
    pub(super) new_market_inflight_keys: Arc<DashMap<String, ()>>,
    pub(super) new_market_fetch_semaphore: Arc<tokio::sync::Semaphore>,
    pub(super) tasks: Weak<TaskHandles>,
    pub(super) task_registration: Weak<StdMutex<()>>,
    pub(super) rtds_feed: PolymarketRtdsFeed,
    pub(super) subscribe_new_markets: bool,
    pub(super) new_market_filter: Option<Arc<dyn InstrumentFilter>>,
    pub(super) drop_quotes_missing_side: bool,
    pub(super) compute_effective_deltas: bool,
    pub(super) cancellation_token: CancellationToken,
}

// The lock releases before the caller dispatches, so no adapter state spans publication
fn is_terminal_condition(ctx: &WsMessageContext, instrument_id: InstrumentId) -> bool {
    crate::providers::extract_condition_id(&instrument_id).is_ok_and(|condition_id| {
        crate::data::runtime::is_condition_closed(&ctx.closed_condition_ids, &condition_id)
    })
}

impl WsMessageContext {
    pub(super) fn resolve_context(&self) -> ResolveContext {
        ResolveContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            watchlist: self.resolve_poll_watchlist.clone(),
            apply_mutex: self.resolve_watch_apply_mutex.clone(),
        }
    }
}

fn new_market_dedupe_key(nm: &PolymarketNewMarket) -> String {
    let condition_id = nm.condition_id.trim();
    if !condition_id.is_empty() {
        return format!("cond:{condition_id}");
    }
    let market_id = nm.market.as_str().trim();
    if !market_id.is_empty() {
        return format!("market:{market_id}");
    }
    format!("slug:{}", nm.slug.trim())
}

fn new_market_fetch_condition_id(nm: &PolymarketNewMarket) -> Option<String> {
    let condition_id = nm.condition_id.trim();
    if !condition_id.is_empty() {
        return Some(condition_id.to_string());
    }

    let market_id = nm.market.as_str().trim();
    if !market_id.is_empty() {
        return Some(market_id.to_string());
    }

    None
}

pub(super) fn handle_ws_message(message: PolymarketWsMessage, ctx: &WsMessageContext) {
    match message {
        PolymarketWsMessage::Market(market_msg) => {
            handle_market_message(market_msg, ctx);
        }
        PolymarketWsMessage::User(_) => {
            log::debug!("Ignoring user message on data client");
        }
        PolymarketWsMessage::Reconnected => {
            log::info!("Polymarket WS reconnected");
            if ctx.cancellation_token.is_cancelled() {
                log::debug!("Skipping RTDS recovery because data client is cancelling");
                return;
            }

            if !ctx.rtds_feed.needs_connection_recovery() {
                log::debug!("Skipping RTDS recovery because RTDS connection is still healthy");
                return;
            }

            ctx.rtds_feed
                .request_reconcile(crate::rtds::ReconcileReason::EnsureConnected);
        }
    }
}

fn handle_market_message(message: MarketWsMessage, ctx: &WsMessageContext) {
    match message {
        MarketWsMessage::Book(snap) => {
            let token_id = snap.asset_id;
            let meta = match ctx.token_meta.get(&token_id) {
                Some(m) => *m,
                None => {
                    log::debug!("No instrument for token_id {token_id}");
                    return;
                }
            };

            let instrument_id = meta.instrument_id;
            if is_terminal_condition(ctx, instrument_id) {
                return;
            }

            if let Some(book) = ctx.order_books.get(&instrument_id) {
                let ts_event = match parse_timestamp_ms(&snap.timestamp) {
                    Ok(ts) => ts,
                    Err(e) => {
                        log::error!("Failed to parse book snapshot timestamp: {e}");
                        return;
                    }
                };

                if ts_event < book.ts_last {
                    log::warn!(
                        "Ignoring stale book snapshot for {instrument_id}: ts_event={ts_event} < ts_last={}",
                        book.ts_last,
                    );
                    return;
                }
            }

            if let Err(e) =
                verify_book_snapshot_hash(&snap, meta.min_order_size.as_deref(), meta.neg_risk)
            {
                log::error!("Rejected book snapshot for {instrument_id}: {e}");
                if ctx.active_delta_subs.contains(&instrument_id) {
                    ctx.pending_snapshot_after_tick_change.insert(instrument_id);
                }
                return;
            }

            let ts_init = ctx.clock.get_time_ns();
            let mut snapshot_accepted = false;

            if ctx.active_delta_subs.contains(&instrument_id) {
                match parse_book_snapshot(
                    &snap,
                    instrument_id,
                    meta.price_precision,
                    meta.size_precision,
                    ts_init,
                ) {
                    Ok(deltas) => {
                        let emit = if ctx.compute_effective_deltas {
                            match ctx.order_books.entry(instrument_id) {
                                Entry::Occupied(mut entry) => {
                                    match apply_snapshot_and_diff(entry.get_mut(), &deltas) {
                                        Ok(effective) => {
                                            snapshot_accepted = true;
                                            effective
                                        }
                                        Err(e) => {
                                            log::error!(
                                                "Failed to apply book snapshot for {instrument_id}: {e}"
                                            );
                                            None
                                        }
                                    }
                                }
                                Entry::Vacant(entry) => {
                                    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
                                    match book.apply_deltas(&deltas) {
                                        Ok(()) => {
                                            entry.insert(book);
                                            snapshot_accepted = true;
                                        }
                                        Err(e) => log::error!(
                                            "Failed to apply book snapshot for {instrument_id}: {e}"
                                        ),
                                    }
                                    Some(deltas)
                                }
                            }
                        } else {
                            snapshot_accepted = true;
                            Some(deltas)
                        };

                        if let Some(deltas) = emit {
                            let data: NautilusData = deltas.into();
                            if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                                log::error!("Failed to emit book deltas: {e}");
                            }
                        }
                    }
                    Err(e) => log::error!("Failed to parse book snapshot: {e}"),
                }
            }

            if ctx.active_quote_subs.contains(&instrument_id) {
                let price_increment = {
                    let instruments = ctx.instruments.load();
                    let Some(instrument) = instruments.get(&instrument_id) else {
                        log::error!("No instrument for {instrument_id}");
                        return;
                    };
                    instrument.price_increment()
                };

                match parse_quote_from_snapshot(
                    &snap,
                    instrument_id,
                    meta.price_precision,
                    meta.size_precision,
                    price_increment,
                    ctx.drop_quotes_missing_side,
                    ts_init,
                ) {
                    Ok(Some(quote)) => emit_quote_if_changed(ctx, instrument_id, quote),
                    Ok(None) => {}
                    Err(e) => log::error!("Failed to parse quote from snapshot: {e}"),
                }
            }

            if snapshot_accepted
                && ctx
                    .pending_snapshot_after_tick_change
                    .contains(&instrument_id)
            {
                ctx.pending_snapshot_after_tick_change
                    .remove(&instrument_id);
                log::debug!("Resumed book for {instrument_id} after tick size change");
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

            let mut resolved = Vec::with_capacity(quotes.price_changes.len());
            let mut groups: Vec<(TokenMeta, Vec<&PolymarketQuote>)> = Vec::new();
            let mut group_indices = AHashMap::with_capacity(quotes.price_changes.len());

            for change in &quotes.price_changes {
                let token_id = change.asset_id;
                let meta = match ctx.token_meta.get(&token_id) {
                    Some(m) => *m,
                    None => {
                        log::debug!("No instrument for token_id {token_id}");
                        continue;
                    }
                };
                let group_index = match group_indices.get(&meta.instrument_id) {
                    Some(index) => *index,
                    None => {
                        let index = groups.len();
                        groups.push((meta, Vec::new()));
                        group_indices.insert(meta.instrument_id, index);
                        index
                    }
                };
                groups[group_index].1.push(change);
                resolved.push((group_index, meta, change));
            }

            for (group_index, meta, change) in resolved {
                let instrument_id = meta.instrument_id;
                if is_terminal_condition(ctx, instrument_id) {
                    continue;
                }

                if let Some(book) = ctx.order_books.get(&instrument_id)
                    && ts_event < book.ts_last
                {
                    log::warn!(
                        "Ignoring stale price change for {instrument_id}: ts_event={ts_event} < ts_last={}",
                        book.ts_last,
                    );
                    continue;
                }

                let changes = std::mem::take(&mut groups[group_index].1);

                if !changes.is_empty() && ctx.active_delta_subs.contains(&instrument_id) {
                    if ctx
                        .pending_snapshot_after_tick_change
                        .contains(&instrument_id)
                    {
                        log::debug!(
                            "Dropping book deltas for {instrument_id}: awaiting valid snapshot",
                        );
                    } else {
                        let parsed = parse_book_deltas(
                            &changes,
                            instrument_id,
                            meta.price_precision,
                            meta.size_precision,
                            ts_event,
                            ts_init,
                        )
                        .into_iter()
                        .filter_map(|result| match result {
                            Ok(delta) => Some(delta),
                            Err(e) => {
                                log::error!("Failed to parse book delta for {instrument_id}: {e}");
                                None
                            }
                        })
                        .collect::<Vec<_>>();

                        if !parsed.is_empty() {
                            let deltas = OrderBookDeltas::new(instrument_id, parsed);

                            if ctx.compute_effective_deltas
                                && let Some(mut book) = ctx.order_books.get_mut(&instrument_id)
                                && let Err(e) = book.apply_deltas(&deltas)
                            {
                                log::error!("Failed to apply book deltas for {instrument_id}: {e}");
                            }

                            let data: NautilusData = deltas.into();
                            if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                                log::error!("Failed to emit book deltas: {e}");
                            }
                        }
                    }
                }

                if ctx.active_quote_subs.contains(&instrument_id) {
                    let price_increment = {
                        let instruments = ctx.instruments.load();
                        let Some(instrument) = instruments.get(&instrument_id) else {
                            log::error!("No instrument for {instrument_id}");
                            continue;
                        };
                        instrument.price_increment()
                    };

                    // Clone and drop guard before emit to avoid DashMap deadlock
                    let last_quote = ctx.last_quotes.get(&instrument_id).map(|r| *r);

                    match parse_quote_from_price_change(
                        change,
                        instrument_id,
                        meta.price_precision,
                        meta.size_precision,
                        price_increment,
                        ctx.drop_quotes_missing_side,
                        last_quote.as_ref(),
                        ts_event,
                        ts_init,
                    ) {
                        Ok(Some(quote)) => {
                            emit_quote_if_changed(ctx, instrument_id, quote);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::error!("Failed to parse quote from price change: {e}");
                        }
                    }
                }
            }
        }

        MarketWsMessage::LastTradePrice(trade) => {
            let token_id = trade.asset_id;
            let meta = match ctx.token_meta.get(&token_id) {
                Some(m) => *m,
                None => {
                    log::debug!("No instrument for token_id {token_id}");
                    return;
                }
            };

            let instrument_id = meta.instrument_id;
            if is_terminal_condition(ctx, instrument_id) {
                return;
            }

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
            let token_id = change.asset_id;
            let meta = match ctx.token_meta.get(&token_id) {
                Some(m) => *m,
                None => {
                    log::error!("No instrument for token_id {token_id}");
                    return;
                }
            };

            if is_terminal_condition(ctx, meta.instrument_id) {
                return;
            }

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

            let instruments = ctx.instruments.load();
            let existing = instruments.get(&meta.instrument_id);

            // No-op tick_size_change must not trigger an epoch transition.
            if let Some(existing_inst) = existing
                && existing_inst.price_increment().as_decimal() == tick_size
            {
                log::debug!(
                    "Ignoring duplicate tick size change for {}: {} -> {}",
                    change.asset_id,
                    change.old_tick_size,
                    change.new_tick_size,
                );
                return;
            }

            drop(instruments);

            log::debug!(
                "Tick size changed for {}: {} -> {}",
                change.asset_id,
                change.old_tick_size,
                change.new_tick_size
            );

            ctx.token_meta.insert(
                token_id,
                TokenMeta {
                    price_precision: new_price_precision,
                    ..meta
                },
            );

            let ts_init = ctx.clock.get_time_ns();
            let mut rebuilt = None;
            let mut rebuild_error = None;

            // Rebuild from the value the map holds now, so a concurrent market closure update is
            // carried forward rather than reverted by an older snapshot. Resolving presence here
            // rather than from the snapshot above also covers an instrument cached after it, whose
            // `token_meta` precision was already advanced.
            ctx.instruments.rcu(|map| {
                rebuilt = None;
                rebuild_error = None;

                let Some(current) = map.get(&meta.instrument_id).cloned() else {
                    return;
                };

                match rebuild_instrument_with_tick_size(
                    &current,
                    &change.new_tick_size,
                    ts_init,
                    ts_init,
                ) {
                    Ok(instrument) => {
                        map.insert(instrument.id(), instrument.clone());
                        rebuilt = Some(instrument);
                    }
                    Err(e) => rebuild_error = Some(e.to_string()),
                }
            });

            if let Some(e) = rebuild_error {
                log::error!("Failed to rebuild instrument for tick size change: {e}");
            } else if let Some(rebuilt) = rebuilt {
                // Retirement wins if the instrument was removed after the cache update
                if let Some(latest) = ctx.instruments.get_cloned(&rebuilt.id())
                    && let Err(e) = ctx.data_sender.send(DataEvent::Instrument(latest))
                {
                    log::error!("Failed to emit rebuilt instrument: {e}");
                }
            }

            // Book epoch transition; see module docs.
            let instrument_id = meta.instrument_id;
            ctx.order_books.remove(&instrument_id);

            if ctx.active_delta_subs.contains(&instrument_id) {
                ctx.pending_snapshot_after_tick_change.insert(instrument_id);
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

            let dedupe_key = new_market_dedupe_key(&nm);
            let fetch_condition_id = new_market_fetch_condition_id(&nm);
            let slug = nm.slug;

            if ctx
                .new_market_inflight_keys
                .insert(dedupe_key.clone(), ())
                .is_some()
            {
                log::debug!(
                    "Deduped new market event key='{dedupe_key}' slug='{slug}' (fetch already in-flight)",
                );
                return;
            }

            let gamma_client = ctx.gamma_client.clone();
            let filters = ctx.filters.clone();
            let token_meta = ctx.token_meta.clone();
            let instruments = ctx.instruments.clone();
            let closed_condition_ids = ctx.closed_condition_ids.clone();
            let data_sender = ctx.data_sender.clone();
            let clock = ctx.clock;
            let cancellation = ctx.cancellation_token.clone();
            let inflight_keys = ctx.new_market_inflight_keys.clone();
            let fetch_semaphore = ctx.new_market_fetch_semaphore.clone();
            let active = nm.active;

            let (Some(tasks), Some(task_registration)) =
                (ctx.tasks.upgrade(), ctx.task_registration.upgrade())
            else {
                ctx.new_market_inflight_keys.remove(&dedupe_key);
                return;
            };

            let inflight_guard = NewMarketInflightGuard::new(inflight_keys, dedupe_key.clone());
            let future = async move {
                let _inflight_guard = inflight_guard;
                let _permit = tokio::select! {
                    permit = fetch_semaphore.clone().acquire_owned() => {
                        match permit {
                            Ok(permit) => permit,
                            Err(_) => {
                                log::debug!("New market fetch semaphore closed");
                                return;
                            }
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("New market fetch for '{slug}' cancelled before acquire");
                        return;
                    }
                };

                let result = if let Some(condition_id) = fetch_condition_id {
                    let mut attempt = 0usize;

                    loop {
                        let params = GetGammaMarketsParams {
                            condition_ids: Some(vec![condition_id.clone()]),
                            ..Default::default()
                        };
                        let fetch =
                            gamma_client.request_instruments_by_params_with_transient(params);

                        let attempt_result = tokio::select! {
                            r = fetch => r,
                            () = cancellation.cancelled() => {
                                log::debug!("New market fetch for '{slug}' cancelled during shutdown");
                                return;
                            }
                        };

                        match attempt_result {
                            Ok((instruments, transient)) => {
                                if !instruments.is_empty() {
                                    break Ok(instruments);
                                }

                                let transient_hit =
                                    transient.iter().any(|cid| cid == &condition_id);

                                if attempt < NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS {
                                    attempt += 1;
                                    let reason = if transient_hit {
                                        "transient hydration"
                                    } else {
                                        "empty result"
                                    };
                                    log::debug!(
                                        "New market empty fetch retry {attempt}/{NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS} for key='{dedupe_key}' slug='{slug}' ({reason})",
                                    );

                                    tokio::select! {
                                        () = tokio::time::sleep(NEW_MARKET_EMPTY_RECHECK_DELAY) => {}
                                        () = cancellation.cancelled() => {
                                            log::debug!("New market fetch for '{slug}' cancelled during retry delay");
                                            return;
                                        }
                                    }
                                    continue;
                                }

                                log::warn!(
                                    "New market fetch returned no instruments for key='{dedupe_key}' slug='{slug}' after {NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS} recheck attempt(s)",
                                );
                                return;
                            }
                            Err(e) => break Err(e),
                        }
                    }
                } else {
                    log::warn!(
                        "New market slug='{slug}' missing condition identifiers; falling back to slug query",
                    );
                    tokio::select! {
                        r = gamma_client.request_instruments_by_slugs_with_retry(vec![slug.clone()]) => r,
                        () = cancellation.cancelled() => {
                            log::debug!("New market slug fallback fetch for '{slug}' cancelled during shutdown");
                            return;
                        }
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

                            if crate::data::runtime::is_instrument_expired(
                                &inst,
                                clock.get_time_ns(),
                            ) {
                                log::debug!(
                                    "Skipping expired new market instrument {} during cache update",
                                    inst.id()
                                );
                                continue;
                            }

                            let instrument_id = inst.id();
                            apply_live_instrument(
                                &closed_condition_ids,
                                &instruments,
                                &token_meta,
                                &inst,
                                |instrument| {
                                    if let Err(e) =
                                        data_sender.send(DataEvent::Instrument(instrument.clone()))
                                    {
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
                                },
                            );
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to fetch instruments for new market slug '{slug}': {e}");
                    }
                }
            };
            spawn_task(&tasks, &task_registration, &ctx.cancellation_token, future);
        }

        MarketWsMessage::MarketResolved(resolved) => {
            let emitted = apply_condition_resolution(
                &ctx.resolve_context(),
                resolved.market.as_str(),
                &resolved.winning_asset_id,
                &resolved.winning_outcome,
            );

            if emitted > 0 {
                log::debug!(
                    "Applied market_resolved for condition_id={} winner={} ({}) tracked_instruments={emitted}",
                    resolved.market,
                    resolved.winning_asset_id,
                    resolved.winning_outcome
                );
            }
        }

        MarketWsMessage::BestBidAsk(bba) => {
            let token_id = bba.asset_id;
            let meta = match ctx.token_meta.get(&token_id) {
                Some(m) => *m,
                None => {
                    log::debug!("No instrument for token_id {token_id}");
                    return;
                }
            };

            let instrument_id = meta.instrument_id;
            if is_terminal_condition(ctx, instrument_id)
                || !ctx.active_quote_subs.contains(&instrument_id)
            {
                return;
            }

            let ts_init = ctx.clock.get_time_ns();
            let ts_event = match parse_timestamp_ms(&bba.timestamp) {
                Ok(ts) => ts,
                Err(e) => {
                    log::error!("Failed to parse best bid/ask timestamp: {e}");
                    return;
                }
            };

            let last_quote = ctx.last_quotes.get(&instrument_id).map(|quote| *quote);
            let price_increment = {
                let instruments = ctx.instruments.load();
                let Some(instrument) = instruments.get(&instrument_id) else {
                    log::error!("No instrument for {instrument_id}");
                    return;
                };
                instrument.price_increment()
            };

            let last_tops = last_quote.map_or((None, None), |quote| {
                (
                    Some((quote.bid_price, quote.bid_size)),
                    Some((quote.ask_price, quote.ask_size)),
                )
            });
            let (bid_top, ask_top) = match ctx.order_books.get(&instrument_id) {
                Some(book) if book.ts_last > ts_event => {
                    log::trace!("Ignoring best bid/ask older than local book for {instrument_id}");
                    return;
                }
                Some(book)
                    if !ctx
                        .pending_snapshot_after_tick_change
                        .contains(&instrument_id) =>
                {
                    (
                        book.best_bid_price().zip(book.best_bid_size()),
                        book.best_ask_price().zip(book.best_ask_size()),
                    )
                }
                _ => last_tops,
            };

            match parse_quote_from_best_bid_ask(
                &bba,
                instrument_id,
                meta.price_precision,
                meta.size_precision,
                price_increment,
                ctx.drop_quotes_missing_side,
                bid_top,
                ask_top,
                ts_event,
                ts_init,
            ) {
                Ok(Some(quote)) => emit_quote_if_changed(ctx, instrument_id, quote),
                Ok(None) => {}
                Err(e) => log::error!("Failed to parse quote from best bid/ask: {e}"),
            }
        }
    }
}

fn emit_quote_if_changed(ctx: &WsMessageContext, instrument_id: InstrumentId, quote: QuoteTick) {
    let existing = ctx
        .last_quotes
        .get(&instrument_id)
        .map(|existing| *existing);
    if existing.is_some_and(|existing| existing.ts_event > quote.ts_event) {
        log::trace!("Ignoring stale quote for {instrument_id}");
        return;
    }

    // Compare prices and sizes only; timestamps always differ between messages
    let emit = !matches!(
        existing,
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

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        net::SocketAddr,
        num::NonZeroUsize,
        ops::{Deref, DerefMut},
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, Duration as StdDuration},
    };

    use ahash::AHashMap;
    use axum::{
        Router,
        extract::{
            Path, RawQuery, State,
            ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
        },
        http::StatusCode,
        response::{IntoResponse, Json, Response},
        routing::get,
    };
    use futures_util::StreamExt;
    use jiff::{SignedDuration, Timestamp, tz::Offset};
    use nautilus_common::{
        clients::DataClient,
        live::runner::replace_data_event_sender,
        messages::{
            DataResponse,
            data::{
                RequestBookSnapshot, RequestCustomData, RequestInstrument, RequestTrades,
                SubscribeBookDeltas, SubscribeQuotes,
            },
        },
        testing::wait_until_async,
    };
    use nautilus_core::{Params, UUID4, UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::{
        data::{BookOrder, CustomData as ModelCustomData, DataType, OrderBookDelta},
        enums::{BookAction, InstrumentCloseType, OrderSide, PositionSide, RecordFlag},
        events::{PositionEvent, PositionOpened},
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol,
            TraderId,
        },
        instruments::stubs::binary_option,
        types::{Currency, Price, Quantity},
    };
    use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
    use rstest::rstest;
    use serde_json::Value;
    use ustr::Ustr;

    use super::{
        super::{PolymarketDataClient, instruments::cache_instrument_unchecked},
        *,
    };
    use crate::{
        common::{
            consts::{POLYMARKET_CLIENT_ID, POLYMARKET_VENUE},
            enums::PolymarketOrderSide,
        },
        config::PolymarketDataClientConfig,
        http::{clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient},
        resolve::{
            PolymarketResolveRequestSummaryData, RESOLVE_REQUEST_TYPE_NAME, ResolveBatchErrorMode,
            fetch_and_apply_resolutions_by_condition_ids, pause_resolve_watch_entries,
            update_resolve_watchlist_from_position_event,
            upsert_resolve_watch_entry_from_instrument,
        },
        websocket::{
            messages::{
                PolymarketBestBidAsk, PolymarketBookLevel, PolymarketBookSnapshot,
                PolymarketMarketResolved, PolymarketQuote, PolymarketQuotes,
                PolymarketTickSizeChange,
            },
            pool::PolymarketMarketConnectionPool,
        },
    };

    fn is_resolve_response(event: &DataEvent) -> bool {
        matches!(event, DataEvent::Response(DataResponse::Data(_)))
    }

    type CacheProbe = Arc<dyn Fn() -> bool + Send + Sync>;

    struct TestWsContext {
        ctx: WsMessageContext,
        tasks: Arc<TaskHandles>,
        _task_registration: Arc<StdMutex<()>>,
    }

    impl Deref for TestWsContext {
        type Target = WsMessageContext;

        fn deref(&self) -> &Self::Target {
            &self.ctx
        }
    }

    impl DerefMut for TestWsContext {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.ctx
        }
    }

    async fn record_json_ws_payloads(
        mut socket: WebSocket,
        received_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
    ) {
        while let Some(result) = socket.next().await {
            let Ok(message) = result else { break };

            match message {
                AxumWsMessage::Text(text) => {
                    let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    received_payloads.lock().await.push(payload);
                }
                AxumWsMessage::Ping(data) => {
                    if socket.send(AxumWsMessage::Pong(data)).await.is_err() {
                        break;
                    }
                }
                AxumWsMessage::Close(_) => break,
                _ => {}
            }
        }
    }

    #[derive(Clone, Default)]
    struct RtdsTestServerState {
        received_payloads: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
    }

    async fn handle_rtds_upgrade(
        ws: WebSocketUpgrade,
        State(state): State<RtdsTestServerState>,
    ) -> axum::response::Response {
        ws.on_upgrade(move |socket| record_json_ws_payloads(socket, state.received_payloads))
    }

    async fn start_rtds_test_server(state: RtdsTestServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind RTDS test server");
        let addr = listener.local_addr().expect("local_addr");
        let router = Router::new()
            .route("/rtds", get(handle_rtds_upgrade))
            .with_state(state);

        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("RTDS test server failed");
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        addr
    }

    fn count_instrument_close_events(events: &[DataEvent]) -> usize {
        events
            .iter()
            .filter(|event| matches!(event, DataEvent::Data(NautilusData::InstrumentClose(_))))
            .count()
    }
    async fn collect_events_until<F>(
        data_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        timeout: StdDuration,
        mut done: F,
    ) -> Vec<DataEvent>
    where
        F: FnMut(&[DataEvent]) -> bool,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut events = Vec::new();

        loop {
            while let Ok(event) = data_rx.try_recv() {
                events.push(event);
            }

            if done(&events) || tokio::time::Instant::now() >= deadline {
                break;
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            let wait_for = remaining.min(StdDuration::from_millis(100));
            if let Ok(Some(event)) = tokio::time::timeout(wait_for, data_rx.recv()).await {
                events.push(event);
            }
        }

        events
    }

    fn stub_instrument(
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
    ) -> InstrumentAny {
        let mut binary = binary_option();
        binary.id = InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str());
        binary.raw_symbol = Symbol::new(raw_symbol);
        binary.currency = Currency::pUSD();
        binary.activation_ns = UnixNanos::default();
        binary.expiration_ns = UnixNanos::from(u64::MAX);
        binary.price_precision = price_increment.precision;
        binary.size_precision = size_increment.precision;
        binary.price_increment = price_increment;
        binary.size_increment = size_increment;
        InstrumentAny::BinaryOption(binary)
    }

    fn make_ws_ctx_with_gamma_base_url(
        gamma_base_url: &str,
    ) -> (
        TestWsContext,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let gamma_client = PolymarketGammaHttpClient::new(
            Some(gamma_base_url.to_string()),
            2,
            RetryConfig {
                max_retries: 0,
                initial_delay_ms: 1,
                max_delay_ms: 1,
                backoff_factor: 1.0,
                jitter_ms: 0,
                operation_timeout_ms: Some(2_000),
                immediate_first: true,
                max_elapsed_ms: Some(2_000),
            },
        )
        .expect("gamma client");
        let default_config = PolymarketDataClientConfig::default();
        let tasks = Arc::new(TaskHandles::default());
        let task_registration = Arc::new(StdMutex::new(()));

        let ctx = WsMessageContext {
            clock: get_atomic_clock_realtime(),
            data_sender: data_tx.clone(),
            token_meta: Arc::new(DashMap::new()),
            instruments: Arc::new(AtomicMap::new()),
            gamma_client,
            filters: vec![],
            order_books: Arc::new(DashMap::new()),
            last_quotes: Arc::new(DashMap::new()),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_delta_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            closed_condition_ids: Arc::new(StdMutex::new(AHashSet::new())),
            resolve_poll_watchlist: Arc::new(AtomicMap::new()),
            resolve_watch_apply_mutex: Arc::new(StdMutex::new(())),
            pending_snapshot_after_tick_change: Arc::new(AtomicSet::new()),
            new_market_inflight_keys: Arc::new(DashMap::new()),
            new_market_fetch_semaphore: Arc::new(tokio::sync::Semaphore::new(
                default_config.new_market_fetch_max_concurrency,
            )),
            tasks: Arc::downgrade(&tasks),
            task_registration: Arc::downgrade(&task_registration),
            rtds_feed: crate::rtds::PolymarketRtdsFeed::new(
                "ws://localhost/rtds".to_string(),
                TransportBackend::default(),
                get_atomic_clock_realtime(),
                data_tx,
            ),
            subscribe_new_markets: false,
            new_market_filter: None,
            drop_quotes_missing_side: default_config.drop_quotes_missing_side,
            compute_effective_deltas: default_config.compute_effective_deltas,
            cancellation_token: CancellationToken::new(),
        };

        (
            TestWsContext {
                ctx,
                tasks,
                _task_registration: task_registration,
            },
            data_rx,
        )
    }

    fn make_ws_ctx() -> (
        TestWsContext,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        make_ws_ctx_with_gamma_base_url("http://localhost")
    }
    fn seed_instrument(
        ctx: &WsMessageContext,
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
    ) -> InstrumentAny {
        let inst = stub_instrument(raw_symbol, price_increment, size_increment);
        cache_instrument_unchecked(&ctx.instruments, &ctx.token_meta, &inst);
        inst
    }

    #[derive(Clone, Copy, Default)]
    struct SeedInstrumentContext<'a> {
        market_slug: Option<&'a str>,
        market_id: Option<&'a str>,
        condition_id: Option<&'a str>,
        min_order_size: Option<&'a str>,
        neg_risk: Option<bool>,
        expiration_ns: Option<UnixNanos>,
        market_closed: Option<bool>,
    }

    fn seed_instrument_with_context(
        ctx: &WsMessageContext,
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
        seed_ctx: SeedInstrumentContext<'_>,
    ) -> InstrumentAny {
        let mut inst = stub_instrument(raw_symbol, price_increment, size_increment);
        if let InstrumentAny::BinaryOption(ref mut binary) = inst {
            if let Some(expiration_ns) = seed_ctx.expiration_ns {
                binary.expiration_ns = expiration_ns;
            }

            let mut info = Params::new();
            info.insert(
                "token_id".to_string(),
                serde_json::Value::String(raw_symbol.to_string()),
            );

            if let Some(market_slug) = seed_ctx.market_slug {
                info.insert(
                    "market_slug".to_string(),
                    serde_json::Value::String(market_slug.to_string()),
                );
            }

            if let Some(market_id) = seed_ctx.market_id {
                info.insert(
                    "market_id".to_string(),
                    serde_json::Value::String(market_id.to_string()),
                );
            }

            if let Some(condition_id) = seed_ctx.condition_id {
                info.insert(
                    "condition_id".to_string(),
                    serde_json::Value::String(condition_id.to_string()),
                );
            }

            if let Some(min_order_size) = seed_ctx.min_order_size {
                info.insert(
                    "min_order_size".to_string(),
                    serde_json::Value::String(min_order_size.to_string()),
                );
            }

            if let Some(neg_risk) = seed_ctx.neg_risk {
                info.insert("neg_risk".to_string(), neg_risk.into());
            }

            if let Some(closed) = seed_ctx.market_closed {
                info.insert("closed".to_string(), closed.into());
            }
            binary.info = Some(info);
        }

        cache_instrument_unchecked(&ctx.instruments, &ctx.token_meta, &inst);
        inst
    }

    fn stub_position_opened_event_with_position_id(
        instrument_id: InstrumentId,
        position_id: &str,
    ) -> PositionEvent {
        PositionEvent::PositionOpened(PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            instrument_id,
            position_id: PositionId::new(position_id),
            account_id: AccountId::from("ACCOUNT-001"),
            opening_order_id: ClientOrderId::from("ENTRY-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 1.0,
            quantity: Quantity::from("1"),
            last_qty: Quantity::from("1"),
            last_px: Price::from("0.75"),
            currency: Currency::pUSD(),
            avg_px_open: 0.75,
            realized_pnl: None,
            event_id: UUID4::new(),
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
        })
    }

    fn stub_position_opened_event(instrument_id: InstrumentId) -> PositionEvent {
        stub_position_opened_event_with_position_id(instrument_id, "P-1")
    }

    fn make_client_ws_ctx(client: &PolymarketDataClient) -> TestWsContext {
        let ctx = WsMessageContext {
            clock: client.clock,
            data_sender: client.data_sender.clone(),
            token_meta: client.token_meta.clone(),
            instruments: client.instruments.clone(),
            gamma_client: client.provider.http_client().clone(),
            filters: client.provider.filters(),
            order_books: client.order_books.clone(),
            last_quotes: client.last_quotes.clone(),
            active_quote_subs: client.active_quote_subs.clone(),
            active_delta_subs: client.active_delta_subs.clone(),
            active_trade_subs: client.active_trade_subs.clone(),
            closed_condition_ids: client.closed_condition_ids.clone(),
            resolve_poll_watchlist: client.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: client.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: client.pending_snapshot_after_tick_change.clone(),
            new_market_inflight_keys: client.new_market_inflight_keys.clone(),
            new_market_fetch_semaphore: client.new_market_fetch_semaphore.clone(),
            tasks: Arc::downgrade(&client.tasks),
            task_registration: Arc::downgrade(&client.task_registration),
            rtds_feed: client.rtds_feed.clone(),
            subscribe_new_markets: client.config.subscribe_new_markets,
            new_market_filter: client.config.new_market_filter.clone(),
            drop_quotes_missing_side: client.config.drop_quotes_missing_side,
            compute_effective_deltas: client.config.compute_effective_deltas,
            cancellation_token: client.cancellation_token.clone(),
        };

        TestWsContext {
            ctx,
            tasks: client.tasks.clone(),
            _task_registration: client.task_registration.clone(),
        }
    }

    fn make_new_market(slug: &str, active: bool) -> MarketWsMessage {
        make_new_market_with_ids(
            slug,
            &format!("cond-{slug}"),
            &format!("cond-{slug}"),
            active,
        )
    }

    fn make_new_market_with_condition(
        slug: &str,
        condition_id: &str,
        active: bool,
    ) -> MarketWsMessage {
        make_new_market_with_ids(slug, condition_id, condition_id, active)
    }

    fn make_new_market_with_ids(
        slug: &str,
        market: &str,
        condition_id: &str,
        active: bool,
    ) -> MarketWsMessage {
        MarketWsMessage::NewMarket(Box::new(PolymarketNewMarket {
            id: format!("id-{slug}"),
            question: format!("Will {slug} settle true?"),
            market: Ustr::from(market),
            slug: slug.to_string(),
            description: format!("desc-{slug}"),
            assets_ids: vec![format!("yes-{slug}"), format!("no-{slug}")],
            outcomes: vec!["Yes".to_string(), "No".to_string()],
            timestamp: "1700000003000".to_string(),
            tags: vec![],
            condition_id: condition_id.to_string(),
            active,
            clob_token_ids: vec![format!("yes-{slug}"), format!("no-{slug}")],
            order_price_min_tick_size: None,
            group_item_title: None,
            event_message: None,
            sports_market_type: None,
            line: None,
            game_start_time: None,
            taker_base_fee: None,
            fees_enabled: None,
            fee_schedule: None,
        }))
    }

    fn gamma_market_expired_fixture_value() -> Value {
        serde_json::from_str(include_str!("../../test_data/gamma_market.json"))
            .expect("gamma market fixture json")
    }

    fn gamma_market_future_closed_fixture_value() -> Value {
        let mut value = gamma_market_recheck_fixture_value();
        value["closed"] = Value::Bool(true);
        value
    }

    fn gamma_market_recheck_fixture_value() -> Value {
        let mut value = gamma_market_expired_fixture_value();
        let future_date = Offset::UTC
            .to_datetime(Timestamp::now() + SignedDuration::from_hours(24 * 365))
            .date();
        let end_date = format!("{}T00:00:00Z", future_date.strftime("%Y-%m-%d"));

        if let Some(root) = value.as_object_mut() {
            root.insert("endDate".to_string(), Value::String(end_date.clone()));
            root.insert(
                "endDateIso".to_string(),
                Value::String(end_date[..10].to_string()),
            );

            if let Some(events) = root.get_mut("events").and_then(Value::as_array_mut) {
                for event in events {
                    if let Some(event_obj) = event.as_object_mut() {
                        event_obj.insert("endDate".to_string(), Value::String(end_date.clone()));
                    }
                }
            }
        }

        value
    }

    fn gamma_market_fixture_for(
        condition_id: &str,
        yes_token_id: &str,
        no_token_id: &str,
        closed: bool,
    ) -> Value {
        let mut value = gamma_market_recheck_fixture_value();
        value["conditionId"] = Value::String(condition_id.to_string());
        value["clobTokenIds"] = Value::String(
            serde_json::to_string(&[yes_token_id, no_token_id]).expect("serialize token ids"),
        );
        value["closed"] = Value::Bool(closed);
        value
    }

    const TEST_CONDITION_ID: &str =
        "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63b";
    const TEST_TOKEN_ID_YES: &str =
        "104239898038807136052399800151408521467737075933964991162589336683346093173875";
    const TEST_TOKEN_ID_NO: &str =
        "71183960810705820955071415844881728181970340514894896943812046065452395013351";

    fn fixture_yes_instrument_id() -> InstrumentId {
        InstrumentId::from(format!("{TEST_CONDITION_ID}-{TEST_TOKEN_ID_YES}.POLYMARKET").as_str())
    }

    fn fixture_no_instrument_id() -> InstrumentId {
        InstrumentId::from(format!("{TEST_CONDITION_ID}-{TEST_TOKEN_ID_NO}.POLYMARKET").as_str())
    }

    fn fixture_instrument_id(condition_id: &str, token_id: &str) -> InstrumentId {
        InstrumentId::from(format!("{condition_id}-{token_id}.POLYMARKET").as_str())
    }

    fn instrument_from_gamma_fixture(value: Value) -> InstrumentAny {
        instruments_from_gamma_fixture(value)
            .into_iter()
            .next()
            .expect("fixture instrument")
    }

    fn instruments_from_gamma_fixture(value: Value) -> Vec<InstrumentAny> {
        let market = serde_json::from_value(value).expect("gamma market fixture");
        let definitions = crate::http::parse::parse_gamma_market(&market).expect("parse fixture");
        definitions
            .iter()
            .map(|definition| {
                crate::http::parse::create_instrument_from_def(definition, UnixNanos::default())
                    .expect("create fixture instrument")
            })
            .collect()
    }

    #[derive(Clone, Copy, Debug)]
    enum ExpiredPath {
        Quotes,
        BookSnapshot,
        Trades,
    }

    #[derive(Clone, Default)]
    struct NewMarketFetchTestServerState {
        total_requests: Arc<AtomicUsize>,
        inflight_requests: Arc<AtomicUsize>,
        max_inflight_requests: Arc<AtomicUsize>,
        seen_condition_ids: Arc<StdMutex<Vec<Option<String>>>>,
        seen_slugs: Arc<StdMutex<Vec<Option<String>>>>,
        empty_then_success_condition_id: Arc<StdMutex<Option<String>>>,
        empty_then_success_payload: Arc<StdMutex<Option<Value>>>,
        per_condition_requests: Arc<StdMutex<AHashMap<String, usize>>>,
        response_delay_ms: u64,
    }

    fn query_param(raw_query: Option<String>, key: &str) -> Option<String> {
        let raw = raw_query?;
        raw.split('&').find_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let pair_key = parts.next().unwrap_or("");
            if pair_key != key {
                return None;
            }
            Some(parts.next().unwrap_or("").to_string())
        })
    }

    async fn handle_new_market_gamma_markets(
        RawQuery(raw_query): RawQuery,
        State(state): State<NewMarketFetchTestServerState>,
    ) -> Json<Value> {
        state.total_requests.fetch_add(1, Ordering::SeqCst);
        let inflight = state.inflight_requests.fetch_add(1, Ordering::SeqCst) + 1;
        let condition_id = query_param(raw_query.clone(), "condition_ids");
        let slug = query_param(raw_query, "slug");

        state
            .seen_condition_ids
            .lock()
            .expect("seen_condition_ids mutex poisoned")
            .push(condition_id.clone());
        state
            .seen_slugs
            .lock()
            .expect("seen_slugs mutex poisoned")
            .push(slug);

        loop {
            let prev = state.max_inflight_requests.load(Ordering::SeqCst);
            if inflight <= prev {
                break;
            }

            if state
                .max_inflight_requests
                .compare_exchange(prev, inflight, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }

        if state.response_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(state.response_delay_ms)).await;
        }

        let response = if let Some(ref cid) = condition_id {
            let next_count = {
                let mut counts = state
                    .per_condition_requests
                    .lock()
                    .expect("per_condition_requests mutex poisoned");
                let next = counts.get(cid).copied().unwrap_or(0) + 1;
                counts.insert(cid.clone(), next);
                next
            };

            let target_cid = state
                .empty_then_success_condition_id
                .lock()
                .expect("empty_then_success_condition_id mutex poisoned")
                .clone();

            if target_cid.as_deref() == Some(cid.as_str()) && next_count >= 2 {
                state
                    .empty_then_success_payload
                    .lock()
                    .expect("empty_then_success_payload mutex poisoned")
                    .clone()
                    .unwrap_or_else(|| serde_json::json!([]))
            } else {
                serde_json::json!([])
            }
        } else {
            serde_json::json!([])
        };

        state.inflight_requests.fetch_sub(1, Ordering::SeqCst);
        Json(response)
    }

    async fn handle_new_market_gamma_markets_keyset(
        raw_query: RawQuery,
        state: State<NewMarketFetchTestServerState>,
    ) -> Json<Value> {
        let Json(markets) = handle_new_market_gamma_markets(raw_query, state).await;
        Json(serde_json::json!({"markets": markets}))
    }

    async fn start_new_market_test_server(state: NewMarketFetchTestServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let router = Router::new()
            .route("/markets", get(handle_new_market_gamma_markets))
            .route(
                "/markets/keyset",
                get(handle_new_market_gamma_markets_keyset),
            )
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
        addr
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_condition_empty_then_success_recheck_loads_instrument() {
        let state = NewMarketFetchTestServerState::default();
        let target_condition = "0xcondition-recheck";
        *state
            .empty_then_success_condition_id
            .lock()
            .expect("empty_then_success_condition_id mutex poisoned") =
            Some(target_condition.to_string());
        *state
            .empty_then_success_payload
            .lock()
            .expect("empty_then_success_payload mutex poisoned") =
            Some(serde_json::json!([gamma_market_recheck_fixture_value()]));

        let addr = start_new_market_test_server(state.clone()).await;
        let gamma_base_url = format!("http://{addr}");
        let (mut ctx, mut data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
        ctx.subscribe_new_markets = true;
        ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        handle_market_message(
            make_new_market_with_ids(
                "btc-updown-5m-recheck",
                target_condition,
                target_condition,
                true,
            ),
            &ctx,
        );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

        loop {
            let done = state.total_requests.load(Ordering::SeqCst) >= 2
                && state.inflight_requests.load(Ordering::SeqCst) == 0
                && ctx.new_market_inflight_keys.is_empty()
                && !ctx.instruments.load().is_empty();

            if done {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for empty-then-success recheck flow",
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let seen_condition_ids = state
            .seen_condition_ids
            .lock()
            .expect("seen_condition_ids mutex poisoned")
            .clone();
        assert!(
            seen_condition_ids
                .iter()
                .all(|cid| cid.as_deref() == Some(target_condition)),
            "all requests should query target condition_id, saw: {seen_condition_ids:?}",
        );
        assert_eq!(
            state.total_requests.load(Ordering::SeqCst),
            2,
            "single recheck policy should perform exactly two condition fetch attempts",
        );

        let mut emitted_instrument = false;

        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(200), data_rx.recv()).await
        {
            if matches!(event, DataEvent::Instrument(_)) {
                emitted_instrument = true;
                break;
            }
        }
        assert!(
            emitted_instrument,
            "expected emitted DataEvent::Instrument after successful recheck"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_does_not_restore_terminal_condition_live_state() {
        let market = gamma_market_recheck_fixture_value();
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([market]))],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (client, mut data_rx) = create_test_client(addr);
        let mut ctx = make_client_ws_ctx(&client);
        ctx.subscribe_new_markets = true;
        client
            .closed_condition_ids
            .lock()
            .unwrap()
            .insert(TEST_CONDITION_ID.to_string());

        handle_market_message(
            make_new_market_with_condition("terminal-condition", TEST_CONDITION_ID, true),
            &ctx,
        );

        wait_until_async(
            || {
                let state = state.clone();
                let ctx = &ctx;
                async move {
                    !state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .is_empty()
                        && ctx.new_market_inflight_keys.is_empty()
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert!(ctx.instruments.load().is_empty());
        assert!(ctx.token_meta.is_empty());
        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn terminal_condition_drops_queued_market_data_dispatch() {
        let state = ScriptedAutoLoadServerState::new(vec![], vec![]);
        let addr = start_scripted_auto_load_test_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let instrument = instrument_from_gamma_fixture(gamma_market_recheck_fixture_value());
        let instrument_id = instrument.id();
        cache_instrument_unchecked(&client.instruments, &client.token_meta, &instrument);
        client.active_delta_subs.insert(instrument_id);
        client
            .closed_condition_ids
            .lock()
            .unwrap()
            .insert(TEST_CONDITION_ID.to_string());
        let ctx = make_client_ws_ctx(&client);

        handle_market_message(
            make_price_change(
                TEST_CONDITION_ID,
                instrument.raw_symbol().as_str(),
                "0.45",
                "20",
            ),
            &ctx,
        );

        assert!(ctx.order_books.is_empty());
        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn queued_reconciliation_cannot_subscribe_terminal_condition() {
        let state = ScriptedAutoLoadServerState::new(vec![], vec![]);
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (client, _data_rx) = create_test_client(addr);
        let instrument = instrument_from_gamma_fixture(gamma_market_recheck_fixture_value());
        let instrument_id = instrument.id();
        let token_id = Ustr::from(instrument.raw_symbol().as_str());
        cache_instrument_unchecked(&client.instruments, &client.token_meta, &instrument);
        client.active_quote_subs.insert(instrument_id);

        let guard = client.ws_sub_mutex.lock().await;
        client.sync_ws_subscription(instrument_id);
        client
            .closed_condition_ids
            .lock()
            .unwrap()
            .insert(TEST_CONDITION_ID.to_string());
        drop(guard);

        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(!client.ws_open_tokens.contains(&token_id));
        assert!(state.market_payloads.lock().await.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn instrument_request_does_not_restore_terminal_condition_live_state() {
        let market = gamma_market_recheck_fixture_value();
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([market]))],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        client
            .closed_condition_ids
            .lock()
            .unwrap()
            .insert(TEST_CONDITION_ID.to_string());
        let instrument_id = fixture_yes_instrument_id();

        client
            .request_instrument(RequestInstrument::new(
                instrument_id,
                None,
                None,
                Some(client.client_id),
                UUID4::new(),
                UnixNanos::default(),
                None,
            ))
            .expect("instrument request should start");

        let events = tokio::time::timeout(StdDuration::from_secs(3), async {
            let mut events = Vec::new();

            loop {
                let event = data_rx.recv().await.expect("data event channel closed");
                let is_response = matches!(event, DataEvent::Response(DataResponse::Instrument(_)));
                events.push(event);
                if is_response {
                    return events;
                }
            }
        })
        .await
        .expect("timed out waiting for instrument response");

        assert!(
            events
                .iter()
                .all(|event| !matches!(event, DataEvent::Instrument(_)))
        );
        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_canceled_registration_cleans_inflight_key() {
        let (mut ctx, _data_rx) = make_ws_ctx();
        ctx.subscribe_new_markets = true;
        ctx.cancellation_token.cancel();

        handle_market_message(make_new_market("btc-updown-5m-1", true), &ctx);

        assert!(ctx.tasks.is_empty());
        assert!(ctx.new_market_inflight_keys.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_dedupes_same_slug_and_cleans_inflight_on_cancel() {
        let state = NewMarketFetchTestServerState::default();
        let addr = start_new_market_test_server(state.clone()).await;
        let gamma_base_url = format!("http://{addr}");
        let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
        ctx.subscribe_new_markets = true;
        ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(0));

        handle_market_message(make_new_market("btc-updown-5m-1", true), &ctx);
        handle_market_message(make_new_market("btc-updown-5m-1", true), &ctx);

        assert_eq!(state.total_requests.load(Ordering::SeqCst), 0);
        assert_eq!(ctx.new_market_inflight_keys.len(), 1);
        assert!(
            ctx.new_market_inflight_keys
                .contains_key("cond:cond-btc-updown-5m-1")
        );
        assert_eq!(ctx.tasks.len(), 1);

        ctx.cancellation_token.cancel();
        for handle in ctx.tasks.take_all() {
            tokio::time::timeout(Duration::from_secs(1), handle)
                .await
                .expect("new market fetch cancellation timeout")
                .expect("new market fetch task join");
        }

        assert!(ctx.tasks.is_empty());
        assert!(ctx.new_market_inflight_keys.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_fetches_respect_global_concurrency_cap() {
        let state = NewMarketFetchTestServerState {
            response_delay_ms: 150,
            ..NewMarketFetchTestServerState::default()
        };
        let addr = start_new_market_test_server(state.clone()).await;
        let gamma_base_url = format!("http://{addr}");
        let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
        ctx.subscribe_new_markets = true;
        ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        let slug_count = 6usize;
        for idx in 0..slug_count {
            let slug = format!("asset-{idx}-updown-5m-1");
            handle_market_message(make_new_market(&slug, true), &ctx);
        }

        let expected_requests = slug_count * (1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);

        loop {
            let done = state.total_requests.load(Ordering::SeqCst) >= expected_requests
                && state.inflight_requests.load(Ordering::SeqCst) == 0
                && ctx.new_market_inflight_keys.is_empty();

            if done {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for new market fetch tasks to complete"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(
            state.total_requests.load(Ordering::SeqCst),
            expected_requests,
        );
        assert_eq!(state.max_inflight_requests.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_same_slug_can_refetch_after_previous_completion() {
        let state = NewMarketFetchTestServerState {
            response_delay_ms: 50,
            ..NewMarketFetchTestServerState::default()
        };
        let addr = start_new_market_test_server(state.clone()).await;
        let gamma_base_url = format!("http://{addr}");
        let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
        ctx.subscribe_new_markets = true;
        ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        let slug = "btc-updown-5m-2";
        let dedupe_key = "cond:cond-btc-updown-5m-2";
        handle_market_message(make_new_market(slug, true), &ctx);

        let per_fetch_requests = 1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS;
        let deadline_first = tokio::time::Instant::now() + Duration::from_secs(3);

        loop {
            let first_done = state.total_requests.load(Ordering::SeqCst) >= per_fetch_requests
                && state.inflight_requests.load(Ordering::SeqCst) == 0
                && !ctx.new_market_inflight_keys.contains_key(dedupe_key);

            if first_done {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline_first,
                "timed out waiting for first slug fetch to complete"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        handle_market_message(make_new_market(slug, true), &ctx);

        let deadline_second = tokio::time::Instant::now() + Duration::from_secs(3);

        loop {
            let second_done = state.total_requests.load(Ordering::SeqCst) >= per_fetch_requests * 2
                && state.inflight_requests.load(Ordering::SeqCst) == 0
                && !ctx.new_market_inflight_keys.contains_key(dedupe_key);

            if second_done {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline_second,
                "timed out waiting for second slug fetch to complete"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(
            state.total_requests.load(Ordering::SeqCst),
            per_fetch_requests * 2,
        );
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_cancellation_during_fetch_cleans_inflight_slug() {
        let state = NewMarketFetchTestServerState {
            response_delay_ms: 500,
            ..NewMarketFetchTestServerState::default()
        };
        let addr = start_new_market_test_server(state.clone()).await;
        let gamma_base_url = format!("http://{addr}");
        let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
        ctx.subscribe_new_markets = true;
        ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        let slug = "eth-updown-5m-cancel";
        let dedupe_key = "cond:cond-eth-updown-5m-cancel";
        handle_market_message(make_new_market(slug, true), &ctx);

        let deadline_started = tokio::time::Instant::now() + Duration::from_secs(2);

        loop {
            let started = state.inflight_requests.load(Ordering::SeqCst) > 0
                && ctx.new_market_inflight_keys.contains_key(dedupe_key);

            if started {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline_started,
                "timed out waiting for in-flight fetch to begin"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        ctx.cancellation_token.cancel();

        let deadline_cleanup = tokio::time::Instant::now() + Duration::from_secs(2);

        loop {
            if ctx.new_market_inflight_keys.is_empty() {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline_cleanup,
                "expected in-flight key cleanup after cancellation during fetch"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(
            state.max_inflight_requests.load(Ordering::SeqCst) <= 1,
            "fetch concurrency exceeded configured cap during cancellation path"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn handle_reconnected_does_not_replay_rtds_when_rtds_is_healthy() {
        let state = RtdsTestServerState::default();
        let addr = start_rtds_test_server(state.clone()).await;
        let (mut ctx, _data_rx) = make_ws_ctx();
        ctx.rtds_feed = crate::rtds::PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            ctx.clock,
            ctx.data_sender.clone(),
        );
        ctx.rtds_feed
            .track_subscribe(DataType::new(
                "PolymarketRtdsCryptoPrice",
                Some({
                    let mut metadata = Params::new();
                    metadata.insert("symbol".to_string(), Value::String("BTCUSDT".to_string()));
                    metadata
                }),
                None,
            ))
            .expect("track RTDS subscribe");
        ctx.rtds_feed.connect().await.expect("connect RTDS feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;
        state.received_payloads.lock().await.clear();

        handle_ws_message(PolymarketWsMessage::Reconnected, &ctx);
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            state.received_payloads.lock().await.is_empty(),
            "healthy RTDS connection should not replay subscriptions on main WS reconnect",
        );
        ctx.rtds_feed.disconnect().await;
    }

    #[rstest]
    #[tokio::test]
    async fn handle_reconnected_recovers_rtds_when_retained_subscriptions_are_missing() {
        let state = RtdsTestServerState::default();
        let addr = start_rtds_test_server(state.clone()).await;
        let (mut ctx, _data_rx) = make_ws_ctx();
        ctx.rtds_feed = crate::rtds::PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            ctx.clock,
            ctx.data_sender.clone(),
        );
        ctx.rtds_feed
            .track_subscribe(DataType::new(
                "PolymarketRtdsCryptoPrice",
                Some({
                    let mut metadata = Params::new();
                    metadata.insert("symbol".to_string(), Value::String("BTCUSDT".to_string()));
                    metadata
                }),
                None,
            ))
            .expect("track RTDS subscribe");

        handle_ws_message(PolymarketWsMessage::Reconnected, &ctx);

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        let payloads = state.received_payloads.lock().await.clone();
        let replay = payloads.last().expect("recovery payload");
        assert_eq!(replay["action"].as_str(), Some("subscribe"));
        ctx.rtds_feed.disconnect().await;
    }

    #[rstest]
    #[tokio::test]
    async fn handle_reconnected_does_not_trigger_rtds_recovery_after_cancellation() {
        let state = RtdsTestServerState::default();
        let addr = start_rtds_test_server(state.clone()).await;
        let (mut ctx, _data_rx) = make_ws_ctx();
        ctx.rtds_feed = crate::rtds::PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            ctx.clock,
            ctx.data_sender.clone(),
        );
        ctx.rtds_feed
            .track_subscribe(DataType::new(
                "PolymarketRtdsCryptoPrice",
                Some({
                    let mut metadata = Params::new();
                    metadata.insert("symbol".to_string(), Value::String("BTCUSDT".to_string()));
                    metadata
                }),
                None,
            ))
            .expect("track RTDS subscribe");

        ctx.cancellation_token.cancel();
        handle_ws_message(PolymarketWsMessage::Reconnected, &ctx);
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(state.received_payloads.lock().await.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_dedupes_mixed_slugs_when_condition_id_matches() {
        let state = NewMarketFetchTestServerState::default();
        let addr = start_new_market_test_server(state.clone()).await;
        let gamma_base_url = format!("http://{addr}");
        let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
        ctx.subscribe_new_markets = true;
        ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(0));

        let condition_id = "0xabc123";
        handle_market_message(
            make_new_market_with_condition("btc-updown-5m-window-a", condition_id, true),
            &ctx,
        );
        handle_market_message(
            make_new_market_with_condition("btc-updown-5m-window-b", condition_id, true),
            &ctx,
        );

        assert_eq!(state.total_requests.load(Ordering::SeqCst), 0);
        assert_eq!(ctx.new_market_inflight_keys.len(), 1);
        assert!(
            ctx.new_market_inflight_keys.contains_key("cond:0xabc123"),
            "mixed slug events with same condition_id should dedupe to one in-flight fetch",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_fetch_prefers_condition_id_query_over_slug_query() {
        let state = NewMarketFetchTestServerState::default();
        let addr = start_new_market_test_server(state.clone()).await;
        let gamma_base_url = format!("http://{addr}");
        let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
        ctx.subscribe_new_markets = true;
        ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        handle_market_message(
            make_new_market_with_ids(
                "btc-updown-5m-query-check",
                "0xmarket-condition-query",
                "0xcondition-query",
                true,
            ),
            &ctx,
        );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);

        loop {
            let done = state.total_requests.load(Ordering::SeqCst) >= 1
                && state.inflight_requests.load(Ordering::SeqCst) == 0
                && ctx.new_market_inflight_keys.is_empty();

            if done {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for condition_id query fetch to complete"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let condition_ids = state
            .seen_condition_ids
            .lock()
            .expect("seen_condition_ids mutex poisoned");
        let slugs = state.seen_slugs.lock().expect("seen_slugs mutex poisoned");
        assert_eq!(
            condition_ids.len(),
            1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS,
        );
        assert_eq!(slugs.len(), 1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS);
        assert!(
            condition_ids
                .iter()
                .all(|cid| cid.as_deref() == Some("0xcondition-query")),
        );
        assert_eq!(
            slugs.iter().filter(|slug| slug.is_none()).count(),
            1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS,
            "condition-aware path should not send slug query for new_market fetch"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn new_market_fetch_falls_back_to_slug_when_identifiers_missing() {
        let state = NewMarketFetchTestServerState::default();
        let addr = start_new_market_test_server(state.clone()).await;
        let gamma_base_url = format!("http://{addr}");
        let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
        ctx.subscribe_new_markets = true;
        ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        handle_market_message(
            make_new_market_with_ids("btc-updown-5m-slug-fallback", "", "", true),
            &ctx,
        );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);

        loop {
            let done = state.total_requests.load(Ordering::SeqCst) >= 1
                && state.inflight_requests.load(Ordering::SeqCst) == 0
                && ctx.new_market_inflight_keys.is_empty();

            if done {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for slug fallback fetch to complete"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let condition_ids = state
            .seen_condition_ids
            .lock()
            .expect("seen_condition_ids mutex poisoned");
        let slugs = state.seen_slugs.lock().expect("seen_slugs mutex poisoned");
        assert_eq!(condition_ids.len(), 1);
        assert_eq!(slugs.len(), 1);
        assert_eq!(condition_ids[0], None);
        assert_eq!(slugs[0].as_deref(), Some("btc-updown-5m-slug-fallback"));
    }

    #[rstest]
    fn new_market_dedupe_key_prefers_condition_then_market_then_slug() {
        let MarketWsMessage::NewMarket(mut nm) =
            make_new_market_with_condition("btc-updown-5m-window-a", "0xcond123", true)
        else {
            panic!("expected new_market message");
        };

        assert_eq!(new_market_dedupe_key(&nm), "cond:0xcond123");

        nm.condition_id.clear();
        nm.market = Ustr::from("0xmarket456");
        assert_eq!(new_market_dedupe_key(&nm), "market:0xmarket456");

        nm.market = Ustr::from("");
        nm.slug = "btc-updown-5m-window-b".to_string();
        assert_eq!(new_market_dedupe_key(&nm), "slug:btc-updown-5m-window-b");
    }

    fn make_market_resolved(
        condition_id: &str,
        winner_asset_id: &str,
        loser_asset_id: &str,
    ) -> MarketWsMessage {
        MarketWsMessage::MarketResolved(PolymarketMarketResolved {
            id: "resolved-1".to_string(),
            market: Ustr::from(condition_id),
            assets_ids: vec![winner_asset_id.to_string(), loser_asset_id.to_string()],
            winning_asset_id: winner_asset_id.to_string(),
            winning_outcome: "Yes".to_string(),
            timestamp: "1700000004000".to_string(),
            tags: vec![],
        })
    }

    fn make_gamma_market_value_with_outcome_prices(
        condition_id: &str,
        clob_token_ids: &str,
        outcome_prices: Option<&str>,
        closed: Option<bool>,
        accepting_orders: Option<bool>,
    ) -> Value {
        let mut value = serde_json::json!({
            "id": "1557558",
            "conditionId": condition_id,
            "questionID": "0xquestion",
            "clobTokenIds": clob_token_ids,
            "outcomes": "[\"Yes\",\"No\"]",
            "question": "Will test pass?",
            "description": null,
            "startDate": null,
            "endDate": null,
            "active": false,
            "closed": closed,
            "acceptingOrders": accepting_orders,
            "enableOrderBook": false,
            "slug": "test-market",
            "events": []
        });

        if let Some(outcome_prices) = outcome_prices {
            value["outcomePrices"] = serde_json::Value::String(outcome_prices.to_string());
        }

        value
    }

    fn make_clob_market_value(
        condition_id: &str,
        winner_token_id: &str,
        loser_token_id: &str,
        closed: bool,
    ) -> Value {
        serde_json::json!({
            "condition_id": condition_id,
            "closed": closed,
            "tokens": [
                {"token_id": winner_token_id, "outcome": "Yes", "winner": true},
                {"token_id": loser_token_id, "outcome": "No", "winner": false}
            ]
        })
    }

    #[derive(Clone, Default)]
    struct TestServerState {
        gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
        clob_market_by_condition: Arc<tokio::sync::Mutex<AHashMap<String, Value>>>,
        market_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
        market_cache_probe: Arc<StdMutex<Option<CacheProbe>>>,
        market_cache_at_connect: Arc<StdMutex<Vec<bool>>>,
    }

    async fn handle_gamma_markets(State(state): State<TestServerState>) -> Json<Value> {
        let body = state
            .gamma_response
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| serde_json::json!([]));
        Json(body)
    }

    async fn handle_gamma_markets_keyset(State(state): State<TestServerState>) -> Json<Value> {
        let Json(markets) = handle_gamma_markets(State(state)).await;
        Json(serde_json::json!({"markets": markets}))
    }

    async fn handle_clob_market(
        State(state): State<TestServerState>,
        Path(condition_id): Path<String>,
    ) -> (StatusCode, Json<Value>) {
        let body = state.clob_market_by_condition.lock().await;
        if let Some(value) = body.get(&condition_id) {
            (StatusCode::OK, Json(value.clone()))
        } else {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"market not found"})),
            )
        }
    }

    async fn handle_market_upgrade(
        ws: WebSocketUpgrade,
        State(state): State<TestServerState>,
    ) -> axum::response::Response {
        let cache_probe = state
            .market_cache_probe
            .lock()
            .expect("market_cache_probe mutex poisoned")
            .clone();

        if let Some(cache_probe) = cache_probe {
            state
                .market_cache_at_connect
                .lock()
                .expect("market_cache_at_connect mutex poisoned")
                .push(cache_probe());
        }

        ws.on_upgrade(move |socket| record_json_ws_payloads(socket, state.market_payloads))
    }

    async fn start_mock_server(state: TestServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let router = Router::new()
            .route("/markets", get(handle_gamma_markets))
            .route("/markets/keyset", get(handle_gamma_markets_keyset))
            .route("/markets/{condition_id}", get(handle_clob_market))
            .route("/ws/market", get(handle_market_upgrade))
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
        addr
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct ExpiredAutoLoadQuery {
        condition_ids: Option<String>,
        closed: Option<String>,
    }

    #[derive(Clone)]
    struct ExpiredAutoLoadServerState {
        queries: Arc<StdMutex<Vec<ExpiredAutoLoadQuery>>>,
        open_response: Value,
        closed_response: Value,
        market_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
    }

    async fn handle_expired_auto_load_markets(
        RawQuery(raw_query): RawQuery,
        State(state): State<ExpiredAutoLoadServerState>,
    ) -> Json<Value> {
        let condition_ids = query_param(raw_query.clone(), "condition_ids");
        let closed = query_param(raw_query, "closed");
        state
            .queries
            .lock()
            .expect("expired auto-load queries mutex poisoned")
            .push(ExpiredAutoLoadQuery {
                condition_ids,
                closed: closed.clone(),
            });

        if closed.as_deref() == Some("true") {
            Json(state.closed_response)
        } else {
            Json(state.open_response)
        }
    }

    async fn handle_expired_auto_load_markets_keyset(
        raw_query: RawQuery,
        state: State<ExpiredAutoLoadServerState>,
    ) -> Json<Value> {
        let Json(markets) = handle_expired_auto_load_markets(raw_query, state).await;
        Json(serde_json::json!({"markets": markets}))
    }

    async fn handle_expired_auto_load_market_upgrade(
        ws: WebSocketUpgrade,
        State(state): State<ExpiredAutoLoadServerState>,
    ) -> axum::response::Response {
        ws.on_upgrade(move |socket| record_json_ws_payloads(socket, state.market_payloads))
    }

    async fn start_expired_auto_load_test_server(state: ExpiredAutoLoadServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let router = Router::new()
            .route("/markets", get(handle_expired_auto_load_markets))
            .route(
                "/markets/keyset",
                get(handle_expired_auto_load_markets_keyset),
            )
            .route("/ws/market", get(handle_expired_auto_load_market_upgrade))
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
        addr
    }

    #[derive(Clone)]
    struct ScriptedAutoLoadReply {
        status: StatusCode,
        body: Value,
        delay: Duration,
        release: Option<Arc<tokio::sync::Semaphore>>,
    }

    impl ScriptedAutoLoadReply {
        fn ok(body: Value) -> Self {
            Self {
                status: StatusCode::OK,
                body,
                delay: Duration::ZERO,
                release: None,
            }
        }

        fn failed() -> Self {
            Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: serde_json::json!({"error": "probe failed"}),
                delay: Duration::ZERO,
                release: None,
            }
        }

        fn delayed(body: Value, delay: Duration) -> Self {
            Self {
                status: StatusCode::OK,
                body,
                delay,
                release: None,
            }
        }

        fn gated(body: Value, release: Arc<tokio::sync::Semaphore>) -> Self {
            Self {
                status: StatusCode::OK,
                body,
                delay: Duration::ZERO,
                release: Some(release),
            }
        }
    }

    #[derive(Clone, Default)]
    struct ScriptedAutoLoadServerState {
        queries: Arc<StdMutex<Vec<ExpiredAutoLoadQuery>>>,
        open_replies: Arc<StdMutex<VecDeque<ScriptedAutoLoadReply>>>,
        closed_replies: Arc<StdMutex<VecDeque<ScriptedAutoLoadReply>>>,
        market_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
        completed_replies: Arc<AtomicUsize>,
    }

    impl ScriptedAutoLoadServerState {
        fn new(
            open_replies: Vec<ScriptedAutoLoadReply>,
            closed_replies: Vec<ScriptedAutoLoadReply>,
        ) -> Self {
            Self {
                queries: Arc::new(StdMutex::new(Vec::new())),
                open_replies: Arc::new(StdMutex::new(open_replies.into())),
                closed_replies: Arc::new(StdMutex::new(closed_replies.into())),
                market_payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
                completed_replies: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    async fn next_scripted_auto_load_reply(
        raw_query: Option<String>,
        state: &ScriptedAutoLoadServerState,
    ) -> ScriptedAutoLoadReply {
        let condition_ids = query_param(raw_query.clone(), "condition_ids");
        let closed = query_param(raw_query, "closed");
        state
            .queries
            .lock()
            .expect("scripted auto-load queries mutex poisoned")
            .push(ExpiredAutoLoadQuery {
                condition_ids,
                closed: closed.clone(),
            });

        let replies = if closed.as_deref() == Some("true") {
            &state.closed_replies
        } else {
            &state.open_replies
        };
        replies
            .lock()
            .expect("scripted auto-load replies mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| ScriptedAutoLoadReply::ok(serde_json::json!([])))
    }

    async fn handle_scripted_auto_load_markets(
        RawQuery(raw_query): RawQuery,
        State(state): State<ScriptedAutoLoadServerState>,
    ) -> Response {
        let reply = next_scripted_auto_load_reply(raw_query, &state).await;

        if !reply.delay.is_zero() {
            tokio::time::sleep(reply.delay).await;
        }

        if let Some(release) = reply.release {
            release
                .acquire()
                .await
                .expect("scripted reply release")
                .forget();
        }
        state.completed_replies.fetch_add(1, Ordering::SeqCst);

        (reply.status, Json(reply.body)).into_response()
    }

    async fn handle_scripted_auto_load_markets_keyset(
        RawQuery(raw_query): RawQuery,
        State(state): State<ScriptedAutoLoadServerState>,
    ) -> Response {
        let reply = next_scripted_auto_load_reply(raw_query, &state).await;

        if !reply.delay.is_zero() {
            tokio::time::sleep(reply.delay).await;
        }

        if let Some(release) = reply.release {
            release
                .acquire()
                .await
                .expect("scripted reply release")
                .forget();
        }
        state.completed_replies.fetch_add(1, Ordering::SeqCst);

        let body = serde_json::json!({"markets": reply.body});
        (reply.status, Json(body)).into_response()
    }

    async fn handle_scripted_auto_load_market_upgrade(
        ws: WebSocketUpgrade,
        State(state): State<ScriptedAutoLoadServerState>,
    ) -> Response {
        ws.on_upgrade(move |socket| record_json_ws_payloads(socket, state.market_payloads))
    }

    async fn start_scripted_auto_load_test_server(
        state: ScriptedAutoLoadServerState,
    ) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let router = Router::new()
            .route("/markets", get(handle_scripted_auto_load_markets))
            .route(
                "/markets/keyset",
                get(handle_scripted_auto_load_markets_keyset),
            )
            .route("/ws/market", get(handle_scripted_auto_load_market_upgrade))
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
        addr
    }

    fn create_test_client(
        addr: SocketAddr,
    ) -> (
        PolymarketDataClient,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(tx);

        let base_url = format!("http://{addr}");
        let gamma =
            PolymarketGammaHttpClient::new(Some(base_url.clone()), 5, RetryConfig::default())
                .expect("gamma client");
        let clob_public =
            PolymarketClobPublicClient::new(Some(base_url.clone()), 5).expect("clob client");
        let data_api =
            PolymarketDataApiHttpClient::new(Some(base_url.clone()), 5).expect("data api client");
        let ws = PolymarketMarketConnectionPool::new(
            Some(format!("ws://{addr}/ws/market")),
            false,
            TransportBackend::default(),
            crate::common::consts::WS_DEFAULT_SUBSCRIPTIONS,
        );

        let config = PolymarketDataClientConfig {
            base_url_http: Some(base_url.clone()),
            base_url_ws: Some(format!("ws://{addr}/ws")),
            base_url_gamma: Some(base_url.clone()),
            base_url_data_api: Some(base_url),
            resolve_poll_enabled: false,
            ..PolymarketDataClientConfig::default()
        };

        let client = PolymarketDataClient::new(
            *POLYMARKET_CLIENT_ID,
            config,
            gamma,
            clob_public,
            data_api,
            ws,
        );

        (client, rx)
    }

    fn make_local_test_client() -> PolymarketDataClient {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(tx);

        let gamma = PolymarketGammaHttpClient::new(
            Some("http://localhost".to_string()),
            5,
            RetryConfig::default(),
        )
        .expect("gamma client");
        let clob_public = PolymarketClobPublicClient::new(Some("http://localhost".to_string()), 5)
            .expect("clob client");
        let data_api = PolymarketDataApiHttpClient::new(Some("http://localhost".to_string()), 5)
            .expect("data api client");
        let ws = PolymarketMarketConnectionPool::new(
            Some("ws://localhost/ws/market".to_string()),
            false,
            TransportBackend::default(),
            crate::common::consts::WS_DEFAULT_SUBSCRIPTIONS,
        );

        PolymarketDataClient::new(
            *POLYMARKET_CLIENT_ID,
            PolymarketDataClientConfig::default(),
            gamma,
            clob_public,
            data_api,
            ws,
        )
    }

    #[rstest]
    fn market_resolved_emits_grouped_close_and_removes_watch_entry() {
        let (ctx, mut data_rx) = make_ws_ctx();
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                min_order_size: None,
                neg_risk: None,
                expiration_ns: Some(expiration_ns),
                market_closed: None,
            },
        );
        let no = seed_instrument_with_context(
            &ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                min_order_size: None,
                neg_risk: None,
                expiration_ns: Some(expiration_ns),
                market_closed: None,
            },
        );

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(yes.id()),
        );
        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(no.id()),
        );

        handle_market_message(
            make_market_resolved("0xCOND-BTC", "0xTOKEN_YES", "0xTOKEN_NO"),
            &ctx,
        );

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        let statuses = events
            .iter()
            .filter(|event| matches!(event, DataEvent::InstrumentStatus(_)))
            .count();
        assert_eq!(statuses, 2);

        let mut yes_close = None;
        let mut no_close = None;

        for event in events {
            if let DataEvent::Data(NautilusData::InstrumentClose(close)) = event {
                if close.instrument_id == yes.id() {
                    yes_close = Some(close);
                } else if close.instrument_id == no.id() {
                    no_close = Some(close);
                }
            }
        }

        let yes_close = yes_close.expect("expected yes close");
        let no_close = no_close.expect("expected no close");
        assert_eq!(yes_close.close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(no_close.close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(
            yes_close.close_price.as_decimal(),
            rust_decimal::Decimal::ONE
        );
        assert_eq!(
            no_close.close_price.as_decimal(),
            rust_decimal::Decimal::ZERO
        );
        assert!(
            !ctx.resolve_poll_watchlist
                .contains_key(&"0xCOND-BTC".to_string())
        );
    }

    #[rstest]
    fn duplicate_market_resolved_after_watch_removal_is_a_noop() {
        let (ctx, mut data_rx) = make_ws_ctx();
        let yes = seed_instrument_with_context(
            &ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-BTC"),
                expiration_ns: Some(UnixNanos::from(1_000_000_000)),
                ..SeedInstrumentContext::default()
            },
        );

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(yes.id()),
        );

        let resolved = make_market_resolved("0xCOND-BTC", "0xTOKEN_YES", "0xTOKEN_NO");
        handle_market_message(resolved.clone(), &ctx);
        let _ = std::iter::from_fn(|| data_rx.try_recv().ok()).collect::<Vec<_>>();

        handle_market_message(resolved, &ctx);
        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    fn market_resolved_emit_failure_merges_watch_entry_back() {
        let (ctx, data_rx) = make_ws_ctx();
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                min_order_size: None,
                neg_risk: None,
                expiration_ns: Some(expiration_ns),
                market_closed: None,
            },
        );
        let no = seed_instrument_with_context(
            &ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                min_order_size: None,
                neg_risk: None,
                expiration_ns: Some(expiration_ns),
                market_closed: None,
            },
        );

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(yes.id()),
        );
        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(no.id()),
        );

        drop(data_rx);

        handle_market_message(
            make_market_resolved("0xCOND-BTC", "0xTOKEN_YES", "0xTOKEN_NO"),
            &ctx,
        );

        let watchlist = ctx.resolve_poll_watchlist.load();
        let entry = watchlist
            .get("0xCOND-BTC")
            .expect("expected watch entry restored after emit failure");
        assert_eq!(entry.tracked.len(), 2);
    }

    #[rstest]
    #[tokio::test]
    async fn request_data_manual_fallback_resolves_paused_entries() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-REQ",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(1_000_000_000);
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );

        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );
        pause_resolve_watch_entries(&client.resolve_poll_watchlist, &["0xCOND-REQ".to_string()]);

        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );
        client.request_data(request).expect("request_data");

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-REQ".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 2
        })
        .await;

        assert!(
            events.iter().any(is_resolve_response),
            "expected custom data response, received: {events:?}"
        );
        let response = events
            .iter()
            .find_map(|event| match event {
                DataEvent::Response(DataResponse::Data(response)) => Some(response),
                _ => None,
            })
            .expect("expected custom data response");
        let custom = response
            .data
            .as_ref()
            .downcast_ref::<ModelCustomData>()
            .expect("expected CustomData response payload");
        assert_eq!(custom.data_type.type_name(), RESOLVE_REQUEST_TYPE_NAME);
        let summary = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketResolveRequestSummaryData>()
            .expect("expected resolve summary payload");
        assert_eq!(
            summary.emitted_condition_ids,
            vec!["0xCOND-REQ".to_string()]
        );
        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 2);
    }

    #[rstest]
    #[tokio::test]
    async fn request_data_manual_fallback_with_auto_poll_disabled_resolves_expired_entries() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-REQ",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(60_000_000_000),
        );
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );

        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );

        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );
        client.request_data(request).expect("request_data");

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-REQ".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 2
        })
        .await;

        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 2);
    }

    #[rstest]
    #[tokio::test]
    async fn request_data_manual_fallback_uses_clob_when_gamma_is_not_strict() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-REQ",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                Some("[\"0.58\",\"0.42\"]"),
                Some(true),
                Some(false),
            )
        ]));
        state.clob_market_by_condition.lock().await.insert(
            "0xCOND-REQ".to_string(),
            make_clob_market_value("0xCOND-REQ", "0xTOKEN_YES", "0xTOKEN_NO", true),
        );

        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(60_000_000_000),
        );
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );

        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );

        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );
        client.request_data(request).expect("request_data");

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-REQ".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 2
        })
        .await;

        let response = events
            .iter()
            .find_map(|event| match event {
                DataEvent::Response(DataResponse::Data(response)) => Some(response),
                _ => None,
            })
            .expect("expected custom data response");
        let custom = response
            .data
            .as_ref()
            .downcast_ref::<ModelCustomData>()
            .expect("expected CustomData response payload");
        let summary = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketResolveRequestSummaryData>()
            .expect("expected resolve summary payload");
        assert_eq!(summary.resolved_markets, 1);
        assert_eq!(summary.skipped_non_binary_markets, 1);
        assert_eq!(summary.clob_fallback_successes, 1);
        assert_eq!(
            summary.emitted_condition_ids,
            vec!["0xCOND-REQ".to_string()]
        );

        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 2);
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_fallback_clob_success_after_gamma_error_does_not_mark_failed() {
        let state = TestServerState::default();
        state.clob_market_by_condition.lock().await.insert(
            "0xCOND-REQ".to_string(),
            make_clob_market_value("0xCOND-REQ", "0xTOKEN_YES", "0xTOKEN_NO", true),
        );

        let addr = start_mock_server(state).await;
        let (client, _data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(60_000_000_000),
        );
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );

        let failing_gamma = PolymarketGammaHttpClient::new(
            Some("http://127.0.0.1:1".to_string()),
            1,
            RetryConfig {
                max_retries: 0,
                initial_delay_ms: 1,
                max_delay_ms: 1,
                backoff_factor: 1.0,
                jitter_ms: 0,
                operation_timeout_ms: Some(200),
                immediate_first: true,
                max_elapsed_ms: Some(200),
            },
        )
        .expect("gamma client");

        let stats = fetch_and_apply_resolutions_by_condition_ids(
            &failing_gamma,
            &client.clob_public_client,
            &ws_ctx.resolve_context(),
            &["0xCOND-REQ".to_string()],
            ResolveBatchErrorMode::StopOnFirstError,
        )
        .await;

        assert_eq!(stats.resolved_markets, 1);
        assert_eq!(stats.clob_fallback_successes, 1);
        assert_eq!(stats.emitted_condition_ids, vec!["0xCOND-REQ".to_string()]);
        assert!(stats.failed_condition_ids.is_empty());
        assert_eq!(stats.error, None);
    }

    #[rstest]
    #[tokio::test]
    async fn request_data_explicit_multiple_condition_ids_resolves_all_requested_conditions() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-A",
                "[\"0xA_YES\",\"0xA_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            ),
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-B",
                "[\"0xB_YES\",\"0xB_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(1_000_000_000);
        let instruments = [
            seed_instrument_with_context(
                &ws_ctx,
                "0xA_YES",
                Price::from("0.001"),
                Quantity::from("0.01"),
                SeedInstrumentContext {
                    condition_id: Some("0xCOND-A"),
                    expiration_ns: Some(expiration_ns),
                    ..SeedInstrumentContext::default()
                },
            ),
            seed_instrument_with_context(
                &ws_ctx,
                "0xA_NO",
                Price::from("0.001"),
                Quantity::from("0.01"),
                SeedInstrumentContext {
                    condition_id: Some("0xCOND-A"),
                    expiration_ns: Some(expiration_ns),
                    ..SeedInstrumentContext::default()
                },
            ),
            seed_instrument_with_context(
                &ws_ctx,
                "0xB_YES",
                Price::from("0.001"),
                Quantity::from("0.01"),
                SeedInstrumentContext {
                    condition_id: Some("0xCOND-B"),
                    expiration_ns: Some(expiration_ns),
                    ..SeedInstrumentContext::default()
                },
            ),
            seed_instrument_with_context(
                &ws_ctx,
                "0xB_NO",
                Price::from("0.001"),
                Quantity::from("0.01"),
                SeedInstrumentContext {
                    condition_id: Some("0xCOND-B"),
                    expiration_ns: Some(expiration_ns),
                    ..SeedInstrumentContext::default()
                },
            ),
        ];

        for instrument in &instruments {
            upsert_resolve_watch_entry_from_instrument(
                &client.resolve_poll_watchlist,
                instrument,
                PositionId::new("P-1"),
            );
        }

        let mut params = Params::new();
        params.insert(
            "condition_ids".to_string(),
            serde_json::json!(["0xCOND-A", "0xCOND-B"]),
        );
        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            Some(params),
        );
        client.request_data(request).expect("request_data");

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-A".to_string())
                    && !client
                        .resolve_poll_watchlist
                        .contains_key(&"0xCOND-B".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 4
        })
        .await;

        let response = events
            .iter()
            .find_map(|event| match event {
                DataEvent::Response(DataResponse::Data(response)) => Some(response),
                _ => None,
            })
            .expect("expected custom data response");
        let custom = response
            .data
            .as_ref()
            .downcast_ref::<ModelCustomData>()
            .expect("expected CustomData response payload");
        let summary = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketResolveRequestSummaryData>()
            .expect("expected resolve summary payload");
        assert_eq!(
            summary.requested_condition_ids,
            vec!["0xCOND-A".to_string(), "0xCOND-B".to_string()]
        );
        assert_eq!(summary.resolved_markets, 2);
        assert_eq!(
            summary.emitted_condition_ids,
            vec!["0xCOND-A".to_string(), "0xCOND-B".to_string()]
        );

        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 4);
    }

    #[rstest]
    #[tokio::test]
    async fn request_data_explicit_invalid_selector_does_not_fallback_to_watchlist() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-REQ",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(1_000_000_000);
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );
        pause_resolve_watch_entries(&client.resolve_poll_watchlist, &["0xCOND-REQ".to_string()]);

        let mut params = Params::new();
        params.insert(
            "instrument_ids".to_string(),
            serde_json::json!(["BTCUSDT-PERP.BINANCE"]),
        );
        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            Some(params),
        );
        client.request_data(request).expect("request_data");

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response)
        })
        .await;

        let response = events
            .iter()
            .find_map(|event| match event {
                DataEvent::Response(DataResponse::Data(response)) => Some(response),
                _ => None,
            })
            .expect("expected custom data response");
        let custom = response
            .data
            .as_ref()
            .downcast_ref::<ModelCustomData>()
            .expect("expected CustomData response payload");
        let summary = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketResolveRequestSummaryData>()
            .expect("expected resolve summary payload");
        assert!(!summary.used_watchlist_fallback);
        assert_eq!(summary.requested_condition_ids, Vec::<String>::new());
        assert!(summary.error.is_some());

        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 0);
        assert!(
            client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-REQ".to_string())
        );
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_poll_task_emits_grouped_close_for_expired_watch_entries() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-POLL",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                None,
                Some(false),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        client.config.resolve_poll_enabled = true;
        client.config.resolve_poll_interval_secs = 1;
        client.config.resolve_poll_grace_secs = 0;
        client.config.resolve_poll_max_wait_secs = 300;

        let ws_ctx = make_client_ws_ctx(&client);
        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(1_000_000_000),
        );
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xCOND-POLL-YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-POLL"),
                expiration_ns: Some(expiration_ns),
                market_closed: Some(false),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xCOND-POLL-NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-POLL"),
                expiration_ns: Some(expiration_ns),
                market_closed: Some(false),
                ..SeedInstrumentContext::default()
            },
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );

        client.spawn_resolve_poll_task();
        tokio::time::sleep(StdDuration::from_millis(200)).await;
        state.gamma_response.lock().await.as_mut().unwrap()[0]["closed"] = true.into();

        wait_until_async(
            || async {
                let loaded = client.instruments.load();
                let closed = loaded
                    .get(&inst_yes.id())
                    .map(crate::filters::market_closed);
                closed == Some(Some(true))
            },
            StdDuration::from_secs(5),
        )
        .await;
        state.gamma_response.lock().await.as_mut().unwrap()[0]["outcomePrices"] =
            serde_json::json!("[1,0]");

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-POLL".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await
            .expect("new market fetch tasks terminated");

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(1), |events| {
            count_instrument_close_events(events) >= 2
        })
        .await;
        let closes = count_instrument_close_events(&events);

        assert_eq!(closes, 2);
        assert!(
            !client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-POLL".to_string())
        );
    }

    #[rstest]
    #[case::quotes(false)]
    #[case::book_deltas(true)]
    #[tokio::test]
    async fn auto_load_data_subscription_caches_before_ws_subscribe(#[case] deltas: bool) {
        let state = TestServerState::default();
        *state.gamma_response.lock().await =
            Some(serde_json::json!([gamma_market_recheck_fixture_value()]));
        let addr = start_mock_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let instrument_id = fixture_yes_instrument_id();
        let instruments = client.instruments.clone();
        *state
            .market_cache_probe
            .lock()
            .expect("market_cache_probe mutex poisoned") = Some(Arc::new(move || {
            instruments.load().contains_key(&instrument_id)
        }));

        assert_eq!(client.ws_client.connection_count(), 0);

        let result = if deltas {
            client.subscribe_book_deltas(SubscribeBookDeltas::new(
                instrument_id,
                BookType::L2_MBP,
                Some(client.client_id),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                true,
                None,
                None,
            ))
        } else {
            client.subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
        };
        result.expect("subscription should queue auto-load");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.market_payloads.lock().await.is_empty() }
            },
            StdDuration::from_secs(3),
        )
        .await;

        let emitted_instrument = tokio::time::timeout(StdDuration::from_secs(1), async {
            loop {
                match data_rx.recv().await {
                    Some(DataEvent::Instrument(instrument)) if instrument.id() == instrument_id => {
                        return instrument;
                    }
                    Some(_) => {}
                    None => panic!("data event channel closed before instrument publication"),
                }
            }
        })
        .await
        .expect("timed out waiting for instrument publication");

        let payloads = state.market_payloads.lock().await.clone();
        let cache_at_connect = state
            .market_cache_at_connect
            .lock()
            .expect("market_cache_at_connect mutex poisoned")
            .clone();
        let cached_instrument = client
            .instruments
            .load()
            .get(&instrument_id)
            .cloned()
            .expect("instrument should be cached");
        client
            .ws_client
            .disconnect()
            .await
            .expect("disconnect failed");

        assert_eq!(emitted_instrument.raw_symbol().as_str(), TEST_TOKEN_ID_YES);
        assert_eq!(cached_instrument.raw_symbol().as_str(), TEST_TOKEN_ID_YES);
        assert_eq!(client.active_delta_subs.contains(&instrument_id), deltas);
        assert_eq!(client.active_quote_subs.contains(&instrument_id), !deltas);
        assert!(!client.order_books.contains_key(&instrument_id));
        assert_eq!(cache_at_connect, vec![true]);
        assert_eq!(
            payloads,
            vec![serde_json::json!({
                "assets_ids": [TEST_TOKEN_ID_YES],
                "type": "market",
                "initial_dump": true,
            })],
        );
    }

    #[rstest]
    #[tokio::test]
    async fn auto_load_closed_future_instrument_retires_without_retrying() {
        let filter_calls = Arc::new(AtomicUsize::new(0));
        let state = ExpiredAutoLoadServerState {
            queries: Arc::new(StdMutex::new(Vec::new())),
            open_response: serde_json::json!([]),
            closed_response: serde_json::json!([gamma_market_future_closed_fixture_value()]),
            market_payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        };
        let addr = start_expired_auto_load_test_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        let filter_calls_clone = filter_calls.clone();
        client.add_instrument_filter(Arc::new(crate::filters::PredicateFilter::new(
            "count-calls",
            move |_| {
                filter_calls_clone.fetch_add(1, Ordering::SeqCst);
                true
            },
        )));
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 3;
        client.config.auto_load_retry_delay_initial_secs = 0.0;
        client.config.auto_load_retry_delay_max_secs = 0.0;

        let instrument_id = fixture_yes_instrument_id();
        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("subscribe_quotes should queue auto-load");

        wait_until_async(
            || {
                let client = &client;
                async move {
                    !client.active_quote_subs.contains(&instrument_id)
                        && client
                            .pending_auto_loads
                            .lock()
                            .expect("pending_auto_loads mutex poisoned")
                            .is_empty()
                        && !client.auto_load_scheduled.load(Ordering::Acquire)
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert_eq!(
            *state
                .queries
                .lock()
                .expect("expired auto-load queries mutex poisoned"),
            vec![
                ExpiredAutoLoadQuery {
                    condition_ids: Some(TEST_CONDITION_ID.to_string()),
                    closed: None,
                },
                ExpiredAutoLoadQuery {
                    condition_ids: Some(TEST_CONDITION_ID.to_string()),
                    closed: Some("true".to_string()),
                },
            ],
        );
        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert_eq!(filter_calls.load(Ordering::SeqCst), 0);
        assert!(data_rx.try_recv().is_err());
        assert!(state.market_payloads.lock().await.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn auto_load_closed_condition_retires_live_sibling_instrument() {
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([]))],
            vec![ScriptedAutoLoadReply::delayed(
                serde_json::json!([gamma_market_future_closed_fixture_value()]),
                Duration::from_millis(200),
            )],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let sibling = instruments_from_gamma_fixture(gamma_market_recheck_fixture_value())
            .into_iter()
            .find(|instrument| instrument.id() == fixture_no_instrument_id())
            .expect("No sibling instrument");
        let sibling_id = sibling.id();
        cache_instrument_unchecked(&client.instruments, &client.token_meta, &sibling);
        client
            .subscribe_quotes(SubscribeQuotes::new(
                sibling_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("cached sibling subscription");
        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.market_payloads.lock().await.is_empty() }
            },
            StdDuration::from_secs(3),
        )
        .await;
        client.active_delta_subs.insert(sibling_id);
        client.active_trade_subs.insert(sibling_id);
        client.pending_snapshot_after_tick_change.insert(sibling_id);
        client
            .order_books
            .insert(sibling_id, OrderBook::new(sibling_id, BookType::L2_MBP));
        client.last_quotes.insert(
            sibling_id,
            QuoteTick::new(
                sibling_id,
                Price::from("0.49"),
                Price::from("0.51"),
                Quantity::from("1"),
                Quantity::from("1"),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
        );

        let requested_id = fixture_yes_instrument_id();
        client
            .subscribe_quotes(SubscribeQuotes::new(
                requested_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("missing sibling subscription should queue auto-load");
        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        >= 2
                }
            },
            StdDuration::from_secs(3),
        )
        .await;
        client
            .pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned")
            .insert(sibling_id);

        wait_until_async(
            || {
                let client = &client;
                async move {
                    !client.active_quote_subs.contains(&requested_id)
                        && !client.active_quote_subs.contains(&sibling_id)
                        && !client.instruments.load().contains_key(&sibling_id)
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert!(!client.active_quote_subs.contains(&sibling_id));
        assert!(!client.active_delta_subs.contains(&sibling_id));
        assert!(!client.active_trade_subs.contains(&sibling_id));
        assert!(!client.instruments.load().contains_key(&sibling_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_NO))
        );
        assert!(!client.order_books.contains_key(&sibling_id));
        assert!(!client.last_quotes.contains_key(&sibling_id));
        assert!(
            !client
                .pending_snapshot_after_tick_change
                .contains(&sibling_id)
        );
        assert!(
            !client
                .pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned")
                .contains(&sibling_id)
        );
        assert!(
            !client
                .ws_open_tokens
                .contains(&Ustr::from(TEST_TOKEN_ID_NO))
        );

        let query_count = state
            .queries
            .lock()
            .expect("scripted auto-load queries mutex poisoned")
            .len();
        let payload_count = state.market_payloads.lock().await.len();

        for instrument_id in [requested_id, sibling_id] {
            let _ = client.subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ));
        }
        // Quiet period: terminal resubscriptions must not enqueue a later auto-load or WS payload.
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(
            state
                .queries
                .lock()
                .expect("scripted auto-load queries mutex poisoned")
                .len(),
            query_count,
        );
        assert_eq!(state.market_payloads.lock().await.len(), payload_count);
        assert!(!client.active_quote_subs.contains(&requested_id));
        assert!(!client.active_quote_subs.contains(&sibling_id));
    }

    #[rstest]
    #[tokio::test]
    async fn terminal_closure_cannot_race_delta_subscription_into_recreating_order_book() {
        let addr = start_mock_server(TestServerState::default()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.compute_effective_deltas = true;

        let instrument = instrument_from_gamma_fixture(gamma_market_recheck_fixture_value());
        let instrument_id = instrument.id();
        cache_instrument_unchecked(&client.instruments, &client.token_meta, &instrument);
        client.ws_open_tokens.insert(Ustr::from(TEST_TOKEN_ID_YES));

        let closed_condition_ids = client.closed_condition_ids.clone();
        let closed_condition_ids_observer = closed_condition_ids.clone();
        let instruments = client.instruments.clone();
        let token_meta = client.token_meta.clone();
        let order_books = client.order_books.clone();
        let last_quotes = client.last_quotes.clone();
        let active_quote_subs = client.active_quote_subs.clone();
        let active_delta_subs = client.active_delta_subs.clone();
        let active_delta_subs_observer = active_delta_subs.clone();
        let active_trade_subs = client.active_trade_subs.clone();
        let resolve_poll_watchlist = client.resolve_poll_watchlist.clone();
        let pending_snapshot_after_tick_change = client.pending_snapshot_after_tick_change.clone();
        let pending_auto_loads = client.pending_auto_loads.clone();
        let ws_open_tokens = client.ws_open_tokens.clone();
        let ws_sub_mutex = client.ws_sub_mutex.clone();
        let ws = client.ws_client.handle();

        // Start with delta intent present, then race a second intent insertion after terminal
        // registration. Closure must remove the first and reject the second without recreating
        // its effective-delta book.
        client.active_delta_subs.insert(instrument_id);

        let closure_thread = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("closure test runtime")
                .block_on(crate::data::runtime::retire_closed_condition_state(
                    TEST_CONDITION_ID,
                    [instrument_id],
                    &closed_condition_ids,
                    &instruments,
                    &token_meta,
                    &order_books,
                    &last_quotes,
                    &active_quote_subs,
                    &active_delta_subs,
                    &active_trade_subs,
                    &resolve_poll_watchlist,
                    &pending_snapshot_after_tick_change,
                    &pending_auto_loads,
                    &ws_open_tokens,
                    &ws_sub_mutex,
                    &ws,
                    None,
                ));
        });

        let deadline = std::time::Instant::now() + StdDuration::from_secs(3);

        while active_delta_subs_observer.contains(&instrument_id)
            || !crate::data::runtime::is_condition_closed(
                &closed_condition_ids_observer,
                TEST_CONDITION_ID,
            )
        {
            assert!(
                std::time::Instant::now() < deadline,
                "closure did not retire delta intent before timeout"
            );
            std::thread::yield_now();
        }

        assert!(!client.add_delta_subscription_intent(instrument_id));
        let recreated_order_book = client.order_books.contains_key(&instrument_id);
        closure_thread.join().expect("closure thread");

        assert!(!recreated_order_book);
        assert!(!client.active_delta_subs.contains(&instrument_id));
        assert!(!client.order_books.contains_key(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(
            !client
                .ws_open_tokens
                .contains(&Ustr::from(TEST_TOKEN_ID_YES))
        );
    }

    #[rstest]
    #[tokio::test]
    async fn auto_load_normal_response_explicit_closed_is_terminal() {
        let filter_calls = Arc::new(AtomicUsize::new(0));
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([
                gamma_market_future_closed_fixture_value()
            ]))],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        let filter_calls_clone = filter_calls.clone();
        client.add_instrument_filter(Arc::new(crate::filters::PredicateFilter::new(
            "count-calls",
            move |_| {
                filter_calls_clone.fetch_add(1, Ordering::SeqCst);
                true
            },
        )));
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 3;
        client.config.auto_load_retry_delay_initial_secs = 0.0;
        client.config.auto_load_retry_delay_max_secs = 0.0;

        let instrument_id = fixture_yes_instrument_id();
        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("missing closed instrument subscription should queue auto-load");

        wait_until_async(
            || {
                let client = &client;
                async move { !client.active_quote_subs.contains(&instrument_id) }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert_eq!(
            state
                .queries
                .lock()
                .expect("scripted auto-load queries mutex poisoned")
                .as_slice(),
            &[ExpiredAutoLoadQuery {
                condition_ids: Some(TEST_CONDITION_ID.to_string()),
                closed: None,
            }],
        );
        assert_eq!(filter_calls.load(Ordering::SeqCst), 0);
        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(client.ws_open_tokens.is_empty());
        assert!(state.market_payloads.lock().await.is_empty());
        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn reset_isolates_delayed_auto_load_generation() {
        let old_reply_release = Arc::new(tokio::sync::Semaphore::new(0));
        let state = ScriptedAutoLoadServerState::new(
            vec![
                ScriptedAutoLoadReply::gated(
                    serde_json::json!([gamma_market_future_closed_fixture_value()]),
                    old_reply_release.clone(),
                ),
                ScriptedAutoLoadReply::ok(serde_json::json!(
                    [gamma_market_recheck_fixture_value()]
                )),
            ],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let instrument_id = fixture_yes_instrument_id();
        let subscribe = |client: &mut PolymarketDataClient| {
            client.subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
        };
        subscribe(&mut client).expect("old-generation subscription");
        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        == 1
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        client.reset_client();
        subscribe(&mut client).expect("new-generation subscription");
        wait_until_async(
            || {
                let client = &client;
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        == 2
                        && client.instruments.load().contains_key(&instrument_id)
                        && client.active_quote_subs.contains(&instrument_id)
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        while data_rx.try_recv().is_ok() {}

        old_reply_release.add_permits(1);
        // Quiet period: cancellation may drop the old HTTP request before the gated server handler
        // completes, and the detached task has no completion handle. Allow any stale mutation or
        // publication to become observable before asserting isolation.
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(
            !client
                .closed_condition_ids
                .lock()
                .expect("closed_condition_ids mutex poisoned")
                .contains(TEST_CONDITION_ID)
        );
        assert!(client.active_quote_subs.contains(&instrument_id));
        assert!(client.instruments.load().contains_key(&instrument_id));
        assert!(
            client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(
            !client
                .pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned")
                .contains(&instrument_id)
        );
        assert!(data_rx.try_recv().is_err());
        assert_eq!(state.completed_replies.load(Ordering::SeqCst), 1);
        assert_eq!(
            state
                .queries
                .lock()
                .expect("scripted auto-load queries mutex poisoned")
                .len(),
            2,
        );
    }

    #[rstest]
    #[tokio::test]
    async fn reset_isolates_closed_application_after_http_completion() {
        let reply_release = Arc::new(tokio::sync::Semaphore::new(0));
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::gated(
                serde_json::json!([gamma_market_future_closed_fixture_value()]),
                reply_release.clone(),
            )],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let instrument_id = fixture_yes_instrument_id();
        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("old-generation subscription");
        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        == 1
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        let old_pending = client.pending_auto_loads.clone();
        let (pending_locked_tx, pending_locked_rx) = std::sync::mpsc::sync_channel(1);
        let (pending_release_tx, pending_release_rx) = std::sync::mpsc::sync_channel(1);
        let pending_lock_thread = std::thread::spawn(move || {
            let _guard = old_pending
                .lock()
                .expect("pending_auto_loads mutex poisoned");
            pending_locked_tx
                .send(())
                .expect("signal pending auto-load gate");
            pending_release_rx
                .recv()
                .expect("release pending auto-load gate");
        });
        pending_locked_rx
            .recv_timeout(StdDuration::from_secs(3))
            .expect("pending auto-load gate");
        let old_closed_condition_ids = client.closed_condition_ids.clone();
        reply_release.add_permits(1);
        wait_until_async(
            || {
                let closed_condition_ids = old_closed_condition_ids.clone();
                async move {
                    closed_condition_ids
                        .lock()
                        .expect("closed_condition_ids mutex poisoned")
                        .contains(TEST_CONDITION_ID)
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        client.reset_client();
        let new_instrument = instrument_from_gamma_fixture(gamma_market_recheck_fixture_value());
        cache_instrument_unchecked(&client.instruments, &client.token_meta, &new_instrument);
        client.active_quote_subs.insert(instrument_id);
        pending_release_tx
            .send(())
            .expect("release pending auto-load gate");
        pending_lock_thread
            .join()
            .expect("pending auto-load gate thread");
        // Quiet period: the detached old-generation task has no completion handle. Give its
        // cancellation branch time to run before checking that no old cache mutation crossed reset.
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(client.active_quote_subs.contains(&instrument_id));
        assert!(client.instruments.load().contains_key(&instrument_id));
        assert!(
            client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
    }

    #[rstest]
    #[tokio::test]
    async fn cancellation_drops_delayed_closure_refresh_before_mutation() {
        let reply_release = Arc::new(tokio::sync::Semaphore::new(0));
        let mut closed_market = gamma_market_expired_fixture_value();
        closed_market["closed"] = Value::Bool(true);
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::gated(
                serde_json::json!([closed_market]),
                reply_release.clone(),
            )],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        client.config.resolve_poll_interval_secs = 1;

        let mut instrument = instrument_from_gamma_fixture(gamma_market_expired_fixture_value());
        if let InstrumentAny::BinaryOption(binary) = &mut instrument {
            binary.expiration_ns = UnixNanos::from(1);
            crate::filters::set_market_closed(binary, false);
        }
        let instrument_id = instrument.id();
        client.instruments.insert(instrument_id, instrument);
        client.spawn_resolve_poll_task();
        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        == 1
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        let closed_condition_ids = client.closed_condition_ids.clone();
        let (locked_tx, locked_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let lock_thread = std::thread::spawn(move || {
            let _guard = closed_condition_ids
                .lock()
                .expect("closed_condition_ids mutex poisoned");
            locked_tx.send(()).expect("signal closure application gate");
            release_rx.recv().expect("release closure application gate");
        });
        locked_rx
            .recv_timeout(StdDuration::from_secs(3))
            .expect("closure application gate");

        reply_release.add_permits(1);
        wait_until_async(
            || {
                let state = state.clone();
                async move { state.completed_replies.load(Ordering::SeqCst) == 1 }
            },
            StdDuration::from_secs(3),
        )
        .await;

        client.stop_client();
        release_tx
            .send(())
            .expect("release closure application gate");
        lock_thread.join().expect("closure application gate thread");
        // Quiet period: cancellation after HTTP completion must still prevent positive closure
        // evidence or publication from crossing the application boundary.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let cached = client
            .instruments
            .get_cloned(&instrument_id)
            .expect("cached instrument");
        assert_eq!(crate::filters::market_closed(&cached), Some(false));
        assert!(
            !client
                .closed_condition_ids
                .lock()
                .expect("closed_condition_ids mutex poisoned")
                .contains(TEST_CONDITION_ID)
        );
        assert!(data_rx.try_recv().is_err());
        client.reset_client();
    }

    #[rstest]
    #[tokio::test]
    async fn auto_load_transient_closed_market_uses_positive_closure_probe() {
        let mut closed_unusable = gamma_market_future_closed_fixture_value();
        closed_unusable["clobTokenIds"] = Value::String("[]".to_string());
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([
                closed_unusable.clone()
            ]))],
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([
                closed_unusable
            ]))],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let instrument_id = fixture_yes_instrument_id();
        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("unusable closed market subscription should queue auto-load");

        wait_until_async(
            || {
                let client = &client;
                async move { !client.active_quote_subs.contains(&instrument_id) }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert_eq!(
            state
                .queries
                .lock()
                .expect("scripted auto-load queries mutex poisoned")
                .as_slice(),
            &[
                ExpiredAutoLoadQuery {
                    condition_ids: Some(TEST_CONDITION_ID.to_string()),
                    closed: None,
                },
                ExpiredAutoLoadQuery {
                    condition_ids: Some(TEST_CONDITION_ID.to_string()),
                    closed: Some("true".to_string()),
                },
            ],
        );
        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(client.ws_open_tokens.is_empty());
        assert!(state.market_payloads.lock().await.is_empty());
        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn closure_refresh_registers_shared_terminal_condition() {
        let mut closed_market = gamma_market_expired_fixture_value();
        closed_market["closed"] = Value::Bool(true);
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([
                closed_market
            ]))],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state).await;
        let (client, _data_rx) = create_test_client(addr);
        let mut instrument = instrument_from_gamma_fixture(gamma_market_expired_fixture_value());
        if let InstrumentAny::BinaryOption(binary) = &mut instrument {
            binary.expiration_ns = UnixNanos::from(1);
            crate::filters::set_market_closed(binary, false);
        }
        client.instruments.insert(instrument.id(), instrument);

        let updated = crate::data::instruments::refresh_expired_market_closure(
            client.provider.http_client(),
            &client.instruments,
            &client.data_sender,
            UnixNanos::from(u64::MAX),
            &client.closed_condition_ids,
            &client.ws_sub_mutex,
            None,
        )
        .await
        .expect("closure refresh");
        assert_eq!(updated, 1);

        assert!(
            client
                .closed_condition_ids
                .lock()
                .expect("closed_condition_ids mutex poisoned")
                .contains(TEST_CONDITION_ID)
        );
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    #[tokio::test]
    async fn closure_refresh_closed_wins_stale_auto_load_open(#[case] open_completes_last: bool) {
        let open_reply = if open_completes_last {
            ScriptedAutoLoadReply::delayed(
                serde_json::json!([gamma_market_expired_fixture_value()]),
                Duration::from_millis(200),
            )
        } else {
            ScriptedAutoLoadReply::ok(serde_json::json!([gamma_market_expired_fixture_value()]))
        };
        let mut closed_market = gamma_market_expired_fixture_value();
        closed_market["closed"] = Value::Bool(true);
        let state = ScriptedAutoLoadServerState::new(
            vec![
                open_reply,
                ScriptedAutoLoadReply::ok(serde_json::json!([closed_market])),
            ],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let mut carried = instrument_from_gamma_fixture(gamma_market_expired_fixture_value());
        if let InstrumentAny::BinaryOption(binary) = &mut carried {
            binary.expiration_ns = UnixNanos::from(1);
            crate::filters::set_market_closed(binary, false);
        }
        let instrument_id = carried.id();
        client.instruments.insert(instrument_id, carried);
        client.active_quote_subs.insert(instrument_id);
        client.queue_pending_load(instrument_id);

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    !state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .is_empty()
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        if !open_completes_last {
            wait_until_async(
                || {
                    let client = &client;
                    async move {
                        client
                            .token_meta
                            .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
                    }
                },
                StdDuration::from_secs(3),
            )
            .await;

            while data_rx.try_recv().is_ok() {}
        }

        crate::data::instruments::refresh_expired_market_closure(
            client.provider.http_client(),
            &client.instruments,
            &client.data_sender,
            UnixNanos::from(u64::MAX),
            &client.closed_condition_ids,
            &client.ws_sub_mutex,
            None,
        )
        .await
        .expect("closure refresh");
        crate::data::runtime::retire_closed_condition_state(
            TEST_CONDITION_ID,
            [instrument_id],
            &client.closed_condition_ids,
            &client.instruments,
            &client.token_meta,
            &client.order_books,
            &client.last_quotes,
            &client.active_quote_subs,
            &client.active_delta_subs,
            &client.active_trade_subs,
            &client.resolve_poll_watchlist,
            &client.pending_snapshot_after_tick_change,
            &client.pending_auto_loads,
            &client.ws_open_tokens,
            &client.ws_sub_mutex,
            &client.ws_client.handle(),
            None,
        )
        .await;
        wait_until_async(
            || {
                let state = state.clone();
                async move { state.completed_replies.load(Ordering::SeqCst) == 2 }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert!(
            client
                .closed_condition_ids
                .lock()
                .expect("closed_condition_ids mutex poisoned")
                .contains(TEST_CONDITION_ID)
        );
        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(
            !client
                .ws_open_tokens
                .contains(&Ustr::from(TEST_TOKEN_ID_YES))
        );

        while let Ok(event) = data_rx.try_recv() {
            if let DataEvent::Instrument(instrument) = event {
                assert_ne!(crate::filters::market_closed(&instrument), Some(false));
            }
        }
    }

    #[rstest]
    #[tokio::test]
    async fn auto_load_expired_open_instrument_is_cached_and_subscribed() {
        let filter_calls = Arc::new(AtomicUsize::new(0));
        let state = ExpiredAutoLoadServerState {
            queries: Arc::new(StdMutex::new(Vec::new())),
            open_response: serde_json::json!([gamma_market_expired_fixture_value()]),
            closed_response: serde_json::json!([]),
            market_payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        };
        let addr = start_expired_auto_load_test_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        let filter_calls_clone = filter_calls.clone();
        client.add_instrument_filter(Arc::new(crate::filters::PredicateFilter::new(
            "count-calls",
            move |_| {
                filter_calls_clone.fetch_add(1, Ordering::SeqCst);
                true
            },
        )));
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 3;
        client.config.auto_load_retry_delay_initial_secs = 0.0;
        client.config.auto_load_retry_delay_max_secs = 0.0;

        let instrument_id = fixture_yes_instrument_id();
        let auto_load_scheduled = client.auto_load_scheduled.clone();
        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("subscribe_quotes should queue auto-load");

        let emitted_instrument = tokio::time::timeout(StdDuration::from_secs(3), async {
            loop {
                match data_rx.recv().await {
                    Some(DataEvent::Instrument(instrument)) if instrument.id() == instrument_id => {
                        return instrument;
                    }
                    Some(_) => {}
                    None => panic!("data event channel closed before instrument publication"),
                }
            }
        })
        .await
        .expect("timed out waiting for instrument publication");

        wait_until_async(
            || {
                let state = state.clone();
                let auto_load_scheduled = auto_load_scheduled.clone();
                async move {
                    !state.market_payloads.lock().await.is_empty()
                        && !auto_load_scheduled.load(Ordering::Acquire)
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert_eq!(
            *state
                .queries
                .lock()
                .expect("expired auto-load queries mutex poisoned"),
            vec![ExpiredAutoLoadQuery {
                condition_ids: Some(TEST_CONDITION_ID.to_string()),
                closed: None,
            }],
        );
        assert!(client.active_quote_subs.contains(&instrument_id));
        assert!(client.instruments.load().contains_key(&instrument_id));
        assert!(
            client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert_eq!(emitted_instrument.raw_symbol().as_str(), TEST_TOKEN_ID_YES);
        assert_eq!(filter_calls.load(Ordering::SeqCst), 2);
        assert!(
            !client
                .pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned")
                .contains(&instrument_id)
        );

        let payloads = state.market_payloads.lock().await.clone();
        assert_eq!(
            payloads,
            vec![serde_json::json!({
                "assets_ids": [TEST_TOKEN_ID_YES],
                "type": "market",
                "initial_dump": true,
            })],
        );

        client
            .ws_client
            .disconnect()
            .await
            .expect("disconnect failed");
    }

    #[rstest]
    #[case::past_end(true, true)]
    #[case::future_end(false, true)]
    #[case::not_accepting_orders(false, false)]
    #[tokio::test]
    async fn auto_load_open_outcome_is_independent_of_end_date_and_accepting_orders(
        #[case] past_end: bool,
        #[case] accepting_orders: bool,
    ) {
        let mut market = if past_end {
            gamma_market_expired_fixture_value()
        } else {
            gamma_market_recheck_fixture_value()
        };
        market["acceptingOrders"] = Value::Bool(accepting_orders);
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([market]))],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let instrument_id = fixture_yes_instrument_id();
        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("missing open instrument subscription should queue auto-load");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.market_payloads.lock().await.is_empty() }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert_eq!(
            state
                .queries
                .lock()
                .expect("scripted auto-load queries mutex poisoned")
                .as_slice(),
            &[ExpiredAutoLoadQuery {
                condition_ids: Some(TEST_CONDITION_ID.to_string()),
                closed: None,
            }],
        );
        assert!(client.instruments.load().contains_key(&instrument_id));
        assert!(
            client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(client.active_quote_subs.contains(&instrument_id));
        assert_eq!(state.market_payloads.lock().await.len(), 1);

        let mut published_requested = false;
        while let Ok(DataEvent::Instrument(instrument)) = data_rx.try_recv() {
            published_requested |= instrument.id() == instrument_id;
        }
        assert!(published_requested);
    }

    #[rstest]
    #[tokio::test]
    async fn auto_load_open_condition_retries_when_requested_token_is_missing() {
        const MISSING_TOKEN: &str =
            "99999999999999999999999999999999999999999999999999999999999999999";
        let market = gamma_market_recheck_fixture_value();
        let state = ScriptedAutoLoadServerState::new(
            vec![
                ScriptedAutoLoadReply::ok(serde_json::json!([market.clone()])),
                ScriptedAutoLoadReply::ok(serde_json::json!([market.clone()])),
                ScriptedAutoLoadReply::ok(serde_json::json!([market])),
            ],
            vec![],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 2;
        client.config.auto_load_retry_delay_initial_secs = 0.0;
        client.config.auto_load_retry_delay_max_secs = 0.0;

        let instrument_id = fixture_instrument_id(TEST_CONDITION_ID, MISSING_TOKEN);
        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("missing token subscription should queue auto-load");

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        >= 3
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert_eq!(
            state
                .queries
                .lock()
                .expect("scripted auto-load queries mutex poisoned")
                .as_slice(),
            &[
                ExpiredAutoLoadQuery {
                    condition_ids: Some(TEST_CONDITION_ID.to_string()),
                    closed: None,
                },
                ExpiredAutoLoadQuery {
                    condition_ids: Some(TEST_CONDITION_ID.to_string()),
                    closed: None,
                },
                ExpiredAutoLoadQuery {
                    condition_ids: Some(TEST_CONDITION_ID.to_string()),
                    closed: None,
                },
            ],
        );
        assert!(client.active_quote_subs.contains(&instrument_id));
        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(!client.ws_open_tokens.contains(&Ustr::from(MISSING_TOKEN)));
    }

    #[rstest]
    #[case::absent(false)]
    #[case::unusable_tokens(true)]
    #[tokio::test]
    async fn auto_load_unclassifiable_condition_remains_unknown_through_retry_budget(
        #[case] unusable_tokens: bool,
    ) {
        let normal_response = if unusable_tokens {
            let mut market = gamma_market_recheck_fixture_value();
            market["clobTokenIds"] = Value::String("[]".to_string());
            serde_json::json!([market])
        } else {
            serde_json::json!([])
        };
        let state = ScriptedAutoLoadServerState::new(
            vec![
                ScriptedAutoLoadReply::ok(normal_response.clone()),
                ScriptedAutoLoadReply::ok(normal_response.clone()),
                ScriptedAutoLoadReply::ok(normal_response),
            ],
            vec![
                ScriptedAutoLoadReply::ok(serde_json::json!([])),
                ScriptedAutoLoadReply::ok(serde_json::json!([])),
                ScriptedAutoLoadReply::ok(serde_json::json!([])),
            ],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 2;
        client.config.auto_load_retry_delay_initial_secs = 0.0;
        client.config.auto_load_retry_delay_max_secs = 0.0;
        let instrument_id = fixture_yes_instrument_id();

        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("subscribe_quotes should queue auto-load");

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        >= 6
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        let queries = state
            .queries
            .lock()
            .expect("scripted auto-load queries mutex poisoned")
            .clone();
        assert_eq!(queries.len(), 6);
        for (index, query) in queries.iter().enumerate() {
            assert_eq!(query.condition_ids.as_deref(), Some(TEST_CONDITION_ID));
            assert_eq!(query.closed.as_deref(), (index % 2 == 1).then_some("true"));
        }
        assert!(client.active_quote_subs.contains(&instrument_id));
        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(state.market_payloads.lock().await.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn auto_load_applies_open_closed_and_unknown_per_condition() {
        const OPEN_CONDITION: &str =
            "0x1111111111111111111111111111111111111111111111111111111111111111";
        const CLOSED_CONDITION: &str =
            "0x2222222222222222222222222222222222222222222222222222222222222222";
        const UNKNOWN_CONDITION: &str =
            "0x3333333333333333333333333333333333333333333333333333333333333333";
        const OPEN_TOKEN: &str =
            "11111111111111111111111111111111111111111111111111111111111111111";
        const CLOSED_TOKEN: &str =
            "22222222222222222222222222222222222222222222222222222222222222222";
        const UNKNOWN_TOKEN: &str =
            "33333333333333333333333333333333333333333333333333333333333333333";

        let open_market = gamma_market_fixture_for(
            OPEN_CONDITION,
            OPEN_TOKEN,
            "11111111111111111111111111111111111111111111111111111111111111112",
            false,
        );
        let closed_market = gamma_market_fixture_for(
            CLOSED_CONDITION,
            CLOSED_TOKEN,
            "22222222222222222222222222222222222222222222222222222222222222223",
            true,
        );
        let state = ScriptedAutoLoadServerState::new(
            vec![
                ScriptedAutoLoadReply::ok(serde_json::json!([open_market])),
                ScriptedAutoLoadReply::ok(serde_json::json!([])),
            ],
            vec![
                ScriptedAutoLoadReply::ok(serde_json::json!([closed_market])),
                ScriptedAutoLoadReply::ok(serde_json::json!([])),
            ],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 1;
        client.config.auto_load_retry_delay_initial_secs = 0.0;
        client.config.auto_load_retry_delay_max_secs = 0.0;

        let open_id = fixture_instrument_id(OPEN_CONDITION, OPEN_TOKEN);
        let closed_id = fixture_instrument_id(CLOSED_CONDITION, CLOSED_TOKEN);
        let unknown_id = fixture_instrument_id(UNKNOWN_CONDITION, UNKNOWN_TOKEN);
        for instrument_id in [open_id, closed_id, unknown_id] {
            client
                .subscribe_quotes(SubscribeQuotes::new(
                    instrument_id,
                    Some(client.client_id),
                    Some(*POLYMARKET_VENUE),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                ))
                .expect("subscribe_quotes should queue auto-load");
        }

        wait_until_async(
            || {
                let client = &client;
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        == 4
                        && client.instruments.load().contains_key(&open_id)
                        && client
                            .closed_condition_ids
                            .lock()
                            .expect("closed_condition_ids mutex poisoned")
                            .contains(CLOSED_CONDITION)
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        let queries = state
            .queries
            .lock()
            .expect("scripted auto-load queries mutex poisoned")
            .clone();
        assert_eq!(queries.len(), 4);
        assert_eq!(queries[2].condition_ids.as_deref(), Some(UNKNOWN_CONDITION));
        assert_eq!(queries[2].closed, None);
        assert_eq!(queries[3].condition_ids.as_deref(), Some(UNKNOWN_CONDITION));
        assert_eq!(queries[3].closed.as_deref(), Some("true"));

        assert!(client.active_quote_subs.contains(&open_id));
        assert!(client.instruments.load().contains_key(&open_id));
        assert!(client.token_meta.contains_key(&Ustr::from(OPEN_TOKEN)));

        assert!(!client.active_quote_subs.contains(&closed_id));
        assert!(!client.instruments.load().contains_key(&closed_id));
        assert!(!client.token_meta.contains_key(&Ustr::from(CLOSED_TOKEN)));

        assert!(client.active_quote_subs.contains(&unknown_id));
        assert!(!client.instruments.load().contains_key(&unknown_id));
        assert!(!client.token_meta.contains_key(&Ustr::from(UNKNOWN_TOKEN)));

        client
            .ws_client
            .disconnect()
            .await
            .expect("disconnect failed");
    }

    #[rstest]
    #[tokio::test]
    async fn auto_load_closed_probe_failure_preserves_open_condition() {
        const OPEN_CONDITION: &str =
            "0x4444444444444444444444444444444444444444444444444444444444444444";
        const UNKNOWN_CONDITION: &str =
            "0x5555555555555555555555555555555555555555555555555555555555555555";
        const OPEN_TOKEN: &str =
            "44444444444444444444444444444444444444444444444444444444444444444";
        const UNKNOWN_TOKEN: &str =
            "55555555555555555555555555555555555555555555555555555555555555555";

        let open_market = gamma_market_fixture_for(
            OPEN_CONDITION,
            OPEN_TOKEN,
            "44444444444444444444444444444444444444444444444444444444444444446",
            false,
        );
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([open_market]))],
            vec![ScriptedAutoLoadReply::failed()],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let open_id = fixture_instrument_id(OPEN_CONDITION, OPEN_TOKEN);
        let unknown_id = fixture_instrument_id(UNKNOWN_CONDITION, UNKNOWN_TOKEN);
        for instrument_id in [open_id, unknown_id] {
            client
                .subscribe_quotes(SubscribeQuotes::new(
                    instrument_id,
                    Some(client.client_id),
                    Some(*POLYMARKET_VENUE),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                ))
                .expect("subscribe_quotes should queue auto-load");
        }

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        >= 2
                }
            },
            StdDuration::from_secs(3),
        )
        .await;
        wait_until_async(
            || {
                let client = &client;
                async move { client.instruments.load().contains_key(&open_id) }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert!(client.active_quote_subs.contains(&open_id));
        assert!(client.instruments.load().contains_key(&open_id));
        assert!(client.token_meta.contains_key(&Ustr::from(OPEN_TOKEN)));
        assert!(client.active_quote_subs.contains(&unknown_id));
        assert!(!client.instruments.load().contains_key(&unknown_id));
        assert!(!client.token_meta.contains_key(&Ustr::from(UNKNOWN_TOKEN)));

        client
            .ws_client
            .disconnect()
            .await
            .expect("disconnect failed");
    }

    #[rstest]
    #[case::stale_open_completes_last(true)]
    #[case::closed_completes_last(false)]
    #[tokio::test]
    async fn auto_load_closed_wins_concurrent_completion_order(#[case] open_completes_last: bool) {
        const CONDITION: &str =
            "0x6666666666666666666666666666666666666666666666666666666666666666";
        const TOKEN: &str = "66666666666666666666666666666666666666666666666666666666666666666";
        let open_market = gamma_market_fixture_for(
            CONDITION,
            TOKEN,
            "66666666666666666666666666666666666666666666666666666666666666667",
            false,
        );
        let closed_market = gamma_market_fixture_for(
            CONDITION,
            TOKEN,
            "66666666666666666666666666666666666666666666666666666666666666667",
            true,
        );
        let delay = Duration::from_millis(250);
        let (open_replies, closed_replies, queries_before_second_load) = if open_completes_last {
            (
                vec![
                    ScriptedAutoLoadReply::delayed(serde_json::json!([open_market]), delay),
                    ScriptedAutoLoadReply::ok(serde_json::json!([])),
                ],
                vec![ScriptedAutoLoadReply::ok(serde_json::json!([
                    closed_market
                ]))],
                1,
            )
        } else {
            (
                vec![
                    ScriptedAutoLoadReply::ok(serde_json::json!([])),
                    ScriptedAutoLoadReply::ok(serde_json::json!([open_market])),
                ],
                vec![ScriptedAutoLoadReply::delayed(
                    serde_json::json!([closed_market]),
                    delay,
                )],
                2,
            )
        };
        let state = ScriptedAutoLoadServerState::new(open_replies, closed_replies);
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;
        let instrument_id = fixture_instrument_id(CONDITION, TOKEN);

        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("subscribe_quotes should queue auto-load");

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state
                        .queries
                        .lock()
                        .expect("scripted auto-load queries mutex poisoned")
                        .len()
                        >= queries_before_second_load
                }
            },
            StdDuration::from_secs(3),
        )
        .await;
        client.queue_pending_load(instrument_id);

        wait_until_async(
            || {
                let client = &client;
                async move {
                    client
                        .closed_condition_ids
                        .lock()
                        .expect("closed_condition_ids mutex poisoned")
                        .contains(CONDITION)
                        && !client.active_quote_subs.contains(&instrument_id)
                        && !client.instruments.load().contains_key(&instrument_id)
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(!client.token_meta.contains_key(&Ustr::from(TOKEN)));
        assert!(!client.ws_open_tokens.contains(&Ustr::from(TOKEN)));
        let payloads = state.market_payloads.lock().await.clone();
        if open_completes_last {
            assert!(payloads.is_empty());
        }

        let published = std::iter::from_fn(|| data_rx.try_recv().ok())
            .filter_map(|event| match event {
                DataEvent::Instrument(instrument) if instrument.id() == instrument_id => {
                    Some(crate::filters::market_closed(&instrument))
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        if open_completes_last {
            assert!(published.is_empty());
        } else {
            assert_eq!(published, vec![Some(false), Some(true)]);
        }
    }

    #[rstest]
    #[tokio::test]
    async fn remembered_closed_condition_rejects_later_subscription() {
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([]))],
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([
                gamma_market_future_closed_fixture_value()
            ]))],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;
        let instrument_id = fixture_yes_instrument_id();

        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("initial subscription should queue auto-load");

        wait_until_async(
            || {
                let client = &client;
                async move {
                    !client.active_quote_subs.contains(&instrument_id)
                        && !client.instruments.load().contains_key(&instrument_id)
                }
            },
            StdDuration::from_secs(3),
        )
        .await;
        let query_count = state
            .queries
            .lock()
            .expect("scripted auto-load queries mutex poisoned")
            .len();

        let _ = client.subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(client.client_id),
            Some(*POLYMARKET_VENUE),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ));
        // Quiet period: a terminal resubscription must not enqueue a delayed auto-load.
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(
            state
                .queries
                .lock()
                .expect("scripted auto-load queries mutex poisoned")
                .len(),
            query_count,
        );
        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(state.market_payloads.lock().await.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn closed_watchlisted_metadata_stays_retired_on_resubscribe() {
        let closed_market = gamma_market_future_closed_fixture_value();
        let state = ScriptedAutoLoadServerState::new(
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([]))],
            vec![ScriptedAutoLoadReply::ok(serde_json::json!([
                closed_market.clone()
            ]))],
        );
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let instruments = instruments_from_gamma_fixture(closed_market);
        let instrument = instruments
            .iter()
            .find(|instrument| instrument.id() == fixture_yes_instrument_id())
            .expect("Yes instrument");
        let sibling = instruments
            .iter()
            .find(|instrument| instrument.id() == fixture_no_instrument_id())
            .expect("No instrument");
        let instrument_id = instrument.id();
        let sibling_id = sibling.id();

        for (instrument, position_id) in [
            (instrument, "P-CLOSED-WATCH-YES"),
            (sibling, "P-CLOSED-WATCH-NO"),
        ] {
            cache_instrument_unchecked(&client.instruments, &client.token_meta, instrument);
            upsert_resolve_watch_entry_from_instrument(
                &client.resolve_poll_watchlist,
                instrument,
                PositionId::new(position_id),
            );
        }
        client.active_quote_subs.insert(instrument_id);
        client.active_quote_subs.insert(sibling_id);
        client.active_delta_subs.insert(sibling_id);
        client.active_trade_subs.insert(sibling_id);
        client.queue_pending_load(instrument_id);

        wait_until_async(
            || {
                let client = &client;
                async move {
                    !client.active_quote_subs.contains(&instrument_id)
                        && !client.active_quote_subs.contains(&sibling_id)
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert!(client.instruments.load().contains_key(&instrument_id));
        assert!(client.instruments.load().contains_key(&sibling_id));
        assert!(
            client
                .resolve_poll_watchlist
                .contains_key(&TEST_CONDITION_ID.to_string())
        );
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_NO))
        );
        assert!(!client.active_delta_subs.contains(&sibling_id));
        assert!(!client.active_trade_subs.contains(&sibling_id));

        let query_count = state
            .queries
            .lock()
            .expect("scripted auto-load queries mutex poisoned")
            .len();

        for instrument_id in [instrument_id, sibling_id] {
            let _ = client.subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ));
        }
        // Quiet period: retained settlement metadata must not trigger a delayed live reload.
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(
            state
                .queries
                .lock()
                .expect("scripted auto-load queries mutex poisoned")
                .len(),
            query_count,
        );
        assert!(client.instruments.load().contains_key(&instrument_id));
        assert!(client.instruments.load().contains_key(&sibling_id));
        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(!client.active_quote_subs.contains(&sibling_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_NO))
        );
        assert!(state.market_payloads.lock().await.is_empty());
    }

    #[rstest]
    #[case::closed_probe_failure(false)]
    #[case::normal_query_failure(true)]
    #[tokio::test]
    async fn auto_load_successful_chunks_survive_failed_chunk(#[case] normal_query_failure: bool) {
        const FIRST_CONDITION: &str =
            "0x0000000000000000000000000000000000000000000000000000000000000001";
        const LAST_CONDITION: &str =
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        const FIRST_TOKEN: &str =
            "77777777777777777777777777777777777777777777777777777777777777777";
        const LAST_TOKEN: &str =
            "88888888888888888888888888888888888888888888888888888888888888888";
        let first_market = gamma_market_fixture_for(
            FIRST_CONDITION,
            FIRST_TOKEN,
            "77777777777777777777777777777777777777777777777777777777777777778",
            false,
        );
        let last_market = gamma_market_fixture_for(
            LAST_CONDITION,
            LAST_TOKEN,
            "88888888888888888888888888888888888888888888888888888888888888889",
            false,
        );
        let state = if normal_query_failure {
            ScriptedAutoLoadServerState::new(
                vec![
                    ScriptedAutoLoadReply::ok(serde_json::json!([first_market])),
                    ScriptedAutoLoadReply::failed(),
                ],
                vec![ScriptedAutoLoadReply::ok(serde_json::json!([]))],
            )
        } else {
            ScriptedAutoLoadServerState::new(
                vec![
                    ScriptedAutoLoadReply::ok(serde_json::json!([first_market])),
                    ScriptedAutoLoadReply::ok(serde_json::json!([last_market])),
                ],
                vec![
                    ScriptedAutoLoadReply::failed(),
                    ScriptedAutoLoadReply::ok(serde_json::json!([])),
                ],
            )
        };
        let addr = start_scripted_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
        client.config.auto_load_debounce_ms = 0;
        client.config.auto_load_max_retries = 0;

        let first_id = fixture_instrument_id(FIRST_CONDITION, FIRST_TOKEN);
        let last_id = fixture_instrument_id(LAST_CONDITION, LAST_TOKEN);
        let mut instrument_ids = vec![first_id, last_id];

        for index in 2..=100 {
            let condition_id = format!("0x{index:064x}");
            let token_id = (9_000_000_u64 + index).to_string();
            instrument_ids.push(fixture_instrument_id(&condition_id, &token_id));
        }

        for instrument_id in instrument_ids {
            client
                .subscribe_quotes(SubscribeQuotes::new(
                    instrument_id,
                    Some(client.client_id),
                    Some(*POLYMARKET_VENUE),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                ))
                .expect("subscribe_quotes should queue auto-load");
        }

        wait_until_async(
            || {
                let client = &client;
                async move {
                    client.instruments.load().contains_key(&first_id)
                        && client.active_quote_subs.contains(&first_id)
                        && client.active_quote_subs.contains(&last_id)
                        && (normal_query_failure
                            || client.instruments.load().contains_key(&last_id))
                }
            },
            StdDuration::from_secs(3),
        )
        .await;

        assert!(client.instruments.load().contains_key(&first_id));
        assert!(client.active_quote_subs.contains(&first_id));
        assert!(client.active_quote_subs.contains(&last_id));
        assert_eq!(
            client.instruments.load().contains_key(&last_id),
            !normal_query_failure
        );

        client
            .ws_client
            .disconnect()
            .await
            .expect("disconnect failed");
    }

    #[rstest]
    #[case::quotes(ExpiredPath::Quotes, "0xTOKEN_EXPIRED")]
    #[case::book(ExpiredPath::BookSnapshot, "0xTOKEN_EXPIRED_BOOK")]
    #[case::trades(ExpiredPath::Trades, "0xCOND-EXPIRED-TRADES")]
    fn cached_expired_instrument_live_paths_honor_market_closure(
        #[case] path: ExpiredPath,
        #[case] raw_symbol: &str,
        #[values(None, Some(true), Some(false))] market_closed: Option<bool>,
    ) {
        let mut client = make_local_test_client();
        let expired = seed_instrument_with_context(
            &make_client_ws_ctx(&client),
            raw_symbol,
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-EXPIRED"),
                expiration_ns: Some(UnixNanos::from(1)),
                market_closed,
                ..SeedInstrumentContext::default()
            },
        );

        let result = match path {
            ExpiredPath::Quotes => client.subscribe_quotes(SubscribeQuotes::new(
                expired.id(),
                Some(client.client_id),
                Some(*POLYMARKET_VENUE),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            ExpiredPath::BookSnapshot => client.request_book_snapshot(RequestBookSnapshot::new(
                expired.id(),
                Some(NonZeroUsize::new(10).expect("nonzero depth")),
                Some(client.client_id),
                UUID4::new(),
                UnixNanos::default(),
                None,
            )),
            ExpiredPath::Trades => client.request_trades(RequestTrades::new(
                expired.id(),
                None,
                None,
                Some(NonZeroUsize::new(10).expect("nonzero limit")),
                Some(client.client_id),
                UUID4::new(),
                UnixNanos::default(),
                None,
            )),
        };

        // Only a positive `closed=false` retains an expired market; unknown state retires it.
        let retained = market_closed == Some(false);

        assert_eq!(result.is_ok(), retained);

        if matches!(path, ExpiredPath::Quotes) {
            assert_eq!(client.active_quote_subs.contains(&expired.id()), retained);
        }
    }

    fn level(price: &str, size: &str) -> PolymarketBookLevel {
        PolymarketBookLevel {
            price: price.to_string(),
            size: size.to_string(),
        }
    }

    fn make_snapshot(market: &str, asset_id: &str, prices: &[(&str, &str)]) -> MarketWsMessage {
        let mid = prices.len() / 2;
        let bids = prices[..mid].iter().map(|(p, s)| level(p, s)).collect();
        let asks = prices[mid..].iter().map(|(p, s)| level(p, s)).collect();
        MarketWsMessage::Book(PolymarketBookSnapshot {
            market: Ustr::from(market),
            asset_id: Ustr::from(asset_id),
            bids,
            asks,
            timestamp: "1700000000000".to_string(),
            hash: None,
            min_order_size: None,
            tick_size: None,
            neg_risk: None,
            last_trade_price: None,
        })
    }

    fn make_tick_change(market: &str, asset_id: &str, old: &str, new: &str) -> MarketWsMessage {
        MarketWsMessage::TickSizeChange(PolymarketTickSizeChange {
            market: Ustr::from(market),
            asset_id: Ustr::from(asset_id),
            new_tick_size: new.to_string(),
            old_tick_size: old.to_string(),
            timestamp: "1700000001000".to_string(),
        })
    }

    fn make_price_change(market: &str, asset_id: &str, price: &str, size: &str) -> MarketWsMessage {
        MarketWsMessage::PriceChange(PolymarketQuotes {
            market: Ustr::from(market),
            price_changes: vec![PolymarketQuote {
                asset_id: Ustr::from(asset_id),
                price: price.to_string(),
                side: PolymarketOrderSide::Buy,
                size: size.to_string(),
                hash: String::new(),
                best_bid: None,
                best_ask: None,
            }],
            timestamp: "1700000002000".to_string(),
        })
    }

    fn make_best_bid_ask(
        market: &str,
        asset_id: &str,
        best_bid: &str,
        best_ask: &str,
    ) -> MarketWsMessage {
        MarketWsMessage::BestBidAsk(PolymarketBestBidAsk {
            market: Ustr::from(market),
            asset_id: Ustr::from(asset_id),
            best_bid: best_bid.to_string(),
            best_ask: best_ask.to_string(),
            spread: String::new(),
            timestamp: "1700000003000".to_string(),
        })
    }

    fn emitted_quotes(
        data_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) -> Vec<QuoteTick> {
        std::iter::from_fn(|| data_rx.try_recv().ok())
            .filter_map(|event| match event {
                DataEvent::Data(NautilusData::Quote(quote)) => Some(quote),
                _ => None,
            })
            .collect()
    }

    fn quote_context(
        asset_id: &str,
    ) -> (
        TestWsContext,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        InstrumentId,
    ) {
        let (ctx, data_rx) = make_ws_ctx();
        let instrument =
            seed_instrument(&ctx, asset_id, Price::from("0.01"), Quantity::from("0.01"));
        let instrument_id = instrument.id();
        ctx.active_quote_subs.insert(instrument_id);
        (ctx, data_rx, instrument_id)
    }

    #[rstest]
    fn best_bid_ask_emits_quote_sized_from_local_book() {
        let asset_id = "0xTOKEN_BBA1";
        let market = "0xMARKET";
        let (mut ctx, mut data_rx, instrument_id) = quote_context(asset_id);
        ctx.compute_effective_deltas = true;
        ctx.active_delta_subs.insert(instrument_id);

        handle_market_message(
            make_snapshot(
                market,
                asset_id,
                &[
                    ("0.49", "100"),
                    ("0.50", "200"),
                    ("0.53", "400"),
                    ("0.52", "300"),
                ],
            ),
            &ctx,
        );
        handle_market_message(make_price_change(market, asset_id, "0.51", "50"), &ctx);

        while data_rx.try_recv().is_ok() {}

        handle_market_message(make_best_bid_ask(market, asset_id, "0.51", "0.52"), &ctx);

        let quotes = emitted_quotes(&mut data_rx);
        assert_eq!(quotes.len(), 1, "expected one quote, found: {quotes:?}");
        let quote = quotes[0];
        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("0.51"));
        assert_eq!(quote.ask_price, Price::from("0.52"));
        assert_eq!(quote.bid_size, Quantity::from("50.00"));
        assert_eq!(quote.ask_size, Quantity::from("300.00"));
        assert_eq!(
            quote.ts_event,
            UnixNanos::from(1_700_000_003_000_000_000u64),
        );
        assert_eq!(
            ctx.last_quotes.get(&instrument_id).map(|stored| *stored),
            Some(quote),
        );
    }

    #[rstest]
    fn best_bid_ask_without_local_book_carries_matching_last_quote_size() {
        let asset_id = "0xTOKEN_BBA2";
        let market = "0xMARKET";
        let (ctx, mut data_rx, instrument_id) = quote_context(asset_id);
        ctx.last_quotes.insert(
            instrument_id,
            QuoteTick::new(
                instrument_id,
                Price::from("0.49"),
                Price::from("0.51"),
                Quantity::from("100.00"),
                Quantity::from("75.00"),
                UnixNanos::from(1_700_000_002_000_000_000u64),
                UnixNanos::default(),
            ),
        );

        handle_market_message(make_best_bid_ask(market, asset_id, "0.49", "0.52"), &ctx);

        let quotes = emitted_quotes(&mut data_rx);
        assert_eq!(quotes.len(), 1, "expected one quote, found: {quotes:?}");
        let quote = quotes[0];
        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("0.49"));
        assert_eq!(quote.ask_price, Price::from("0.52"));
        assert_eq!(quote.bid_size, Quantity::from("100.00"));
        assert_eq!(quote.ask_size, Quantity::from("0.00"));
        assert_eq!(
            quote.ts_event,
            UnixNanos::from(1_700_000_003_000_000_000u64),
        );
    }

    #[rstest]
    fn best_bid_ask_uses_last_quote_while_snapshot_pending() {
        let asset_id = "0xTOKEN_BBA3";
        let market = "0xMARKET";
        let (mut ctx, mut data_rx, instrument_id) = quote_context(asset_id);
        ctx.compute_effective_deltas = true;
        ctx.active_delta_subs.insert(instrument_id);
        handle_market_message(
            make_snapshot(
                market,
                asset_id,
                &[
                    ("0.49", "100"),
                    ("0.50", "200"),
                    ("0.53", "400"),
                    ("0.52", "300"),
                ],
            ),
            &ctx,
        );
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);
        ctx.last_quotes.insert(
            instrument_id,
            QuoteTick::new(
                instrument_id,
                Price::from("0.50"),
                Price::from("0.51"),
                Quantity::from("12.00"),
                Quantity::from("13.00"),
                UnixNanos::from(1_700_000_002_000_000_000u64),
                UnixNanos::default(),
            ),
        );

        while data_rx.try_recv().is_ok() {}

        handle_market_message(make_best_bid_ask(market, asset_id, "0.50", "0.52"), &ctx);

        let quotes = emitted_quotes(&mut data_rx);
        assert_eq!(quotes.len(), 1, "expected one quote, found: {quotes:?}");
        assert_eq!(quotes[0].bid_size, Quantity::from("12.00"));
        assert_eq!(quotes[0].ask_size, Quantity::from("0.00"));
    }

    #[rstest]
    fn best_bid_ask_older_than_local_book_is_ignored() {
        let asset_id = "0xTOKEN_BBA8";
        let market = "0xMARKET";
        let (mut ctx, mut data_rx, instrument_id) = quote_context(asset_id);
        ctx.compute_effective_deltas = true;
        ctx.active_delta_subs.insert(instrument_id);
        handle_market_message(
            make_snapshot(
                market,
                asset_id,
                &[
                    ("0.49", "100"),
                    ("0.50", "200"),
                    ("0.53", "400"),
                    ("0.52", "300"),
                ],
            ),
            &ctx,
        );
        ctx.order_books.get_mut(&instrument_id).unwrap().ts_last =
            UnixNanos::from(1_700_000_004_000_000_000u64);
        let last_quote = QuoteTick::new(
            instrument_id,
            Price::from("0.50"),
            Price::from("0.51"),
            Quantity::from("12.00"),
            Quantity::from("13.00"),
            UnixNanos::from(1_700_000_002_000_000_000u64),
            UnixNanos::default(),
        );
        ctx.last_quotes.insert(instrument_id, last_quote);

        while data_rx.try_recv().is_ok() {}

        handle_market_message(make_best_bid_ask(market, asset_id, "0.50", "0.52"), &ctx);

        assert!(emitted_quotes(&mut data_rx).is_empty());
        assert_eq!(
            ctx.last_quotes.get(&instrument_id).map(|stored| *stored),
            Some(last_quote),
        );
    }

    #[rstest]
    #[case::valid(None)]
    #[case::invalid_hash(Some("invalid"))]
    fn stale_snapshot_is_ignored_before_book_and_quote_paths(#[case] hash: Option<&str>) {
        let asset_id = "0xTOKEN_BBA_STALE_BOOK";
        let market = "0xMARKET";
        let (mut ctx, mut data_rx, instrument_id) = quote_context(asset_id);
        ctx.compute_effective_deltas = true;
        ctx.active_delta_subs.insert(instrument_id);

        handle_market_message(
            make_snapshot(
                market,
                asset_id,
                &[
                    ("0.49", "100"),
                    ("0.50", "200"),
                    ("0.53", "400"),
                    ("0.52", "300"),
                ],
            ),
            &ctx,
        );
        handle_market_message(make_price_change(market, asset_id, "0.51", "50"), &ctx);

        while data_rx.try_recv().is_ok() {}

        let mut stale = make_snapshot(
            market,
            asset_id,
            &[("0.48", "20"), ("0.51", "5"), ("0.52", "6"), ("0.54", "12")],
        );
        let MarketWsMessage::Book(snapshot) = &mut stale else {
            unreachable!("make_snapshot must return a book message");
        };
        snapshot.hash = hash.map(str::to_string);
        handle_market_message(stale, &ctx);
        assert!(
            data_rx.try_recv().is_err(),
            "stale snapshot must not emit book or quote data",
        );
        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id),
            "stale snapshot must not gate later book data",
        );

        handle_market_message(make_best_bid_ask(market, asset_id, "0.51", "0.52"), &ctx);

        let quotes = emitted_quotes(&mut data_rx);
        assert_eq!(quotes.len(), 1, "expected one quote, found: {quotes:?}");
        assert_eq!(quotes[0].bid_price, Price::from("0.51"));
        assert_eq!(quotes[0].ask_price, Price::from("0.52"));
        assert_eq!(quotes[0].bid_size, Quantity::from("50.00"));
        assert_eq!(quotes[0].ask_size, Quantity::from("300.00"));

        let book = ctx.order_books.get(&instrument_id).unwrap();
        assert_eq!(book.best_bid_price(), Some(Price::from("0.51")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("50.00")));
        assert_eq!(book.best_ask_price(), Some(Price::from("0.52")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("300.00")));
        assert_eq!(book.ts_last, UnixNanos::from(1_700_000_002_000_000_000u64),);
    }

    #[rstest]
    fn stale_price_change_is_ignored_before_book_and_quote_paths() {
        let asset_id = "0xTOKEN_BBA_STALE_CHANGE";
        let market = "0xMARKET";
        let (mut ctx, mut data_rx, instrument_id) = quote_context(asset_id);
        ctx.compute_effective_deltas = true;
        ctx.active_delta_subs.insert(instrument_id);

        handle_market_message(
            make_snapshot(
                market,
                asset_id,
                &[
                    ("0.49", "100"),
                    ("0.50", "200"),
                    ("0.53", "400"),
                    ("0.52", "300"),
                ],
            ),
            &ctx,
        );
        handle_market_message(
            MarketWsMessage::PriceChange(PolymarketQuotes {
                market: Ustr::from(market),
                price_changes: vec![PolymarketQuote {
                    asset_id: Ustr::from(asset_id),
                    price: "0.51".to_string(),
                    side: PolymarketOrderSide::Buy,
                    size: "50".to_string(),
                    hash: String::new(),
                    best_bid: Some("0.51".to_string()),
                    best_ask: Some("0.52".to_string()),
                }],
                timestamp: "1700000002000".to_string(),
            }),
            &ctx,
        );

        while data_rx.try_recv().is_ok() {}

        handle_market_message(
            MarketWsMessage::PriceChange(PolymarketQuotes {
                market: Ustr::from(market),
                price_changes: vec![PolymarketQuote {
                    asset_id: Ustr::from(asset_id),
                    price: "0.51".to_string(),
                    side: PolymarketOrderSide::Buy,
                    size: "5".to_string(),
                    hash: String::new(),
                    best_bid: Some("0.51".to_string()),
                    best_ask: Some("0.52".to_string()),
                }],
                timestamp: "1700000001000".to_string(),
            }),
            &ctx,
        );
        assert!(
            data_rx.try_recv().is_err(),
            "stale price change must not emit book or quote data",
        );

        handle_market_message(make_best_bid_ask(market, asset_id, "0.51", "0.52"), &ctx);
        assert!(
            emitted_quotes(&mut data_rx).is_empty(),
            "unchanged BBA must not expose stale book sizes",
        );

        let book = ctx.order_books.get(&instrument_id).unwrap();
        assert_eq!(book.best_bid_price(), Some(Price::from("0.51")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("50.00")));
        assert_eq!(book.best_ask_price(), Some(Price::from("0.52")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("300.00")));
        assert_eq!(book.ts_last, UnixNanos::from(1_700_000_002_000_000_000u64),);
    }

    #[rstest]
    fn best_bid_ask_older_than_last_quote_is_ignored() {
        let asset_id = "0xTOKEN_BBA4";
        let market = "0xMARKET";
        let (ctx, mut data_rx, instrument_id) = quote_context(asset_id);
        let latest = QuoteTick::new(
            instrument_id,
            Price::from("0.49"),
            Price::from("0.51"),
            Quantity::from("100.00"),
            Quantity::from("75.00"),
            UnixNanos::from(1_700_000_004_000_000_000u64),
            UnixNanos::default(),
        );
        ctx.last_quotes.insert(instrument_id, latest);

        handle_market_message(make_best_bid_ask(market, asset_id, "0.50", "0.52"), &ctx);

        assert!(emitted_quotes(&mut data_rx).is_empty());
        assert_eq!(
            ctx.last_quotes.get(&instrument_id).map(|stored| *stored),
            Some(latest),
        );
    }

    #[derive(Clone, Copy)]
    enum StaleQuoteSource {
        Snapshot,
        PriceChange,
    }

    #[rstest]
    #[case::snapshot(StaleQuoteSource::Snapshot)]
    #[case::price_change(StaleQuoteSource::PriceChange)]
    fn quote_source_older_than_best_bid_ask_is_ignored(#[case] source: StaleQuoteSource) {
        let asset_id = "0xTOKEN_BBA_STALE";
        let market = "0xMARKET";
        let (ctx, mut data_rx, instrument_id) = quote_context(asset_id);

        handle_market_message(make_best_bid_ask(market, asset_id, "0.50", "0.52"), &ctx);
        let latest = emitted_quotes(&mut data_rx)
            .into_iter()
            .next()
            .expect("best bid/ask should emit a quote");

        let stale = match source {
            StaleQuoteSource::Snapshot => make_snapshot(
                market,
                asset_id,
                &[
                    ("0.48", "20"),
                    ("0.49", "10"),
                    ("0.51", "8"),
                    ("0.53", "12"),
                ],
            ),
            StaleQuoteSource::PriceChange => MarketWsMessage::PriceChange(PolymarketQuotes {
                market: Ustr::from(market),
                price_changes: vec![PolymarketQuote {
                    asset_id: Ustr::from(asset_id),
                    price: "0.49".to_string(),
                    side: PolymarketOrderSide::Buy,
                    size: "10".to_string(),
                    hash: String::new(),
                    best_bid: Some("0.49".to_string()),
                    best_ask: Some("0.51".to_string()),
                }],
                timestamp: "1700000002000".to_string(),
            }),
        };
        handle_market_message(stale, &ctx);

        assert!(emitted_quotes(&mut data_rx).is_empty());
        assert_eq!(
            ctx.last_quotes.get(&instrument_id).map(|stored| *stored),
            Some(latest),
        );
    }

    #[rstest]
    fn best_bid_ask_unchanged_quote_is_not_re_emitted() {
        let asset_id = "0xTOKEN_BBA5";
        let market = "0xMARKET";
        let (ctx, mut data_rx, instrument_id) = quote_context(asset_id);
        ctx.last_quotes.insert(
            instrument_id,
            QuoteTick::new(
                instrument_id,
                Price::from("0.49"),
                Price::from("0.51"),
                Quantity::from("100.00"),
                Quantity::from("75.00"),
                UnixNanos::from(1_700_000_002_000_000_000u64),
                UnixNanos::default(),
            ),
        );

        handle_market_message(make_best_bid_ask(market, asset_id, "0.49", "0.51"), &ctx);

        assert!(emitted_quotes(&mut data_rx).is_empty());
    }

    #[rstest]
    fn best_bid_ask_missing_side_drops_by_default() {
        let asset_id = "0xTOKEN_BBA6";
        let market = "0xMARKET";
        let (ctx, mut data_rx, _) = quote_context(asset_id);

        handle_market_message(make_best_bid_ask(market, asset_id, "0.50", "1"), &ctx);

        assert!(emitted_quotes(&mut data_rx).is_empty());
    }

    #[rstest]
    fn best_bid_ask_without_quote_subscription_is_ignored() {
        let asset_id = "0xTOKEN_BBA7";
        let market = "0xMARKET";
        let (ctx, mut data_rx) = make_ws_ctx();
        seed_instrument(&ctx, asset_id, Price::from("0.01"), Quantity::from("0.01"));

        handle_market_message(make_best_bid_ask(market, asset_id, "0.50", "0.52"), &ctx);

        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    fn best_bid_ask_for_terminal_condition_is_ignored() {
        let asset_id = "0xCONDITION-token";
        let market = "0xCONDITION";
        let (ctx, mut data_rx, instrument_id) = quote_context(asset_id);
        ctx.closed_condition_ids
            .lock()
            .unwrap()
            .insert(market.to_string());

        handle_market_message(make_best_bid_ask(market, asset_id, "0.50", "0.52"), &ctx);

        assert!(emitted_quotes(&mut data_rx).is_empty());
        assert!(!ctx.last_quotes.contains_key(&instrument_id));
    }

    #[rstest]
    fn tick_size_change_clears_book_and_marks_pending() {
        let asset_id_str = "0xTOKEN";
        let token_ustr = Ustr::from(asset_id_str);
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);

        let prior_quote = QuoteTick::new(
            instrument_id,
            Price::from("0.504"),
            Price::from("0.506"),
            Quantity::from("5.00"),
            Quantity::from("8.00"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        ctx.last_quotes.insert(instrument_id, prior_quote);

        let snap = make_snapshot(
            market,
            asset_id_str,
            &[
                ("0.501", "10"),
                ("0.504", "5"),
                ("0.506", "8"),
                ("0.509", "12"),
            ],
        );
        handle_market_message(snap, &ctx);
        assert!(ctx.order_books.contains_key(&instrument_id));

        while data_rx.try_recv().is_ok() {}

        let change = make_tick_change(market, asset_id_str, "0.001", "0.01");
        handle_market_message(change, &ctx);

        assert!(!ctx.order_books.contains_key(&instrument_id));
        assert!(ctx.last_quotes.contains_key(&instrument_id));
        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );

        let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
        assert_eq!(meta.price_precision, 2);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
            "expected rebuilt instrument event, found: {events:?}",
        );
        assert!(
            !events.iter().any(|e| matches!(e, DataEvent::Data(_))),
            "tick size change must not emit Data events: {events:?}",
        );
    }

    #[rstest]
    fn pending_drops_price_change_until_snapshot() {
        let asset_id_str = "0xTOKEN2";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);

        let pc = make_price_change(market, asset_id_str, "0.50", "20");
        handle_market_message(pc, &ctx);

        assert!(!ctx.order_books.contains_key(&instrument_id));
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.is_empty(),
            "price_change while pending must not emit any events: {events:?}",
        );

        let snap = make_snapshot(
            market,
            asset_id_str,
            &[("0.45", "5"), ("0.49", "10"), ("0.51", "8"), ("0.55", "12")],
        );
        handle_market_message(snap, &ctx);

        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(!ctx.order_books.contains_key(&instrument_id));
    }

    #[rstest]
    fn tick_size_change_noop_preserves_book_and_quote() {
        let asset_id_str = "0xTOKEN_NOOP";
        let token_ustr = Ustr::from(asset_id_str);
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );

        let snap = make_snapshot(
            market,
            asset_id_str,
            &[("0.50", "10"), ("0.54", "5"), ("0.56", "8"), ("0.59", "12")],
        );
        handle_market_message(snap, &ctx);
        let book_ts_before = ctx
            .order_books
            .get(&instrument_id)
            .expect("book entry")
            .ts_last;

        while data_rx.try_recv().is_ok() {}

        let change = make_tick_change(market, asset_id_str, "0.01", "0.01");
        handle_market_message(change, &ctx);

        let book_after = ctx.order_books.get(&instrument_id).expect("book entry");
        assert_eq!(book_after.ts_last, book_ts_before);
        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
        assert_eq!(meta.price_precision, 2);
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.is_empty(),
            "no-op tick change must not emit events: {events:?}",
        );
    }

    #[rstest]
    #[case::same_precision("0.005", "0.001", 3, "0.999")]
    #[case::between_non_power_ticks("0.005", "0.0025", 4, "0.9975")]
    fn tick_size_change_rebuilds_exact_increment(
        #[case] old_tick: &str,
        #[case] new_tick: &str,
        #[case] expected_precision: u8,
        #[case] expected_max: &str,
    ) {
        let asset_id_str = "0xTOKEN_VALUE";
        let token_ustr = Ustr::from(asset_id_str);
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from(old_tick),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );

        let change = make_tick_change(market, asset_id_str, old_tick, new_tick);
        handle_market_message(change, &ctx);

        assert!(!ctx.order_books.contains_key(&instrument_id));
        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
        assert_eq!(meta.price_precision, expected_precision);

        let rebuilt = ctx
            .instruments
            .load()
            .get(&instrument_id)
            .cloned()
            .expect("rebuilt instrument");
        assert_eq!(rebuilt.price_increment(), Price::from(new_tick));
        assert_eq!(rebuilt.min_price(), Some(Price::from(new_tick)));
        assert_eq!(rebuilt.max_price(), Some(Price::from(expected_max)));

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
            "expected rebuilt instrument event, found: {events:?}",
        );
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn tick_size_change_preserves_market_closure_state(#[case] closed: bool) {
        let asset_id_str = "0xTOKEN_CLOSURE";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument_with_context(
            &ctx,
            asset_id_str,
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_closed: Some(closed),
                ..SeedInstrumentContext::default()
            },
        );
        let instrument_id = inst.id();

        handle_market_message(
            make_tick_change(market, asset_id_str, "0.001", "0.01"),
            &ctx,
        );

        let rebuilt = ctx
            .instruments
            .load()
            .get(&instrument_id)
            .cloned()
            .expect("rebuilt instrument");

        assert_eq!(rebuilt.price_increment(), Price::from("0.01"));
        assert_eq!(crate::filters::market_closed(&rebuilt), Some(closed));

        let event = data_rx.try_recv().expect("tick size instrument event");
        let DataEvent::Instrument(published) = event else {
            panic!("Expected instrument event, was {event:?}");
        };
        assert_eq!(published.id(), instrument_id);
        assert_eq!(published.price_increment(), Price::from("0.01"));
        assert_eq!(crate::filters::market_closed(&published), Some(closed));
        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    fn tick_size_change_does_not_mark_pending_for_trade_only_sub() {
        let asset_id_str = "0xTOKEN6";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_trade_subs.insert(instrument_id);

        let change = make_tick_change(market, asset_id_str, "0.001", "0.01");
        handle_market_message(change, &ctx);

        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
            "instrument update must still be emitted: {events:?}",
        );
    }

    #[rstest]
    fn pending_persists_when_snapshot_has_corrupt_level() {
        let asset_id_str = "0xTOKEN7";

        let (ctx, _data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.active_quote_subs.insert(instrument_id);
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);

        let snap = MarketWsMessage::Book(PolymarketBookSnapshot {
            market: Ustr::from("0xMARKET"),
            asset_id: Ustr::from(asset_id_str),
            bids: vec![level("not-a-number", "1"), level("0.49", "10")],
            asks: vec![level("0.51", "8"), level("0.55", "12")],
            timestamp: "1700000000000".to_string(),
            hash: None,
            min_order_size: None,
            tick_size: None,
            neg_risk: None,
            last_trade_price: None,
        });
        handle_market_message(snap, &ctx);

        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(!ctx.order_books.contains_key(&instrument_id));
    }

    #[rstest]
    #[case::initial_snapshot(false)]
    #[case::tick_change_recovery(true)]
    fn snapshot_hash_mismatch_gates_until_valid_snapshot(#[case] already_pending: bool) {
        let valid: PolymarketBookSnapshot = serde_json::from_str(include_str!(
            "../../test_data/ws_book_snapshot_captured.json"
        ))
        .expect("captured snapshot should deserialize");
        let asset_id = valid.asset_id.as_str();
        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument_with_context(
            &ctx,
            asset_id,
            Price::from("0.01"),
            Quantity::from("0.000001"),
            SeedInstrumentContext {
                min_order_size: Some("5"),
                neg_risk: Some(false),
                ..SeedInstrumentContext::default()
            },
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.active_quote_subs.insert(instrument_id);
        if already_pending {
            ctx.pending_snapshot_after_tick_change.insert(instrument_id);
        }

        let mut divergent = valid.clone();
        divergent.bids[0].size = "3149725.71".to_string();
        handle_market_message(MarketWsMessage::Book(divergent), &ctx);

        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(!ctx.order_books.contains_key(&instrument_id));
        assert!(!ctx.last_quotes.contains_key(&instrument_id));
        assert!(data_rx.try_recv().is_err());

        handle_market_message(MarketWsMessage::Book(valid), &ctx);

        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(!ctx.order_books.contains_key(&instrument_id));
        assert!(ctx.last_quotes.contains_key(&instrument_id));
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            DataEvent::Data(NautilusData::Deltas(_))
        ));
        assert!(matches!(events[1], DataEvent::Data(NautilusData::Quote(_))));
    }

    #[rstest]
    fn incomplete_snapshot_hash_preimage_resumes_deltas() {
        let mut snapshot: PolymarketBookSnapshot = serde_json::from_str(include_str!(
            "../../test_data/ws_book_snapshot_captured.json"
        ))
        .expect("captured snapshot should deserialize");
        snapshot.tick_size = None;
        snapshot.last_trade_price = None;

        let asset_id = snapshot.asset_id.as_str();
        let (ctx, mut data_rx) = make_ws_ctx();
        let instrument = seed_instrument_with_context(
            &ctx,
            asset_id,
            Price::from("0.01"),
            Quantity::from("0.000001"),
            SeedInstrumentContext {
                min_order_size: Some("5"),
                neg_risk: Some(false),
                ..SeedInstrumentContext::default()
            },
        );
        let instrument_id = instrument.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.active_quote_subs.insert(instrument_id);
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);

        handle_market_message(MarketWsMessage::Book(snapshot), &ctx);

        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(!ctx.order_books.contains_key(&instrument_id));
        assert!(ctx.last_quotes.contains_key(&instrument_id));
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            DataEvent::Data(NautilusData::Deltas(_))
        ));
        assert!(matches!(events[1], DataEvent::Data(NautilusData::Quote(_))));
    }

    #[rstest]
    fn price_change_emits_delta_without_updating_local_book_state_when_disabled() {
        let asset_id_str = "0xTOKEN10";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );

        let pc = make_price_change(market, asset_id_str, "0.50", "20");
        handle_market_message(pc, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DataEvent::Data(NautilusData::Deltas(_)))),
            "delta must be emitted on the not-pending happy path: {events:?}",
        );

        let book = ctx.order_books.get(&instrument_id).expect("book entry");
        assert_eq!(book.best_bid_price(), None);
        assert_eq!(book.best_bid_size(), None);
        assert_eq!(book.update_count, 0);
    }

    #[rstest]
    fn price_change_batches_interleaved_changes_by_instrument() {
        let asset_a = "0xTOKEN-A";
        let asset_b = "0xTOKEN-B";
        let asset_unknown = "0xTOKEN-UNKNOWN";
        let market = Ustr::from("0xMARKET");
        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let instrument_a =
            seed_instrument(&ctx, asset_a, Price::from("0.001"), Quantity::from("0.01")).id();
        let instrument_b =
            seed_instrument(&ctx, asset_b, Price::from("0.001"), Quantity::from("0.01")).id();
        ctx.active_delta_subs.insert(instrument_a);
        ctx.active_delta_subs.insert(instrument_b);
        ctx.active_quote_subs.insert(instrument_a);
        ctx.active_quote_subs.insert(instrument_b);

        handle_market_message(
            make_snapshot(
                market.as_str(),
                asset_a,
                &[("0.003", "10"), ("0.005", "10")],
            ),
            &ctx,
        );
        handle_market_message(
            make_snapshot(
                market.as_str(),
                asset_b,
                &[("0.993", "10"), ("0.995", "10")],
            ),
            &ctx,
        );

        while data_rx.try_recv().is_ok() {}

        let price_changes = vec![
            (asset_unknown, "0.111", PolymarketOrderSide::Buy, "1"),
            (asset_a, "0.007", PolymarketOrderSide::Buy, "20"),
            (asset_b, "0.997", PolymarketOrderSide::Buy, "20"),
            (asset_unknown, "0.222", PolymarketOrderSide::Sell, "2"),
            (asset_a, "0.005", PolymarketOrderSide::Sell, "0"),
            (asset_b, "0.995", PolymarketOrderSide::Sell, "0"),
            (asset_a, "0.009", PolymarketOrderSide::Sell, "30"),
            (asset_b, "0.999", PolymarketOrderSide::Sell, "30"),
        ]
        .into_iter()
        .map(|(asset_id, price, side, size)| PolymarketQuote {
            asset_id: Ustr::from(asset_id),
            price: price.to_string(),
            side,
            size: size.to_string(),
            hash: String::new(),
            best_bid: Some(
                if asset_id == asset_a {
                    "0.007"
                } else {
                    "0.997"
                }
                .to_string(),
            ),
            best_ask: Some(
                if asset_id == asset_a {
                    "0.009"
                } else {
                    "0.999"
                }
                .to_string(),
            ),
        })
        .collect();
        handle_market_message(
            MarketWsMessage::PriceChange(PolymarketQuotes {
                market,
                price_changes,
                timestamp: "1700000003000".to_string(),
            }),
            &ctx,
        );

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        let batches: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                DataEvent::Data(NautilusData::Deltas(deltas)) => Some(deltas),
                _ => None,
            })
            .collect();
        let quote_instruments: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                DataEvent::Data(NautilusData::Quote(quote)) => Some(quote.instrument_id),
                _ => None,
            })
            .collect();
        let event_sequence: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                DataEvent::Data(NautilusData::Deltas(deltas)) => {
                    Some(("deltas", deltas.instrument_id))
                }
                DataEvent::Data(NautilusData::Quote(quote)) => Some(("quote", quote.instrument_id)),
                _ => None,
            })
            .collect();
        let book_a = ctx.order_books.get(&instrument_a).expect("book A");
        let book_b = ctx.order_books.get(&instrument_b).expect("book B");
        assert_eq!(batches.len(), 2);

        let ts_event = UnixNanos::from(1_700_000_003_000_000_000_u64);
        let ts_init_a = batches[0].ts_init;
        let ts_init_b = batches[1].ts_init;

        assert_eq!(batches[0].instrument_id, instrument_a);
        assert_eq!(batches[0].flags, RecordFlag::F_LAST as u8);
        assert_eq!(batches[0].sequence, 0);
        assert_eq!(batches[0].ts_event, ts_event);
        assert_eq!(
            batches[0].deltas,
            vec![
                OrderBookDelta::new(
                    instrument_a,
                    BookAction::Update,
                    BookOrder::new(
                        OrderSide::Buy,
                        Price::from("0.007"),
                        Quantity::from("20.00"),
                        0,
                    ),
                    0,
                    0,
                    ts_event,
                    ts_init_a,
                ),
                OrderBookDelta::new(
                    instrument_a,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from("0.005"),
                        Quantity::from("0.00"),
                        0,
                    ),
                    0,
                    0,
                    ts_event,
                    ts_init_a,
                ),
                OrderBookDelta::new(
                    instrument_a,
                    BookAction::Update,
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from("0.009"),
                        Quantity::from("30.00"),
                        0,
                    ),
                    RecordFlag::F_LAST as u8,
                    0,
                    ts_event,
                    ts_init_a,
                ),
            ]
        );
        assert_eq!(batches[1].instrument_id, instrument_b);
        assert_eq!(batches[1].flags, RecordFlag::F_LAST as u8);
        assert_eq!(batches[1].sequence, 0);
        assert_eq!(batches[1].ts_event, ts_event);
        assert_eq!(
            batches[1].deltas,
            vec![
                OrderBookDelta::new(
                    instrument_b,
                    BookAction::Update,
                    BookOrder::new(
                        OrderSide::Buy,
                        Price::from("0.997"),
                        Quantity::from("20.00"),
                        0,
                    ),
                    0,
                    0,
                    ts_event,
                    ts_init_b,
                ),
                OrderBookDelta::new(
                    instrument_b,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from("0.995"),
                        Quantity::from("0.00"),
                        0,
                    ),
                    0,
                    0,
                    ts_event,
                    ts_init_b,
                ),
                OrderBookDelta::new(
                    instrument_b,
                    BookAction::Update,
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from("0.999"),
                        Quantity::from("30.00"),
                        0,
                    ),
                    RecordFlag::F_LAST as u8,
                    0,
                    ts_event,
                    ts_init_b,
                ),
            ]
        );
        assert_eq!(
            event_sequence,
            vec![
                ("deltas", instrument_a),
                ("quote", instrument_a),
                ("deltas", instrument_b),
                ("quote", instrument_b),
                ("quote", instrument_a),
                ("quote", instrument_b),
            ]
        );
        assert_eq!(
            quote_instruments,
            vec![instrument_a, instrument_b, instrument_a, instrument_b]
        );
        assert_eq!(book_a.best_bid_price(), Some(Price::from("0.007")));
        assert_eq!(book_a.best_ask_price(), Some(Price::from("0.009")));
        assert_eq!(book_b.best_bid_price(), Some(Price::from("0.997")));
        assert_eq!(book_b.best_ask_price(), Some(Price::from("0.999")));
        assert!(nautilus_model::orderbook::analysis::book_check_integrity(&book_a).is_ok());
        assert!(nautilus_model::orderbook::analysis::book_check_integrity(&book_b).is_ok());
    }

    #[rstest]
    fn price_change_quotes_use_per_entry_resolved_metadata() {
        let asset_a = "0xTOKEN-META-A";
        let asset_b = Ustr::from("0xTOKEN-META-B");
        let market = Ustr::from("0xMARKET");
        let (ctx, mut data_rx) = make_ws_ctx();
        let instrument_id =
            seed_instrument(&ctx, asset_a, Price::from("0.001"), Quantity::from("0.01")).id();
        ctx.token_meta.insert(
            asset_b,
            TokenMeta {
                instrument_id,
                price_precision: 2,
                size_precision: 1,
                min_order_size: None,
                neg_risk: None,
            },
        );
        ctx.active_quote_subs.insert(instrument_id);

        handle_market_message(
            MarketWsMessage::PriceChange(PolymarketQuotes {
                market,
                price_changes: vec![
                    PolymarketQuote {
                        asset_id: Ustr::from(asset_a),
                        price: "0.501".to_string(),
                        side: PolymarketOrderSide::Buy,
                        size: "20".to_string(),
                        hash: String::new(),
                        best_bid: Some("invalid".to_string()),
                        best_ask: Some("0.509".to_string()),
                    },
                    PolymarketQuote {
                        asset_id: asset_b,
                        price: "0.50".to_string(),
                        side: PolymarketOrderSide::Buy,
                        size: "3".to_string(),
                        hash: String::new(),
                        best_bid: Some("0.50".to_string()),
                        best_ask: Some("0.51".to_string()),
                    },
                ],
                timestamp: "1700000003000".to_string(),
            }),
            &ctx,
        );

        let quotes = std::iter::from_fn(|| data_rx.try_recv().ok())
            .filter_map(|event| match event {
                DataEvent::Data(NautilusData::Quote(quote)) => Some(quote),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].instrument_id, instrument_id);
        assert_eq!(quotes[0].bid_price, Price::from("0.50"));
        assert_eq!(quotes[0].ask_price, Price::from("0.51"));
        assert_eq!(quotes[0].bid_size, Quantity::from("3.0"));
        assert_eq!(quotes[0].ask_size, Quantity::from("0.0"));
    }

    #[rstest]
    fn malformed_price_change_entry_preserves_other_updates() {
        let asset_a = "0xTOKEN-BAD";
        let asset_b = "0xTOKEN-GOOD";
        let market = Ustr::from("0xMARKET");
        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let instrument_a =
            seed_instrument(&ctx, asset_a, Price::from("0.001"), Quantity::from("0.01")).id();
        let instrument_b =
            seed_instrument(&ctx, asset_b, Price::from("0.001"), Quantity::from("0.01")).id();
        ctx.active_delta_subs.insert(instrument_a);
        ctx.active_delta_subs.insert(instrument_b);

        handle_market_message(
            make_snapshot(
                market.as_str(),
                asset_a,
                &[("0.003", "10"), ("0.005", "10")],
            ),
            &ctx,
        );
        handle_market_message(
            make_snapshot(
                market.as_str(),
                asset_b,
                &[("0.993", "10"), ("0.995", "10")],
            ),
            &ctx,
        );

        while data_rx.try_recv().is_ok() {}

        let price_changes = vec![
            PolymarketQuote {
                asset_id: Ustr::from(asset_a),
                price: "0.004".to_string(),
                side: PolymarketOrderSide::Buy,
                size: "20".to_string(),
                hash: String::new(),
                best_bid: None,
                best_ask: None,
            },
            PolymarketQuote {
                asset_id: Ustr::from(asset_b),
                price: "0.994".to_string(),
                side: PolymarketOrderSide::Buy,
                size: "20".to_string(),
                hash: String::new(),
                best_bid: None,
                best_ask: None,
            },
            PolymarketQuote {
                asset_id: Ustr::from(asset_a),
                price: "invalid".to_string(),
                side: PolymarketOrderSide::Sell,
                size: "0".to_string(),
                hash: String::new(),
                best_bid: None,
                best_ask: None,
            },
        ];
        handle_market_message(
            MarketWsMessage::PriceChange(PolymarketQuotes {
                market,
                price_changes,
                timestamp: "1700000003000".to_string(),
            }),
            &ctx,
        );

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        let batches: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                DataEvent::Data(NautilusData::Deltas(deltas)) => Some(deltas),
                _ => None,
            })
            .collect();
        let book_a = ctx.order_books.get(&instrument_a).expect("book A");
        let book_b = ctx.order_books.get(&instrument_b).expect("book B");

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].instrument_id, instrument_a);
        assert_eq!(batches[0].deltas.len(), 1);
        assert_eq!(batches[0].flags, RecordFlag::F_LAST as u8);
        assert_eq!(batches[0].deltas[0].flags, RecordFlag::F_LAST as u8);
        assert_eq!(batches[1].instrument_id, instrument_b);
        assert_eq!(batches[1].deltas.len(), 1);
        assert_eq!(batches[1].flags, RecordFlag::F_LAST as u8);
        assert_eq!(batches[1].deltas[0].flags, RecordFlag::F_LAST as u8);
        assert_eq!(book_a.best_bid_price(), Some(Price::from("0.004")));
        assert_eq!(book_b.best_bid_price(), Some(Price::from("0.994")));
        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_a)
        );
    }

    #[rstest]
    fn all_malformed_price_changes_emit_no_delta_batch() {
        let asset_id = "0xTOKEN-INVALID";
        let market = Ustr::from("0xMARKET");
        let (ctx, mut data_rx) = make_ws_ctx();
        let instrument_id =
            seed_instrument(&ctx, asset_id, Price::from("0.001"), Quantity::from("0.01")).id();
        ctx.active_delta_subs.insert(instrument_id);

        handle_market_message(
            MarketWsMessage::PriceChange(PolymarketQuotes {
                market,
                price_changes: vec![
                    PolymarketQuote {
                        asset_id: Ustr::from(asset_id),
                        price: "invalid".to_string(),
                        side: PolymarketOrderSide::Buy,
                        size: "20".to_string(),
                        hash: String::new(),
                        best_bid: None,
                        best_ask: None,
                    },
                    PolymarketQuote {
                        asset_id: Ustr::from(asset_id),
                        price: "0.004".to_string(),
                        side: PolymarketOrderSide::Sell,
                        size: "invalid".to_string(),
                        hash: String::new(),
                        best_bid: None,
                        best_ask: None,
                    },
                ],
                timestamp: "1700000003000".to_string(),
            }),
            &ctx,
        );

        let batches = collect_delta_batches(&mut data_rx);

        assert!(batches.is_empty());
    }

    #[rstest]
    fn quote_path_open_during_pending_window() {
        let asset_id_str = "0xTOKEN8";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.active_quote_subs.insert(instrument_id);
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);

        let prior = QuoteTick::new(
            instrument_id,
            Price::from("0.49"),
            Price::from("0.51"),
            Quantity::from("100.00"),
            Quantity::from("75.00"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        ctx.last_quotes.insert(instrument_id, prior);

        let pc = MarketWsMessage::PriceChange(PolymarketQuotes {
            market: Ustr::from(market),
            price_changes: vec![PolymarketQuote {
                asset_id: Ustr::from(asset_id_str),
                price: "0.50".to_string(),
                side: PolymarketOrderSide::Buy,
                size: "20".to_string(),
                hash: String::new(),
                best_bid: Some("0.50".to_string()),
                best_ask: Some("0.52".to_string()),
            }],
            timestamp: "1700000003000".to_string(),
        });
        handle_market_message(pc, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DataEvent::Data(NautilusData::Deltas(_)))),
            "delta must be dropped while pending: {events:?}",
        );
        let emitted_quote = events
            .iter()
            .find_map(|e| match e {
                DataEvent::Data(NautilusData::Quote(q)) => Some(q),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected quote event, found: {events:?}"));
        assert_eq!(emitted_quote.bid_size, Quantity::from("20.00"));
        assert_eq!(emitted_quote.ask_size, Quantity::from("75.00"));
    }

    #[rstest]
    fn price_change_missing_side_quote_drops_by_default() {
        let asset_id_str = "0xTOKEN11";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_quote_subs.insert(instrument_id);

        let pc = MarketWsMessage::PriceChange(PolymarketQuotes {
            market: Ustr::from(market),
            price_changes: vec![PolymarketQuote {
                asset_id: Ustr::from(asset_id_str),
                price: "0.50".to_string(),
                side: PolymarketOrderSide::Buy,
                size: "20".to_string(),
                hash: String::new(),
                best_bid: Some("0.50".to_string()),
                best_ask: Some("1".to_string()),
            }],
            timestamp: "1700000003000".to_string(),
        });
        handle_market_message(pc, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DataEvent::Data(NautilusData::Quote(_)))),
            "missing ask quote must be dropped by default: {events:?}",
        );
    }

    #[rstest]
    fn price_change_missing_sides_use_current_tick_bounds_when_drop_disabled() {
        let asset_id_str = "0xTOKEN12";
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.drop_quotes_missing_side = false;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.005"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_quote_subs.insert(instrument_id);

        let change = make_tick_change(market, asset_id_str, "0.005", "0.0025");
        handle_market_message(change, &ctx);

        while data_rx.try_recv().is_ok() {}

        let pc = make_price_change(market, asset_id_str, "0.50", "20");
        handle_market_message(pc, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        let emitted_quote = events
            .iter()
            .find_map(|e| match e {
                DataEvent::Data(NautilusData::Quote(q)) => Some(q),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected quote event, found: {events:?}"));
        assert_eq!(emitted_quote.bid_price, Price::from("0.0025"));
        assert_eq!(emitted_quote.bid_size, Quantity::from("0.00"));
        assert_eq!(emitted_quote.ask_price, Price::from("0.9975"));
        assert_eq!(emitted_quote.ask_size, Quantity::from("0.00"));
    }

    #[rstest]
    fn pending_persists_when_snapshot_fails_to_seed() {
        let asset_id_str = "0xTOKEN5";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);

        let empty = MarketWsMessage::Book(PolymarketBookSnapshot {
            market: Ustr::from(market),
            asset_id: Ustr::from(asset_id_str),
            bids: vec![],
            asks: vec![],
            timestamp: "1700000000000".to_string(),
            hash: None,
            min_order_size: None,
            tick_size: None,
            neg_risk: None,
            last_trade_price: None,
        });
        handle_market_message(empty, &ctx);

        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            !events.iter().any(|e| matches!(e, DataEvent::Data(_))),
            "empty snapshot must not emit Data events: {events:?}",
        );
    }

    fn make_price_change_batch(
        market: &str,
        asset_id: &str,
        changes: &[(&str, PolymarketOrderSide, &str)],
    ) -> MarketWsMessage {
        MarketWsMessage::PriceChange(PolymarketQuotes {
            market: Ustr::from(market),
            price_changes: changes
                .iter()
                .map(|(price, side, size)| PolymarketQuote {
                    asset_id: Ustr::from(asset_id),
                    price: price.to_string(),
                    side: *side,
                    size: size.to_string(),
                    hash: String::new(),
                    best_bid: None,
                    best_ask: None,
                })
                .collect(),
            timestamp: "1700000003000".to_string(),
        })
    }

    fn collect_delta_batches(
        data_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) -> Vec<OrderBookDeltas> {
        std::iter::from_fn(|| data_rx.try_recv().ok())
            .filter_map(|event| match event {
                DataEvent::Data(NautilusData::Deltas(deltas)) => Some(*deltas),
                _ => None,
            })
            .collect()
    }

    #[rstest]
    fn effective_deltas_first_snapshot_emits_adds_only() {
        let asset_id_str = "0xTOKEN-EFF1";
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );

        let snap = make_snapshot(
            market,
            asset_id_str,
            &[("0.45", "5"), ("0.49", "10"), ("0.51", "8"), ("0.55", "12")],
        );
        handle_market_message(snap, &ctx);

        let batches = collect_delta_batches(&mut data_rx);
        assert_eq!(batches.len(), 1);

        let batch = &batches[0];
        let ts_event = UnixNanos::from(1_700_000_000_000_000_000_u64);
        let ts_init = batch.ts_init;
        let actual: Vec<_> = batch
            .deltas
            .iter()
            .map(|delta| {
                (
                    delta.instrument_id,
                    delta.action,
                    delta.order.side,
                    delta.order.price,
                    delta.order.size,
                    delta.order.order_id,
                    delta.flags,
                    delta.sequence,
                    delta.ts_event,
                    delta.ts_init,
                )
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                (
                    instrument_id,
                    BookAction::Add,
                    Some(OrderSide::Buy),
                    Price::from("0.49"),
                    Quantity::from("10.00"),
                    0,
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Add,
                    Some(OrderSide::Buy),
                    Price::from("0.45"),
                    Quantity::from("5.00"),
                    0,
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Add,
                    Some(OrderSide::Sell),
                    Price::from("0.51"),
                    Quantity::from("8.00"),
                    0,
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Add,
                    Some(OrderSide::Sell),
                    Price::from("0.55"),
                    Quantity::from("12.00"),
                    0,
                    RecordFlag::F_LAST as u8,
                    0,
                    ts_event,
                    ts_init,
                ),
            ]
        );

        let book = ctx.order_books.get(&instrument_id).expect("book entry");
        assert_eq!(book.best_bid_price(), Some(Price::from("0.49")));
        assert_eq!(book.best_ask_price(), Some(Price::from("0.51")));
    }

    #[rstest]
    fn effective_deltas_snapshot_diffs_against_preceding_price_change() {
        let asset_id_str = "0xTOKEN-EFF9";
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );

        handle_market_message(make_price_change(market, asset_id_str, "0.45", "20"), &ctx);

        while data_rx.try_recv().is_ok() {}

        let mut snapshot = make_snapshot(
            market,
            asset_id_str,
            &[("0.45", "5"), ("0.49", "10"), ("0.51", "8"), ("0.55", "12")],
        );
        let MarketWsMessage::Book(book_snapshot) = &mut snapshot else {
            unreachable!("make_snapshot must return a book message");
        };
        book_snapshot.timestamp = "1700000003000".to_string();
        handle_market_message(snapshot, &ctx);

        let batches = collect_delta_batches(&mut data_rx);
        assert_eq!(batches.len(), 1);

        let batch = &batches[0];
        let ts_event = UnixNanos::from(1_700_000_003_000_000_000_u64);
        let ts_init = batch.ts_init;
        assert!(batch.deltas.iter().all(|delta| {
            delta.instrument_id == instrument_id
                && delta.order.order_id == 0
                && delta.sequence == 0
                && delta.ts_event == ts_event
                && delta.ts_init == ts_init
        }));
        let actual: Vec<_> = batch
            .deltas
            .iter()
            .map(|delta| {
                (
                    delta.action,
                    delta.order.side,
                    delta.order.price,
                    delta.order.size,
                    delta.flags,
                )
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                (
                    BookAction::Add,
                    Some(OrderSide::Buy),
                    Price::from("0.49"),
                    Quantity::from("10.00"),
                    0,
                ),
                (
                    BookAction::Update,
                    Some(OrderSide::Buy),
                    Price::from("0.45"),
                    Quantity::from("5.00"),
                    0,
                ),
                (
                    BookAction::Add,
                    Some(OrderSide::Sell),
                    Price::from("0.51"),
                    Quantity::from("8.00"),
                    0,
                ),
                (
                    BookAction::Add,
                    Some(OrderSide::Sell),
                    Price::from("0.55"),
                    Quantity::from("12.00"),
                    RecordFlag::F_LAST as u8,
                ),
            ]
        );
    }

    #[rstest]
    fn effective_deltas_repeat_snapshot_emits_nothing() {
        let asset_id_str = "0xTOKEN-EFF2";
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);

        let levels = [("0.45", "5"), ("0.49", "10"), ("0.51", "8"), ("0.55", "12")];
        handle_market_message(make_snapshot(market, asset_id_str, &levels), &ctx);

        while data_rx.try_recv().is_ok() {}

        handle_market_message(make_snapshot(market, asset_id_str, &levels), &ctx);

        let batches = collect_delta_batches(&mut data_rx);
        assert!(
            batches.is_empty(),
            "identical snapshot must not emit deltas: {batches:?}",
        );
    }

    #[rstest]
    fn effective_deltas_preserve_price_change_and_update_snapshot_baseline() {
        let asset_id_str = "0xTOKEN-EFF3";
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);

        handle_market_message(
            make_snapshot(
                market,
                asset_id_str,
                &[("0.45", "5"), ("0.49", "10"), ("0.51", "8"), ("0.55", "12")],
            ),
            &ctx,
        );

        while data_rx.try_recv().is_ok() {}

        let pc = make_price_change_batch(
            market,
            asset_id_str,
            &[
                ("0.49", PolymarketOrderSide::Buy, "20"),
                ("0.47", PolymarketOrderSide::Buy, "7"),
                ("0.45", PolymarketOrderSide::Buy, "0"),
            ],
        );
        handle_market_message(pc, &ctx);

        let batches = collect_delta_batches(&mut data_rx);
        assert_eq!(batches.len(), 1);

        let batch = &batches[0];
        let ts_event = UnixNanos::from(1_700_000_003_000_000_000_u64);
        let ts_init = batch.ts_init;
        let actual: Vec<_> = batch
            .deltas
            .iter()
            .map(|delta| {
                (
                    delta.instrument_id,
                    delta.action,
                    delta.order.side,
                    delta.order.price,
                    delta.order.size,
                    delta.order.order_id,
                    delta.flags,
                    delta.sequence,
                    delta.ts_event,
                    delta.ts_init,
                )
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                (
                    instrument_id,
                    BookAction::Update,
                    Some(OrderSide::Buy),
                    Price::from("0.49"),
                    Quantity::from("20.00"),
                    0,
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Update,
                    Some(OrderSide::Buy),
                    Price::from("0.47"),
                    Quantity::from("7.00"),
                    0,
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Delete,
                    Some(OrderSide::Buy),
                    Price::from("0.45"),
                    Quantity::from("0.00"),
                    0,
                    RecordFlag::F_LAST as u8,
                    0,
                    ts_event,
                    ts_init,
                ),
            ]
        );

        handle_market_message(
            make_snapshot(
                market,
                asset_id_str,
                &[("0.47", "7"), ("0.49", "20"), ("0.51", "8"), ("0.55", "12")],
            ),
            &ctx,
        );

        let batches = collect_delta_batches(&mut data_rx);
        assert!(
            batches.is_empty(),
            "matching snapshot must not repeat applied price changes: {batches:?}",
        );

        let book = ctx.order_books.get(&instrument_id).expect("book entry");
        assert_eq!(book.best_bid_price(), Some(Price::from("0.49")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("20.00")));
        assert_eq!(book.best_ask_price(), Some(Price::from("0.51")));
    }

    #[rstest]
    fn effective_deltas_snapshot_emits_exact_net_changes() {
        let asset_id_str = "0xTOKEN-EFF4";
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);

        handle_market_message(
            make_snapshot(
                market,
                asset_id_str,
                &[("0.45", "5"), ("0.49", "10"), ("0.51", "8"), ("0.55", "12")],
            ),
            &ctx,
        );

        while data_rx.try_recv().is_ok() {}

        handle_market_message(
            make_snapshot(
                market,
                asset_id_str,
                &[("0.47", "7"), ("0.49", "20"), ("0.53", "9"), ("0.55", "12")],
            ),
            &ctx,
        );

        let batches = collect_delta_batches(&mut data_rx);
        assert_eq!(batches.len(), 1);

        let batch = &batches[0];
        let ts_event = UnixNanos::from(1_700_000_000_000_000_000_u64);
        let ts_init = batch.ts_init;
        let actual: Vec<_> = batch
            .deltas
            .iter()
            .map(|delta| {
                (
                    delta.instrument_id,
                    delta.action,
                    delta.order.side,
                    delta.order.price,
                    delta.order.size,
                    delta.order.order_id,
                    delta.flags,
                    delta.sequence,
                    delta.ts_event,
                    delta.ts_init,
                )
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                (
                    instrument_id,
                    BookAction::Update,
                    Some(OrderSide::Buy),
                    Price::from("0.49"),
                    Quantity::from("20.00"),
                    0,
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Add,
                    Some(OrderSide::Buy),
                    Price::from("0.47"),
                    Quantity::from("7.00"),
                    0,
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Add,
                    Some(OrderSide::Sell),
                    Price::from("0.53"),
                    Quantity::from("9.00"),
                    0,
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Delete,
                    Some(OrderSide::Buy),
                    Price::from("0.45"),
                    Quantity::from("5.00"),
                    0,
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Delete,
                    Some(OrderSide::Sell),
                    Price::from("0.51"),
                    Quantity::from("8.00"),
                    0,
                    RecordFlag::F_LAST as u8,
                    0,
                    ts_event,
                    ts_init,
                ),
            ]
        );

        let book = ctx.order_books.get(&instrument_id).expect("book entry");
        assert_eq!(book.best_bid_price(), Some(Price::from("0.49")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("20.00")));
        assert_eq!(book.best_ask_price(), Some(Price::from("0.53")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("9.00")));
    }

    #[rstest]
    fn effective_deltas_preserve_v1_delete_order() {
        let asset_id_str = "0xTOKEN-EFF8";
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);

        handle_market_message(
            make_snapshot(
                market,
                asset_id_str,
                &[
                    ("0.43", "3"),
                    ("0.45", "5"),
                    ("0.47", "7"),
                    ("0.49", "9"),
                    ("0.51", "11"),
                    ("0.53", "13"),
                    ("0.55", "15"),
                    ("0.57", "17"),
                ],
            ),
            &ctx,
        );

        while data_rx.try_recv().is_ok() {}

        handle_market_message(
            make_snapshot(
                market,
                asset_id_str,
                &[("0.47", "7"), ("0.49", "9"), ("0.55", "15"), ("0.57", "17")],
            ),
            &ctx,
        );

        let batches = collect_delta_batches(&mut data_rx);
        assert_eq!(batches.len(), 1);

        let batch = &batches[0];
        let ts_event = UnixNanos::from(1_700_000_000_000_000_000_u64);
        let ts_init = batch.ts_init;
        let actual: Vec<_> = batch
            .deltas
            .iter()
            .map(|delta| {
                (
                    delta.instrument_id,
                    delta.action,
                    delta.order,
                    delta.flags,
                    delta.sequence,
                    delta.ts_event,
                    delta.ts_init,
                )
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                (
                    instrument_id,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Buy,
                        Price::from("0.45"),
                        Quantity::from("5.00"),
                        0,
                    ),
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Buy,
                        Price::from("0.43"),
                        Quantity::from("3.00"),
                        0,
                    ),
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from("0.51"),
                        Quantity::from("11.00"),
                        0,
                    ),
                    0,
                    0,
                    ts_event,
                    ts_init,
                ),
                (
                    instrument_id,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Sell,
                        Price::from("0.53"),
                        Quantity::from("13.00"),
                        0,
                    ),
                    RecordFlag::F_LAST as u8,
                    0,
                    ts_event,
                    ts_init,
                ),
            ]
        );
    }

    #[rstest]
    fn effective_deltas_tick_size_change_reseeds_wire_faithful_snapshot() {
        let asset_id_str = "0xTOKEN-EFF5";
        let market = "0xMARKET";

        let (mut ctx, mut data_rx) = make_ws_ctx();
        ctx.compute_effective_deltas = true;
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);

        handle_market_message(
            make_snapshot(
                market,
                asset_id_str,
                &[
                    ("0.455", "5"),
                    ("0.499", "10"),
                    ("0.501", "8"),
                    ("0.555", "12"),
                ],
            ),
            &ctx,
        );

        while data_rx.try_recv().is_ok() {}

        handle_market_message(
            make_tick_change(market, asset_id_str, "0.001", "0.01"),
            &ctx,
        );

        while data_rx.try_recv().is_ok() {}

        handle_market_message(
            make_snapshot(market, asset_id_str, &[("0.45", "5"), ("0.51", "8")]),
            &ctx,
        );

        let batches = collect_delta_batches(&mut data_rx);
        assert_eq!(batches.len(), 1);

        let deltas = &batches[0].deltas;
        assert_eq!(deltas.len(), 3);
        assert_eq!(deltas[0].action, BookAction::Clear);
        assert_eq!(deltas[0].flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(deltas[1].action, BookAction::Add);
        assert_eq!(deltas[1].order.side, Some(OrderSide::Buy));
        assert_eq!(deltas[1].order.price, Price::from("0.45"));
        assert_eq!(deltas[1].order.size, Quantity::from("5.00"));
        assert_eq!(deltas[1].flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(deltas[2].action, BookAction::Add);
        assert_eq!(deltas[2].order.side, Some(OrderSide::Sell));
        assert_eq!(deltas[2].order.price, Price::from("0.51"));
        assert_eq!(deltas[2].order.size, Quantity::from("8.00"));
        assert_eq!(
            deltas[2].flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );

        let book = ctx.order_books.get(&instrument_id).expect("book entry");
        assert_eq!(book.best_bid_price(), Some(Price::from("0.45")));
        assert_eq!(book.best_ask_price(), Some(Price::from("0.51")));
    }

    #[rstest]
    fn effective_deltas_apply_failure_leaves_book_untouched() {
        let instrument_id = InstrumentId::from("0xTOKEN-EFF7.POLYMARKET");
        let other_id = InstrumentId::from("0xTOKEN-OTHER.POLYMARKET");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let seed = OrderBookDeltas::new(
            instrument_id,
            vec![OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(OrderSide::Buy, Price::from("0.49"), Quantity::from("10"), 0),
                0,
                0,
                UnixNanos::from(1_u64),
                UnixNanos::from(1_u64),
            )],
        );
        let seeded = apply_snapshot_and_diff(&mut book, &seed).expect("seed applies");
        assert!(seeded.is_some());
        assert_eq!(book.best_bid_price(), Some(Price::from("0.49")));

        let mismatched = OrderBookDeltas::new(
            other_id,
            vec![OrderBookDelta::new(
                other_id,
                BookAction::Add,
                BookOrder::new(OrderSide::Buy, Price::from("0.51"), Quantity::from("8"), 0),
                0,
                0,
                UnixNanos::from(2_u64),
                UnixNanos::from(2_u64),
            )],
        );
        let result = apply_snapshot_and_diff(&mut book, &mismatched);

        assert!(result.is_err());
        assert_eq!(book.best_bid_price(), Some(Price::from("0.49")));
        assert_eq!(book.update_count, 1);
    }

    #[rstest]
    fn wire_faithful_repeat_snapshot_reemits_full_batch() {
        let asset_id_str = "0xTOKEN-EFF6";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);

        let levels = [("0.45", "5"), ("0.49", "10"), ("0.51", "8"), ("0.55", "12")];
        handle_market_message(make_snapshot(market, asset_id_str, &levels), &ctx);
        assert!(!ctx.order_books.contains_key(&instrument_id));

        while data_rx.try_recv().is_ok() {}

        handle_market_message(make_snapshot(market, asset_id_str, &levels), &ctx);
        assert!(!ctx.order_books.contains_key(&instrument_id));

        let batches = collect_delta_batches(&mut data_rx);
        assert_eq!(batches.len(), 1);

        let deltas = &batches[0].deltas;
        assert_eq!(deltas.len(), 5);
        assert_eq!(deltas[0].action, BookAction::Clear);
        assert!(
            deltas
                .iter()
                .all(|d| d.flags & RecordFlag::F_SNAPSHOT as u8 != 0),
            "wire-faithful emission must keep F_SNAPSHOT on every record: {deltas:?}",
        );
    }
}
