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
//! Tick-size changes are handled as book epoch transitions: the local order
//! book is dropped, incremental `price_change` deltas are gated through
//! `pending_snapshot_after_tick_change`, and the gate clears once the next
//! venue snapshot reseeds the book under the new precision. The quote arm of
//! `price_change` stays open through the gap because each payload carries
//! `best_bid` / `best_ask` on the new grid; `last_quotes` is preserved so the
//! unchanged side's size carries forward. See
//! `docs/integrations/polymarket.md` for the full description.

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
    instruments::{TokenMeta, cache_instrument_if_active},
};
use crate::{
    filters::InstrumentFilter,
    http::{
        clob::PolymarketClobPublicClient, gamma::PolymarketGammaHttpClient,
        parse::rebuild_instrument_with_tick_size, query::GetGammaMarketsParams,
    },
    resolve::{ResolveContext, ResolveWatchEntry, apply_condition_resolution},
    rtds::PolymarketRtdsFeed,
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
    pub(super) rtds_feed: PolymarketRtdsFeed,
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

                            if !cache_instrument_if_active(
                                clock.get_time_ns(),
                                &instruments,
                                &token_meta,
                                &inst,
                            ) {
                                log::debug!(
                                    "Skipping expired new market instrument {} during cache update",
                                    inst.id()
                                );
                                continue;
                            }

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
                log::debug!(
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

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        num::NonZeroUsize,
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
        response::Json,
        routing::get,
    };
    use chrono::{Duration as ChronoDuration, Utc};
    use futures_util::StreamExt;
    use nautilus_common::{
        clients::DataClient,
        live::runner::replace_data_event_sender,
        messages::{
            DataResponse,
            data::{RequestBookSnapshot, RequestCustomData, RequestTrades, SubscribeQuotes},
        },
        testing::wait_until_async,
    };
    use nautilus_core::{Params, UUID4, UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::{
        data::{CustomData as ModelCustomData, DataType},
        enums::{InstrumentCloseType, OrderSide, PositionSide},
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
        super::{PolymarketDataClient, instruments::cache_instrument},
        *,
    };
    use crate::{
        common::{
            consts::{POLYMARKET_CLIENT_ID, POLYMARKET_VENUE},
            enums::PolymarketOrderSide,
        },
        config::PolymarketDataClientConfig,
        http::data_api::PolymarketDataApiHttpClient,
        resolve::{
            PolymarketResolveRequestSummaryData, RESOLVE_REQUEST_TYPE_NAME, ResolveBatchErrorMode,
            fetch_and_apply_resolutions_by_condition_ids, pause_resolve_watch_entries,
            update_resolve_watchlist_from_position_event,
            upsert_resolve_watch_entry_from_instrument,
        },
        websocket::{
            client::PolymarketWebSocketClient,
            messages::{
                PolymarketBookLevel, PolymarketBookSnapshot, PolymarketMarketResolved,
                PolymarketQuote, PolymarketTickSizeChange,
            },
        },
    };

    fn is_resolve_response(event: &DataEvent) -> bool {
        matches!(event, DataEvent::Response(DataResponse::Data(_)))
    }

    #[derive(Clone, Default)]
    struct RtdsTestServerState {
        received_payloads: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
    }

    async fn handle_rtds_upgrade(
        ws: WebSocketUpgrade,
        State(state): State<RtdsTestServerState>,
    ) -> axum::response::Response {
        ws.on_upgrade(move |socket| handle_rtds_socket(socket, state))
    }

    async fn handle_rtds_socket(mut socket: WebSocket, state: RtdsTestServerState) {
        while let Some(result) = socket.next().await {
            let Ok(message) = result else { break };

            match message {
                AxumWsMessage::Text(text) => {
                    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    state.received_payloads.lock().await.push(payload);
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
        WsMessageContext,
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
        let clob_public_client =
            PolymarketClobPublicClient::new(Some("http://localhost".to_string()), 5)
                .expect("clob client");

        let ctx = WsMessageContext {
            clock: get_atomic_clock_realtime(),
            data_sender: data_tx.clone(),
            token_meta: Arc::new(DashMap::new()),
            instruments: Arc::new(AtomicMap::new()),
            gamma_client,
            clob_public_client,
            filters: vec![],
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
                PolymarketDataClientConfig::default().new_market_fetch_max_concurrency,
            )),
            rtds_feed: crate::rtds::PolymarketRtdsFeed::new(
                "ws://localhost/rtds".to_string(),
                TransportBackend::default(),
                get_atomic_clock_realtime(),
                data_tx,
            ),
            subscribe_new_markets: false,
            new_market_filter: None,
            cancellation_token: CancellationToken::new(),
        };

        (ctx, data_rx)
    }

    fn make_ws_ctx() -> (
        WsMessageContext,
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
        cache_instrument(&ctx.instruments, &ctx.token_meta, &inst);
        inst
    }

    #[derive(Clone, Copy, Default)]
    struct SeedInstrumentContext<'a> {
        market_slug: Option<&'a str>,
        market_id: Option<&'a str>,
        condition_id: Option<&'a str>,
        expiration_ns: Option<UnixNanos>,
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

            binary.info = Some(info);
        }

        cache_instrument(&ctx.instruments, &ctx.token_meta, &inst);
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
            event_id: UUID4::new(),
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
        })
    }

    fn stub_position_opened_event(instrument_id: InstrumentId) -> PositionEvent {
        stub_position_opened_event_with_position_id(instrument_id, "P-1")
    }

    fn make_client_ws_ctx(client: &PolymarketDataClient) -> WsMessageContext {
        WsMessageContext {
            clock: client.clock,
            data_sender: client.data_sender.clone(),
            token_meta: client.token_meta.clone(),
            instruments: client.instruments.clone(),
            gamma_client: client.provider.http_client().clone(),
            clob_public_client: client.clob_public_client.clone(),
            filters: client.provider.filters(),
            order_books: client.order_books.clone(),
            last_quotes: client.last_quotes.clone(),
            active_quote_subs: client.active_quote_subs.clone(),
            active_delta_subs: client.active_delta_subs.clone(),
            active_trade_subs: client.active_trade_subs.clone(),
            resolve_poll_watchlist: client.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: client.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: client.pending_snapshot_after_tick_change.clone(),
            new_market_inflight_keys: client.new_market_inflight_keys.clone(),
            new_market_fetch_semaphore: client.new_market_fetch_semaphore.clone(),
            rtds_feed: client.rtds_feed.clone(),
            subscribe_new_markets: client.config.subscribe_new_markets,
            new_market_filter: client.config.new_market_filter.clone(),
            cancellation_token: client.cancellation_token.clone(),
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
        }))
    }

    fn gamma_market_expired_fixture_value() -> Value {
        serde_json::from_str(include_str!("../../test_data/gamma_market.json"))
            .expect("gamma market fixture json")
    }

    fn gamma_market_recheck_fixture_value() -> Value {
        let mut value = gamma_market_expired_fixture_value();
        let future_date = (Utc::now() + ChronoDuration::days(365)).date_naive();
        let end_date = format!("{}T00:00:00Z", future_date.format("%Y-%m-%d"));

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

    const TEST_CONDITION_ID: &str =
        "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63b";
    const TEST_TOKEN_ID_YES: &str =
        "104239898038807136052399800151408521467737075933964991162589336683346093173875";

    fn fixture_yes_instrument_id() -> InstrumentId {
        InstrumentId::from(format!("{TEST_CONDITION_ID}-{TEST_TOKEN_ID_YES}.POLYMARKET").as_str())
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

    async fn start_new_market_test_server(state: NewMarketFetchTestServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let router = Router::new()
            .route("/markets", get(handle_new_market_gamma_markets))
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

        ctx.cancellation_token.cancel();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

        loop {
            if ctx.new_market_inflight_keys.is_empty() {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "expected in-flight key cleanup after cancellation"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
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

    async fn start_mock_server(state: TestServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let router = Router::new()
            .route("/markets", get(handle_gamma_markets))
            .route("/markets/{condition_id}", get(handle_clob_market))
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
        addr
    }

    #[derive(Clone)]
    struct ExpiredAutoLoadServerState {
        requests: Arc<AtomicUsize>,
        response: Value,
    }

    async fn handle_expired_auto_load_markets(
        State(state): State<ExpiredAutoLoadServerState>,
    ) -> Json<Value> {
        state.requests.fetch_add(1, Ordering::SeqCst);
        Json(state.response)
    }

    async fn start_expired_auto_load_test_server(state: ExpiredAutoLoadServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let router = Router::new()
            .route("/markets", get(handle_expired_auto_load_markets))
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
        let ws = PolymarketWebSocketClient::new_market(
            Some(format!("ws://{addr}/ws/market")),
            false,
            TransportBackend::default(),
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
        let ws = PolymarketWebSocketClient::new_market(
            Some("ws://localhost/ws/market".to_string()),
            false,
            TransportBackend::default(),
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
                expiration_ns: Some(expiration_ns),
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
                expiration_ns: Some(expiration_ns),
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
                expiration_ns: Some(expiration_ns),
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
                expiration_ns: Some(expiration_ns),
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
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
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
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-POLL"),
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
                condition_id: Some("0xCOND-POLL"),
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

        client.spawn_resolve_poll_task();

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
            .await;

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
    #[tokio::test]
    async fn auto_load_expired_instrument_retires_without_retrying() {
        let state = ExpiredAutoLoadServerState {
            requests: Arc::new(AtomicUsize::new(0)),
            response: serde_json::json!([gamma_market_expired_fixture_value()]),
        };
        let addr = start_expired_auto_load_test_server(state.clone()).await;
        let (mut client, _data_rx) = create_test_client(addr);
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

        let quiet_start = tokio::time::Instant::now();
        while quiet_start.elapsed() < StdDuration::from_millis(100) {
            assert_eq!(state.requests.load(Ordering::SeqCst), 1);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(state.requests.load(Ordering::SeqCst), 1);
        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from(TEST_TOKEN_ID_YES))
        );
        assert!(!client.instruments.load().contains_key(&instrument_id));
    }

    #[rstest]
    #[case::quotes(ExpiredPath::Quotes, "0xTOKEN_EXPIRED")]
    #[case::book(ExpiredPath::BookSnapshot, "0xTOKEN_EXPIRED_BOOK")]
    #[case::trades(ExpiredPath::Trades, "0xTOKEN_EXPIRED_TRADES")]
    fn cached_expired_instrument_live_paths_are_rejected(
        #[case] path: ExpiredPath,
        #[case] raw_symbol: &str,
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

        assert!(result.is_err());
        if matches!(path, ExpiredPath::Quotes) {
            assert!(!client.active_quote_subs.contains(&expired.id()));
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

    #[rstest]
    fn tick_size_change_clears_book_and_marks_pending() {
        let asset_id_str = "0xTOKEN";
        let token_ustr = Ustr::from(asset_id_str);
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
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
        assert!(ctx.order_books.contains_key(&instrument_id));
    }

    #[rstest]
    fn tick_size_change_noop_preserves_book_and_quote() {
        let asset_id_str = "0xTOKEN_NOOP";
        let token_ustr = Ustr::from(asset_id_str);
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
    fn tick_size_change_same_precision_different_value_triggers_epoch() {
        let asset_id_str = "0xTOKEN_VALUE";
        let token_ustr = Ustr::from(asset_id_str);
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.005"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );

        let change = make_tick_change(market, asset_id_str, "0.005", "0.001");
        handle_market_message(change, &ctx);

        assert!(!ctx.order_books.contains_key(&instrument_id));
        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
        assert_eq!(meta.price_precision, 3);

        let rebuilt = ctx
            .instruments
            .load()
            .get(&instrument_id)
            .cloned()
            .expect("rebuilt instrument");
        assert_eq!(rebuilt.price_increment(), Price::from("0.001"));

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
            "expected rebuilt instrument event, found: {events:?}",
        );
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
        });
        handle_market_message(snap, &ctx);

        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(!ctx.order_books.contains_key(&instrument_id));
    }

    #[rstest]
    fn price_change_emits_delta_when_not_pending() {
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
        assert_eq!(book.best_bid_price(), Some(Price::from("0.50")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("20.00")));
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
}
