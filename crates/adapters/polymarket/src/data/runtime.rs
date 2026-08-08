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

//! Shared runtime helpers for the Polymarket data client.

use std::sync::{Arc, Mutex as StdMutex};

use ahash::AHashSet;
use dashmap::DashMap;
use nautilus_core::{AtomicMap, AtomicSet, UnixNanos};
use nautilus_model::{
    data::QuoteTick,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use ustr::Ustr;

use super::{
    instruments::TokenMeta,
    subscriptions::{resolve_token_id_from, sync_ws_subscription_async},
};
use crate::resolve::ResolveWatchEntry;

/// Returns `true` if `instrument` may be retired from local runtime state.
///
/// Delegates to [`crate::filters::is_retirable`] so the data and execution clients share one
/// definition of retirement.
pub(crate) fn is_instrument_retirable(instrument: &InstrumentAny, now_ns: UnixNanos) -> bool {
    crate::filters::is_retirable(instrument, now_ns)
}

pub(crate) fn seed_token_meta_from_live_instruments(
    now_ns: UnixNanos,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
) {
    let loaded = instruments.load();

    for instrument in loaded.values() {
        if is_instrument_retirable(instrument, now_ns) {
            continue;
        }

        token_meta.insert(
            Ustr::from(instrument.raw_symbol().as_str()),
            TokenMeta {
                instrument_id: instrument.id(),
                price_precision: instrument.price_precision(),
                size_precision: instrument.size_precision(),
            },
        );
    }
}

fn is_watchlisted_instrument(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    instrument_id: InstrumentId,
) -> bool {
    let snapshot = watchlist.load();
    snapshot.values().any(|entry| {
        entry
            .tracked
            .values()
            .any(|tracked| tracked.instrument_id == instrument_id)
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared adapter state is held in Arcs"
)]
fn has_live_runtime_state(
    instrument_id: InstrumentId,
    token_id: Option<&str>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    order_books: &Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: &Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: &Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: &Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: &Arc<AtomicSet<InstrumentId>>,
    pending_snapshot_after_tick_change: &Arc<AtomicSet<InstrumentId>>,
    pending_auto_loads: &Arc<StdMutex<AHashSet<InstrumentId>>>,
    ws_open_tokens: &Arc<AtomicSet<Ustr>>,
) -> bool {
    if active_quote_subs.contains(&instrument_id)
        || active_delta_subs.contains(&instrument_id)
        || active_trade_subs.contains(&instrument_id)
        || pending_snapshot_after_tick_change.contains(&instrument_id)
        || order_books.contains_key(&instrument_id)
        || last_quotes.contains_key(&instrument_id)
    {
        return true;
    }

    if pending_auto_loads
        .lock()
        .expect("pending_auto_loads mutex poisoned")
        .contains(&instrument_id)
    {
        return true;
    }

    let Some(token_id) = token_id else {
        return false;
    };
    let token_id = Ustr::from(token_id);
    token_meta.contains_key(&token_id) || ws_open_tokens.contains(&token_id)
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared adapter state is held in Arcs"
)]
pub(crate) async fn retire_local_instrument_state(
    instrument_id: InstrumentId,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    order_books: &Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: &Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: &Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: &Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: &Arc<AtomicSet<InstrumentId>>,
    resolve_poll_watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    pending_snapshot_after_tick_change: &Arc<AtomicSet<InstrumentId>>,
    pending_auto_loads: &Arc<StdMutex<AHashSet<InstrumentId>>>,
    ws_open_tokens: &Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: &Arc<tokio::sync::Mutex<()>>,
    ws: &crate::websocket::pool::PolymarketMarketPoolHandle,
) {
    let token_id = resolve_token_id_from(instruments, instrument_id).ok();

    active_quote_subs.remove(&instrument_id);
    active_delta_subs.remove(&instrument_id);
    active_trade_subs.remove(&instrument_id);

    if let Some(token_id) = token_id.as_ref() {
        sync_ws_subscription_async(
            instrument_id,
            token_id.clone(),
            active_quote_subs.clone(),
            active_delta_subs.clone(),
            active_trade_subs.clone(),
            ws_open_tokens.clone(),
            ws_sub_mutex.clone(),
            ws.clone(),
        )
        .await;
    }

    pending_snapshot_after_tick_change.remove(&instrument_id);
    {
        let mut pending = pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned");
        pending.remove(&instrument_id);
    }

    order_books.remove(&instrument_id);
    last_quotes.remove(&instrument_id);

    if let Some(token_id) = token_id {
        token_meta.remove(&Ustr::from(token_id.as_str()));
    }

    let keep_local_metadata = is_watchlisted_instrument(resolve_poll_watchlist, instrument_id);
    if !keep_local_metadata {
        instruments.remove(&instrument_id);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared adapter state is held in Arcs"
)]
pub(crate) async fn retire_expired_local_instruments(
    now_ns: UnixNanos,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    order_books: &Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: &Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: &Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: &Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: &Arc<AtomicSet<InstrumentId>>,
    resolve_poll_watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    pending_snapshot_after_tick_change: &Arc<AtomicSet<InstrumentId>>,
    pending_auto_loads: &Arc<StdMutex<AHashSet<InstrumentId>>>,
    ws_open_tokens: &Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: &Arc<tokio::sync::Mutex<()>>,
    ws: &crate::websocket::pool::PolymarketMarketPoolHandle,
) {
    let expired_candidates: Vec<(InstrumentId, String)> = {
        let loaded = instruments.load();
        loaded
            .iter()
            .filter_map(|(instrument_id, instrument)| {
                is_instrument_retirable(instrument, now_ns)
                    .then_some((*instrument_id, instrument.raw_symbol().as_str().to_string()))
            })
            .collect()
    };

    let mut expired_ids = Vec::new();

    for (instrument_id, token_id) in expired_candidates {
        let keep_local_metadata = is_watchlisted_instrument(resolve_poll_watchlist, instrument_id);
        if keep_local_metadata
            && !has_live_runtime_state(
                instrument_id,
                Some(token_id.as_str()),
                token_meta,
                order_books,
                last_quotes,
                active_quote_subs,
                active_delta_subs,
                active_trade_subs,
                pending_snapshot_after_tick_change,
                pending_auto_loads,
                ws_open_tokens,
            )
        {
            continue;
        }

        expired_ids.push(instrument_id);
    }

    if !expired_ids.is_empty() {
        // Dropping a live subscription must be visible: this path was previously silent at every
        // level, which is what made the endDate defect so hard to diagnose.
        log::info!(
            "Retiring {} Polymarket instrument(s) closed at the venue: {}",
            expired_ids.len(),
            expired_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        );
    }

    for instrument_id in expired_ids {
        retire_local_instrument_state(
            instrument_id,
            instruments,
            token_meta,
            order_books,
            last_quotes,
            active_quote_subs,
            active_delta_subs,
            active_trade_subs,
            resolve_poll_watchlist,
            pending_snapshot_after_tick_change,
            pending_auto_loads,
            ws_open_tokens,
            ws_sub_mutex,
            ws,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use ahash::AHashSet;
    use dashmap::DashMap;
    use nautilus_core::{AtomicMap, AtomicSet, UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::{
        data::QuoteTick,
        enums::BookType,
        identifiers::{InstrumentId, PositionId, Symbol},
        instruments::{Instrument, stubs::binary_option},
        orderbook::OrderBook,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        resolve::upsert_resolve_watch_entry_from_instrument,
        websocket::{handler::HandlerCommand, pool::PolymarketMarketPoolHandle},
    };

    fn seed_expired_instrument(raw_symbol: &str, condition_id: &str) -> InstrumentAny {
        let clock = get_atomic_clock_realtime();
        let mut binary = binary_option();
        binary.id = InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str());
        binary.raw_symbol = Symbol::new(raw_symbol);
        binary.currency = Currency::pUSD();
        binary.activation_ns = UnixNanos::default();
        binary.expiration_ns =
            UnixNanos::from(clock.get_time_ns().as_u64().saturating_sub(1_000_000_000));

        let mut info = nautilus_core::Params::new();
        info.insert(
            "token_id".to_string(),
            serde_json::Value::String(raw_symbol.to_string()),
        );
        info.insert(
            "condition_id".to_string(),
            serde_json::Value::String(condition_id.to_string()),
        );
        binary.info = Some(info);

        InstrumentAny::BinaryOption(binary)
    }

    fn seed_cached_instrument(
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
        instrument: &InstrumentAny,
    ) {
        token_meta.insert(
            Ustr::from(instrument.raw_symbol().as_str()),
            TokenMeta {
                instrument_id: instrument.id(),
                price_precision: instrument.price_precision(),
                size_precision: instrument.size_precision(),
            },
        );
        instruments.insert(instrument.id(), instrument.clone());
    }

    #[allow(clippy::too_many_arguments, reason = "test seeds shared runtime state")]
    fn seed_runtime_state(
        instrument: &InstrumentAny,
        order_books: &Arc<DashMap<InstrumentId, OrderBook>>,
        last_quotes: &Arc<DashMap<InstrumentId, QuoteTick>>,
        active_quote_subs: &Arc<AtomicSet<InstrumentId>>,
        active_delta_subs: &Arc<AtomicSet<InstrumentId>>,
        active_trade_subs: &Arc<AtomicSet<InstrumentId>>,
        pending_snapshot_after_tick_change: &Arc<AtomicSet<InstrumentId>>,
        pending_auto_loads: &Arc<StdMutex<AHashSet<InstrumentId>>>,
        ws_open_tokens: &Arc<AtomicSet<Ustr>>,
    ) {
        let instrument_id = instrument.id();

        active_quote_subs.insert(instrument_id);
        active_delta_subs.insert(instrument_id);
        active_trade_subs.insert(instrument_id);
        pending_snapshot_after_tick_change.insert(instrument_id);
        pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned")
            .insert(instrument_id);
        ws_open_tokens.insert(Ustr::from(instrument.raw_symbol().as_str()));
        order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );
        last_quotes.insert(
            instrument_id,
            QuoteTick::new(
                instrument_id,
                Price::from("0.504"),
                Price::from("0.506"),
                Quantity::from("5.00"),
                Quantity::from("8.00"),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
        );
    }

    #[rstest]
    #[tokio::test]
    async fn retire_expired_local_instruments_retires_watchlisted_runtime_state_once() {
        let instruments = Arc::new(AtomicMap::new());
        let token_meta = Arc::new(DashMap::new());
        let order_books = Arc::new(DashMap::new());
        let last_quotes = Arc::new(DashMap::new());
        let active_quote_subs = Arc::new(AtomicSet::new());
        let active_delta_subs = Arc::new(AtomicSet::new());
        let active_trade_subs = Arc::new(AtomicSet::new());
        let resolve_poll_watchlist = Arc::new(AtomicMap::new());
        let pending_snapshot_after_tick_change = Arc::new(AtomicSet::new());
        let pending_auto_loads = Arc::new(StdMutex::new(AHashSet::new()));
        let ws_open_tokens = Arc::new(AtomicSet::new());
        let ws_sub_mutex = Arc::new(tokio::sync::Mutex::new(()));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let ws = PolymarketMarketPoolHandle::test_single_shard(tx, &["0xTOKEN_WATCHED"]);

        let inst = seed_expired_instrument("0xTOKEN_WATCHED", "0xCOND-WATCHED");
        let instrument_id = inst.id();
        let token_id = Ustr::from(inst.raw_symbol().as_str());
        seed_cached_instrument(&instruments, &token_meta, &inst);
        upsert_resolve_watch_entry_from_instrument(
            &resolve_poll_watchlist,
            &inst,
            PositionId::new("P-1"),
        );
        seed_runtime_state(
            &inst,
            &order_books,
            &last_quotes,
            &active_quote_subs,
            &active_delta_subs,
            &active_trade_subs,
            &pending_snapshot_after_tick_change,
            &pending_auto_loads,
            &ws_open_tokens,
        );

        let now_ns = get_atomic_clock_realtime().get_time_ns();
        retire_expired_local_instruments(
            now_ns,
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
        )
        .await;

        match rx
            .try_recv()
            .expect("expected first retirement unsubscribe")
        {
            HandlerCommand::UnsubscribeMarket(ids) => {
                assert_eq!(ids, vec![inst.raw_symbol().as_str().to_string()]);
            }
            other => panic!("unexpected WS command: {other:?}"),
        }
        assert!(rx.try_recv().is_err());
        assert!(instruments.load().contains_key(&instrument_id));
        assert!(!token_meta.contains_key(&token_id));

        retire_expired_local_instruments(
            now_ns,
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
        )
        .await;

        assert!(
            rx.try_recv().is_err(),
            "watchlisted expired instruments should retire live runtime state only once",
        );
        assert!(instruments.load().contains_key(&instrument_id));
        assert!(!token_meta.contains_key(&token_id));
        assert!(!order_books.contains_key(&instrument_id));
        assert!(!last_quotes.contains_key(&instrument_id));
        assert!(!active_quote_subs.contains(&instrument_id));
        assert!(!active_delta_subs.contains(&instrument_id));
        assert!(!active_trade_subs.contains(&instrument_id));
        assert!(!pending_snapshot_after_tick_change.contains(&instrument_id));
        assert!(
            pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned")
                .is_empty()
        );
        assert!(!ws_open_tokens.contains(&token_id));
    }

    #[rstest]
    fn instrument_before_expiration_is_never_retirable() {
        let clock = get_atomic_clock_realtime();
        let mut binary = binary_option();
        binary.expiration_ns =
            UnixNanos::from(clock.get_time_ns().as_u64().saturating_add(60_000_000_000));
        let instrument = InstrumentAny::BinaryOption(binary);

        assert!(!is_instrument_retirable(&instrument, clock.get_time_ns()));
    }

    #[rstest]
    fn expired_instrument_open_at_venue_is_retained() {
        // The defect: the venue keeps trading past `endDate`, so the market must stay carried.
        let clock = get_atomic_clock_realtime();
        let instrument =
            expired_instrument_with_state("0xTOKEN_VENUE", "0xCOND-VENUE", Some(false));

        assert!(!is_instrument_retirable(&instrument, clock.get_time_ns()));
    }

    #[rstest]
    fn expired_instrument_closed_at_venue_is_retired() {
        let clock = get_atomic_clock_realtime();
        let instrument = expired_instrument_with_state("0xTOKEN_VENUE", "0xCOND-VENUE", Some(true));

        assert!(is_instrument_retirable(&instrument, clock.get_time_ns()));
    }

    #[rstest]
    fn expired_instrument_without_venue_state_is_retired() {
        // Instruments cached before this field existed keep the previous behaviour.
        let clock = get_atomic_clock_realtime();
        let instrument = expired_instrument_with_state("0xTOKEN_VENUE", "0xCOND-VENUE", None);

        assert!(is_instrument_retirable(&instrument, clock.get_time_ns()));
    }

    #[rstest]
    fn live_venue_payload_past_end_date_is_retained() {
        // Captured from Gamma on 2026-08-08: `endDate` two months in the past while the venue
        // still reports the market open and accepting orders.
        let market: crate::http::models::GammaMarket = serde_json::from_str(include_str!(
            "../../test_data/gamma_market_past_end_date_open.json"
        ))
        .expect("gamma market fixture json");

        assert_eq!(market.end_date.as_deref(), Some("2026-06-01T00:00:00Z"));
        assert_eq!(market.closed, Some(false));
        assert_eq!(market.accepting_orders, Some(true));

        let clock = get_atomic_clock_realtime();
        let defs = crate::http::parse::parse_gamma_market(&market).expect("parse gamma market");
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], clock.get_time_ns())
                .expect("create instrument");

        assert!(
            crate::filters::is_expired(&instrument, clock.get_time_ns()),
            "fixture must be past its endDate for this test to mean anything",
        );
        assert_eq!(
            crate::filters::venue_reports_closed(&instrument),
            Some(false)
        );
        assert!(
            !is_instrument_retirable(&instrument, clock.get_time_ns()),
            "a market the venue still trades must not be retired on endDate alone",
        );
    }

    /// Builds an expired instrument with an explicit venue state and a chosen token/condition.
    fn expired_instrument_with_state(
        raw_symbol: &str,
        condition_id: &str,
        venue_closed: Option<bool>,
    ) -> InstrumentAny {
        let inst = seed_expired_instrument(raw_symbol, condition_id);
        let InstrumentAny::BinaryOption(mut binary) = inst else {
            panic!("expected BinaryOption");
        };

        let mut info = binary
            .info
            .clone()
            .unwrap_or_else(nautilus_core::Params::new);

        if let Some(closed) = venue_closed {
            info.insert("venue_closed".to_string(), serde_json::Value::Bool(closed));
        }
        binary.info = Some(info);

        InstrumentAny::BinaryOption(binary)
    }

    #[rstest]
    #[tokio::test]
    async fn sweep_emits_ws_unsubscribe_only_when_venue_reports_closed() {
        // Evidences the WebSocket teardown directly: the retirement path emits no log at any
        // level, so the command channel is the observable.
        for (venue_closed, expect_unsubscribe) in [(Some(false), false), (Some(true), true)] {
            let instruments = Arc::new(AtomicMap::new());
            let token_meta = Arc::new(DashMap::new());
            let order_books = Arc::new(DashMap::new());
            let last_quotes = Arc::new(DashMap::new());
            let active_quote_subs = Arc::new(AtomicSet::new());
            let active_delta_subs = Arc::new(AtomicSet::new());
            let active_trade_subs = Arc::new(AtomicSet::new());
            let resolve_poll_watchlist = Arc::new(AtomicMap::new());
            let pending_snapshot_after_tick_change = Arc::new(AtomicSet::new());
            let pending_auto_loads = Arc::new(StdMutex::new(AHashSet::new()));
            let ws_open_tokens = Arc::new(AtomicSet::new());
            let ws_sub_mutex = Arc::new(tokio::sync::Mutex::new(()));
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
            let ws = PolymarketMarketPoolHandle::test_single_shard(tx, &["0xTOKEN_AB"]);

            let inst = expired_instrument_with_state("0xTOKEN_AB", "0xCOND-AB", venue_closed);
            let instrument_id = inst.id();
            seed_cached_instrument(&instruments, &token_meta, &inst);
            seed_runtime_state(
                &inst,
                &order_books,
                &last_quotes,
                &active_quote_subs,
                &active_delta_subs,
                &active_trade_subs,
                &pending_snapshot_after_tick_change,
                &pending_auto_loads,
                &ws_open_tokens,
            );

            retire_expired_local_instruments(
                get_atomic_clock_realtime().get_time_ns(),
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
            )
            .await;

            if expect_unsubscribe {
                match rx
                    .try_recv()
                    .expect("venue-closed market should unsubscribe")
                {
                    HandlerCommand::UnsubscribeMarket(ids) => {
                        assert_eq!(ids, vec!["0xTOKEN_AB".to_string()]);
                    }
                    other => panic!("unexpected WS command: {other:?}"),
                }
                assert!(!instruments.load().contains_key(&instrument_id));
            } else {
                assert!(
                    rx.try_recv().is_err(),
                    "market open at the venue must not be unsubscribed",
                );
                assert!(instruments.load().contains_key(&instrument_id));
                assert!(token_meta.contains_key(&Ustr::from("0xTOKEN_AB")));
            }
        }
    }

    /// A watchlisted instrument keeps its cache entry through retirement so settlement can still
    /// read it. A later publish of the same venue-closed definition must correct that entry without
    /// restoring runtime state, or every refresh would re-arm the sweep for a market the venue has
    /// already closed and message routing would come back with it.
    #[rstest]
    #[tokio::test]
    async fn superseding_a_watchlisted_instrument_does_not_restore_runtime_state() {
        let instruments = Arc::new(AtomicMap::new());
        let token_meta = Arc::new(DashMap::new());
        let order_books = Arc::new(DashMap::new());
        let last_quotes = Arc::new(DashMap::new());
        let active_quote_subs = Arc::new(AtomicSet::new());
        let active_delta_subs = Arc::new(AtomicSet::new());
        let active_trade_subs = Arc::new(AtomicSet::new());
        let resolve_poll_watchlist = Arc::new(AtomicMap::new());
        let pending_snapshot_after_tick_change = Arc::new(AtomicSet::new());
        let pending_auto_loads = Arc::new(StdMutex::new(AHashSet::new()));
        let ws_open_tokens = Arc::new(AtomicSet::new());
        let ws_sub_mutex = Arc::new(tokio::sync::Mutex::new(()));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let ws = PolymarketMarketPoolHandle::test_single_shard(tx, &["0xTOKEN_HELD"]);
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel();

        let inst = expired_instrument_with_state("0xTOKEN_HELD", "0xCOND-HELD", Some(true));
        let instrument_id = inst.id();
        let token = Ustr::from("0xTOKEN_HELD");
        seed_cached_instrument(&instruments, &token_meta, &inst);
        seed_runtime_state(
            &inst,
            &order_books,
            &last_quotes,
            &active_quote_subs,
            &active_delta_subs,
            &active_trade_subs,
            &pending_snapshot_after_tick_change,
            &pending_auto_loads,
            &ws_open_tokens,
        );

        // An open position pins the instrument, so retirement keeps the cache entry.
        resolve_poll_watchlist.insert(
            "0xCOND-HELD".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND-HELD".to_string(),
                expiration_ns: inst.expiration_ns().unwrap_or_default(),
                tracked: ahash::AHashMap::from_iter([(
                    "0xTOKEN_HELD".to_string(),
                    crate::resolve::TrackedInstrument {
                        instrument_id,
                        token_id: "0xTOKEN_HELD".to_string(),
                        price_precision: inst.price_precision(),
                        open_position_ids: AHashSet::new(),
                    },
                )]),
                paused: false,
            },
        );

        let sweep = async || {
            retire_expired_local_instruments(
                get_atomic_clock_realtime().get_time_ns(),
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
            )
            .await;
        };

        sweep().await;

        assert!(
            matches!(rx.try_recv(), Ok(HandlerCommand::UnsubscribeMarket(_))),
            "the first sweep retires the closed market",
        );
        assert!(
            instruments.load().contains_key(&instrument_id),
            "a watchlisted instrument keeps its cache entry for settlement",
        );
        assert!(!token_meta.contains_key(&token));

        // The same venue-closed definition arrives again from a refresh.
        crate::data::instruments::cache_and_publish_instruments(
            &instruments,
            &token_meta,
            &data_tx,
            get_atomic_clock_realtime().get_time_ns(),
            vec![inst.clone()],
        );

        assert!(
            !token_meta.contains_key(&token),
            "superseding must not restore routing for a market the venue has closed",
        );

        sweep().await;

        assert!(
            rx.try_recv().is_err(),
            "an already retired instrument must not be torn down again",
        );
    }

    #[rstest]
    fn reconnect_reseeds_token_meta_only_for_markets_open_at_the_venue() {
        // Evidences the reconnect path: seeding is what restores WS message routing.
        let instruments = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());

        let open = expired_instrument_with_state("0xTOKEN_OPEN", "0xCOND-OPEN", Some(false));
        let closed = expired_instrument_with_state("0xTOKEN_CLOSED", "0xCOND-CLOSED", Some(true));
        let unknown = expired_instrument_with_state("0xTOKEN_UNKNOWN", "0xCOND-UNKNOWN", None);
        for inst in [&open, &closed, &unknown] {
            instruments.insert(inst.id(), inst.clone());
        }

        seed_token_meta_from_live_instruments(
            get_atomic_clock_realtime().get_time_ns(),
            &instruments,
            &token_meta,
        );

        assert!(token_meta.contains_key(&Ustr::from("0xTOKEN_OPEN")));
        assert!(!token_meta.contains_key(&Ustr::from("0xTOKEN_CLOSED")));
        assert!(!token_meta.contains_key(&Ustr::from("0xTOKEN_UNKNOWN")));
    }
}
