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

use std::sync::{Arc, Mutex as StdMutex};

use dashmap::DashMap;
use nautilus_common::{live::get_runtime, messages::DataEvent};
use nautilus_core::{AtomicMap, AtomicSet, time::AtomicTime};
use nautilus_model::{
    data::{Data as NautilusData, InstrumentStatus, OrderBookDeltas_API, QuoteTick},
    enums::{BookType, MarketStatusAction},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    NEW_MARKET_EMPTY_RECHECK_DELAY, NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS,
    instruments::{TokenMeta, cache_instrument},
};
use crate::{
    filters::InstrumentFilter,
    http::{
        clob::PolymarketClobPublicClient, gamma::PolymarketGammaHttpClient,
        parse::rebuild_instrument_with_tick_size, query::GetGammaMarketsParams,
    },
    resolve::{ResolveContext, ResolveWatchEntry, apply_condition_resolution},
    websocket::{
        messages::{MarketWsMessage, PolymarketNewMarket, PolymarketQuotes, PolymarketWsMessage},
        parse::{
            parse_book_deltas, parse_book_snapshot, parse_quote_from_price_change,
            parse_quote_from_snapshot, parse_timestamp_ms, parse_trade_tick,
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
    pub(super) clob_public_client: PolymarketClobPublicClient,
    pub(super) filters: Vec<Arc<dyn InstrumentFilter>>,
    pub(super) order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    pub(super) last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    pub(super) active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) resolve_poll_watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    pub(super) resolve_watch_apply_mutex: Arc<StdMutex<()>>,
    pub(super) pending_snapshot_after_tick_change: Arc<AtomicSet<InstrumentId>>,
    pub(super) new_market_inflight_keys: Arc<DashMap<String, ()>>,
    pub(super) new_market_fetch_semaphore: Arc<tokio::sync::Semaphore>,
    pub(super) subscribe_new_markets: bool,
    pub(super) new_market_filter: Option<Arc<dyn InstrumentFilter>>,
    pub(super) cancellation_token: CancellationToken,
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

pub(super) fn new_market_dedupe_key(nm: &PolymarketNewMarket) -> String {
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
        }
    }
}

pub(super) fn handle_market_message(message: MarketWsMessage, ctx: &WsMessageContext) {
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
            let mut book_seeded = false;

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

                        match book.apply_deltas(&deltas) {
                            Ok(()) => book_seeded = true,
                            Err(e) => {
                                log::error!(
                                    "Failed to apply book snapshot for {instrument_id}: {e}"
                                );
                            }
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
                    Ok(Some(quote)) => emit_quote_if_changed(ctx, instrument_id, quote),
                    Ok(None) => {}
                    Err(e) => log::error!("Failed to parse quote from snapshot: {e}"),
                }
            }

            if book_seeded
                && ctx
                    .pending_snapshot_after_tick_change
                    .contains(&instrument_id)
            {
                ctx.pending_snapshot_after_tick_change
                    .remove(&instrument_id);
                log::info!("Resumed book for {instrument_id} after tick size change");
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
                let pending = ctx
                    .pending_snapshot_after_tick_change
                    .contains(&instrument_id);

                if pending && ctx.active_delta_subs.contains(&instrument_id) {
                    log::debug!(
                        "Dropping book delta for {instrument_id}: awaiting snapshot after tick size change",
                    );
                } else if ctx.active_delta_subs.contains(&instrument_id) {
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
                                log::error!("Failed to apply book deltas for {instrument_id}: {e}");
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

            log::info!(
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

            if let Some(existing) = existing {
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
            let data_sender = ctx.data_sender.clone();
            let clock = ctx.clock;
            let cancellation = ctx.cancellation_token.clone();
            let inflight_keys = ctx.new_market_inflight_keys.clone();
            let fetch_semaphore = ctx.new_market_fetch_semaphore.clone();
            let active = nm.active;

            get_runtime().spawn(async move {
                let _inflight_guard =
                    NewMarketInflightGuard::new(inflight_keys, dedupe_key.clone());
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
                            condition_ids: Some(condition_id.clone()),
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

                                let transient_hit = transient.iter().any(|cid| cid == &condition_id);
                                if attempt < NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS {
                                    attempt += 1;
                                    let reason = if transient_hit {
                                        "transient hydration"
                                    } else {
                                        "empty result"
                                    };
                                    log::info!(
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

                            if let Err(e) = data_sender.send(DataEvent::InstrumentStatus(status)) {
                                log::error!(
                                    "Failed to emit instrument status for {instrument_id}: {e}"
                                );
                            }
                        }
                    }
                    Err(e) => log::warn!(
                        "Failed to fetch instruments for new market slug '{slug}': {e}"
                    ),
                }
            });
        }

        MarketWsMessage::MarketResolved(resolved) => {
            let emitted = apply_condition_resolution(
                &ctx.resolve_context(),
                resolved.market.as_str(),
                &resolved.winning_asset_id,
                &resolved.winning_outcome,
            );

            if emitted > 0 {
                log::info!(
                    "Applied market_resolved for condition_id={} winner={} ({}) tracked_instruments={emitted}",
                    resolved.market,
                    resolved.winning_asset_id,
                    resolved.winning_outcome
                );
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

fn emit_quote_if_changed(ctx: &WsMessageContext, instrument_id: InstrumentId, quote: QuoteTick) {
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
