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

use crate::{
    data::{
        DataTokenMeta as TokenMeta, data_resolve_token_id_from as resolve_token_id_from,
        data_sync_ws_subscription_async as sync_ws_subscription_async,
    },
    resolve::ResolveWatchEntry,
};

pub(crate) fn is_instrument_expired(instrument: &InstrumentAny, now_ns: UnixNanos) -> bool {
    instrument
        .expiration_ns()
        .is_some_and(|expiration_ns| expiration_ns.as_u64() != 0 && expiration_ns <= now_ns)
}

pub(crate) fn seed_token_meta_from_live_instruments(
    now_ns: UnixNanos,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
) {
    let loaded = instruments.load();

    for instrument in loaded.values() {
        if is_instrument_expired(instrument, now_ns) {
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
    ws: &crate::websocket::client::WsSubscriptionHandle,
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
    ws: &crate::websocket::client::WsSubscriptionHandle,
) {
    let expired_candidates: Vec<(InstrumentId, String)> = {
        let loaded = instruments.load();
        loaded
            .iter()
            .filter_map(|(instrument_id, instrument)| {
                is_instrument_expired(instrument, now_ns)
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
        enums::{AssetClass, BookType},
        identifiers::{InstrumentId, PositionId, Symbol},
        instruments::{BinaryOption, Instrument},
        orderbook::OrderBook,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        resolve::upsert_resolve_watch_entry_from_instrument,
        websocket::{client::WsSubscriptionHandle, handler::HandlerCommand},
    };

    fn seed_expired_instrument(raw_symbol: &str, condition_id: &str) -> InstrumentAny {
        let clock = get_atomic_clock_realtime();
        let mut inst = InstrumentAny::BinaryOption(BinaryOption::new(
            InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str()),
            Symbol::new(raw_symbol),
            AssetClass::Alternative,
            Currency::pUSD(),
            UnixNanos::default(),
            clock.get_time_ns(),
            3,
            2,
            Price::from("0.001"),
            Quantity::from("0.01"),
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
        ));

        if let InstrumentAny::BinaryOption(ref mut binary) = inst {
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
        }

        inst
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
        let ws = WsSubscriptionHandle::from_sender(tx);

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
}
