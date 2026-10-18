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

//! Runtime state coordination for the Polymarket data client.

use std::sync::Arc;

use ahash::AHashSet;
use nautilus_core::{AtomicMap, UnixNanos};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    instruments::TokenMeta,
    subscriptions::{DataSubscriptions, resolve_token_id_from},
};
use crate::{providers::extract_condition_id, resolve::ResolveWatchEntry};

impl DataSubscriptions {
    pub(crate) async fn mark_closed(
        &self,
        condition_id: &str,
        cancellation: Option<&CancellationToken>,
    ) -> bool {
        let _reconcile_guard = if let Some(cancellation) = cancellation {
            tokio::select! {
                guard = self.ws_sub_mutex.lock() => guard,
                () = cancellation.cancelled() => return false,
            }
        } else {
            self.ws_sub_mutex.lock().await
        };

        let newly_closed = {
            let mut terminal_conditions = self.closed_condition_ids.lock();

            if cancellation.is_some_and(CancellationToken::is_cancelled) {
                return false;
            }

            terminal_conditions.insert(condition_id.to_string())
        };

        if newly_closed {
            log::info!("Market closed for condition {condition_id}, retiring live data state");
        }

        true
    }

    pub(super) fn seed_token_meta(&self, now_ns: UnixNanos) {
        let loaded = self.instruments.load();

        for instrument in loaded.values() {
            if is_instrument_expired_and_not_reported_open(instrument, now_ns) {
                continue;
            }

            let terminal_conditions = self.closed_condition_ids.lock();

            if extract_condition_id(&instrument.id())
                .is_ok_and(|condition_id| terminal_conditions.contains(&condition_id))
            {
                continue;
            }

            self.token_meta.insert(
                Ustr::from(instrument.raw_symbol().as_str()),
                TokenMeta::from_instrument(instrument),
            );
        }
    }

    fn has_live_runtime_state(&self, instrument_id: InstrumentId, token_id: Option<&str>) -> bool {
        if self.active_quote_subs.contains(&instrument_id)
            || self.active_delta_subs.contains(&instrument_id)
            || self.active_trade_subs.contains(&instrument_id)
            || self.book_sync.book_gated(instrument_id)
            || self.last_quotes.contains_key(&instrument_id)
        {
            return true;
        }

        if self.pending_auto_loads.lock().contains(&instrument_id) {
            return true;
        }

        let Some(token_id) = token_id else {
            return false;
        };

        let token_id = Ustr::from(token_id);
        let resolution_owned = self.active_instrument_status_subs.contains(&instrument_id)
            || self.active_instrument_close_subs.contains(&instrument_id);
        self.token_meta.contains_key(&token_id)
            || (self.ws_open_tokens.contains(&token_id) && !resolution_owned)
    }

    pub(super) async fn retire_instrument(&self, instrument_id: InstrumentId) {
        let token_id = resolve_token_id_from(&self.instruments, instrument_id)
            .or_else(|_| crate::providers::extract_token_id(&instrument_id))
            .ok();

        self.active_quote_subs.remove(&instrument_id);
        self.active_delta_subs.remove(&instrument_id);
        self.active_trade_subs.remove(&instrument_id);
        if let Some(token_id) = token_id.as_ref() {
            self.reconcile(instrument_id, token_id.clone()).await;
        }

        self.book_sync.remove(instrument_id);
        if is_watchlisted_instrument(&self.watchlist, instrument_id)
            || (!self.active_instrument_status_subs.contains(&instrument_id)
                && !self.active_instrument_close_subs.contains(&instrument_id))
        {
            let mut pending = self.pending_auto_loads.lock();
            pending.remove(&instrument_id);
        }

        self.order_books.remove(&instrument_id);
        self.last_quotes.remove(&instrument_id);

        if let Some(token_id) = token_id {
            self.token_meta.remove(&Ustr::from(token_id.as_str()));
        } else {
            // Without a cache entry the token id is unresolvable, so match on the routing value
            self.token_meta
                .retain(|_, meta| meta.instrument_id != instrument_id);
        }

        let keep_local_metadata = is_watchlisted_instrument(&self.watchlist, instrument_id);
        if !keep_local_metadata {
            self.instruments.remove(&instrument_id);
        }
    }

    pub(super) async fn retire_condition(
        &self,
        condition_id: &str,
        seed_ids: impl IntoIterator<Item = InstrumentId>,
        cancellation: Option<&CancellationToken>,
    ) -> bool {
        if cancellation.is_some_and(CancellationToken::is_cancelled) {
            return false;
        }

        // The terminal marker must be visible before state is inspected or any asynchronous
        // WebSocket reconciliation begins.
        if !self.mark_closed(condition_id, cancellation).await {
            return false;
        }

        let matches_condition = |instrument_id: &InstrumentId| {
            extract_condition_id(instrument_id).is_ok_and(|candidate| candidate == condition_id)
        };

        let collect_live_ids = || {
            let mut ids = AHashSet::<InstrumentId>::new();

            ids.extend(
                self.instruments
                    .load()
                    .keys()
                    .filter(|id| matches_condition(id))
                    .copied(),
            );

            for subscriptions in [
                &self.active_quote_subs,
                &self.active_delta_subs,
                &self.active_trade_subs,
            ] {
                ids.extend(
                    subscriptions
                        .load()
                        .iter()
                        .filter(|id| matches_condition(id))
                        .copied(),
                );
            }

            ids.extend(
                self.pending_auto_loads
                    .lock()
                    .iter()
                    .filter(|id| matches_condition(id))
                    .copied(),
            );
            ids.extend(
                self.token_meta
                    .iter()
                    .map(|entry| entry.value().instrument_id)
                    .filter(matches_condition),
            );
            ids.extend(
                self.order_books
                    .iter()
                    .map(|entry| *entry.key())
                    .filter(matches_condition),
            );
            ids.extend(
                self.last_quotes
                    .iter()
                    .map(|entry| *entry.key())
                    .filter(matches_condition),
            );

            ids
        };

        let mut retiring_ids = seed_ids
            .into_iter()
            .filter(matches_condition)
            .collect::<AHashSet<_>>();

        retiring_ids.extend(collect_live_ids());

        for instrument_id in retiring_ids {
            let retirement = self.retire_instrument(instrument_id);

            if let Some(cancellation) = cancellation {
                tokio::select! {
                    () = cancellation.cancelled() => return false,
                    () = retirement => {}
                }
            } else {
                retirement.await;
            }
        }

        // State can reappear after the scan above, so report whether anything is left
        collect_live_ids().is_empty()
    }

    pub(super) async fn retire_expired(&self, now_ns: UnixNanos) {
        let expired_candidates: Vec<(InstrumentId, String)> = {
            let loaded = self.instruments.load();
            loaded
                .iter()
                .filter_map(|(instrument_id, instrument)| {
                    is_instrument_expired_and_not_reported_open(instrument, now_ns)
                        .then_some((*instrument_id, instrument.raw_symbol().as_str().to_string()))
                })
                .collect()
        };

        let mut expired_ids = Vec::new();

        for (instrument_id, token_id) in expired_candidates {
            let keep_local_metadata = is_watchlisted_instrument(&self.watchlist, instrument_id);
            if keep_local_metadata
                && !self.has_live_runtime_state(instrument_id, Some(token_id.as_str()))
            {
                continue;
            }

            expired_ids.push(instrument_id);
        }

        if !expired_ids.is_empty() {
            log::info!(
                "Removing live state for {} closed Polymarket instrument(s)",
                expired_ids.len()
            );
        }

        for instrument_id in expired_ids {
            {
                let _guard = self.owner_lock.lock();

                if !is_watchlisted_instrument(&self.watchlist, instrument_id)
                    && !self.deferred_resolutions.contains_key(&instrument_id)
                {
                    self.active_instrument_status_subs.remove(&instrument_id);
                    self.active_instrument_close_subs.remove(&instrument_id);
                }
            }

            self.retire_instrument(instrument_id).await;
        }
    }
}

pub(crate) fn is_condition_closed(
    closed_condition_ids: &Arc<Mutex<AHashSet<String>>>,
    condition_id: &str,
) -> bool {
    closed_condition_ids.lock().contains(condition_id)
}

pub(crate) fn is_instrument_expired_and_not_reported_open(
    instrument: &InstrumentAny,
    now_ns: UnixNanos,
) -> bool {
    crate::filters::is_expired_and_not_reported_open(instrument, now_ns)
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ahash::AHashSet;
    use dashmap::DashMap;
    use log::{Level, LevelFilter, Log, Metadata, Record};
    use nautilus_common::live::dst::time::Instant;
    use nautilus_core::{AtomicMap, AtomicSet, UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::{
        data::QuoteTick,
        enums::BookType,
        identifiers::{InstrumentId, PositionId, Symbol},
        instruments::{Instrument, stubs::binary_option},
        orderbook::OrderBook,
        types::{Currency, Price, Quantity},
    };
    use parking_lot::Mutex;
    use rstest::rstest;

    use super::*;
    use crate::{
        book::sync::BookSyncTracker,
        resolve::{
            upsert_data_resolve_watch_entry_from_instrument,
            upsert_resolve_watch_entry_from_instrument,
        },
        websocket::{handler::HandlerCommand, pool::PolymarketMarketPoolHandle},
    };

    struct ClosureLogCapture {
        messages: Mutex<Vec<String>>,
    }

    static CLOSURE_LOG_CAPTURE: ClosureLogCapture = ClosureLogCapture {
        messages: Mutex::new(Vec::new()),
    };

    impl Log for ClosureLogCapture {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() == Level::Info
                && metadata.target() == "nautilus_polymarket::data::runtime"
        }

        fn log(&self, record: &Record<'_>) {
            if self.enabled(record.metadata()) {
                self.messages.lock().push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    #[rstest]
    #[tokio::test]
    async fn register_closed_condition_logs_once_per_condition() {
        log::set_logger(&CLOSURE_LOG_CAPTURE).expect("test logger already installed");
        log::set_max_level(LevelFilter::Info);

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let ws_client = PolymarketMarketPoolHandle::test_single_shard(tx, &[]);

        let subscriptions = DataSubscriptions::new(
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(Mutex::new(())),
            ws_client,
            false,
        );

        for condition_id in ["0xCOND-A", "0xCOND-A", "0xCOND-B"] {
            assert!(subscriptions.mark_closed(condition_id, None).await);
        }

        assert_eq!(
            *CLOSURE_LOG_CAPTURE.messages.lock(),
            vec![
                "Market closed for condition 0xCOND-A, retiring live data state".to_string(),
                "Market closed for condition 0xCOND-B, retiring live data state".to_string(),
            ],
        );
        assert_eq!(
            *subscriptions.closed_condition_ids.lock(),
            AHashSet::from_iter(["0xCOND-A".to_string(), "0xCOND-B".to_string()]),
        );
    }

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
            TokenMeta::from_instrument(instrument),
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
        book_sync: &BookSyncTracker,
        pending_auto_loads: &Arc<Mutex<AHashSet<InstrumentId>>>,
        ws_open_tokens: &Arc<AtomicSet<Ustr>>,
    ) {
        let instrument_id = instrument.id();

        active_quote_subs.insert(instrument_id);
        active_delta_subs.insert(instrument_id);
        active_trade_subs.insert(instrument_id);
        book_sync.request_recovery(instrument_id, Instant::now());
        pending_auto_loads.lock().insert(instrument_id);
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
    fn active_delta_subscription_is_live_without_other_runtime_state() {
        let instrument_id = InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET");
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let ws_client = PolymarketMarketPoolHandle::test_single_shard(tx, &[]);

        let subscriptions = DataSubscriptions::new(
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(Mutex::new(())),
            ws_client,
            false,
        );

        subscriptions.active_delta_subs.insert(instrument_id);

        assert!(subscriptions.has_live_runtime_state(instrument_id, None));

        subscriptions.active_delta_subs.remove(&instrument_id);

        assert!(!subscriptions.has_live_runtime_state(instrument_id, None));
    }

    #[rstest]
    #[tokio::test]
    async fn retire_instrument_removes_orphaned_token_meta() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let ws_client = PolymarketMarketPoolHandle::test_single_shard(tx, &["0xTOKEN_ORPHAN"]);

        let subscriptions = DataSubscriptions::new(
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(Mutex::new(())),
            ws_client,
            false,
        );

        // A handler can re-insert routing after retirement dropped the cache entry
        let inst = seed_expired_instrument("0xTOKEN_ORPHAN", "0xCOND-ORPHAN");
        let instrument_id = inst.id();
        let token_id = Ustr::from(inst.raw_symbol().as_str());
        subscriptions
            .token_meta
            .insert(token_id, TokenMeta::from_instrument(&inst));

        assert!(
            !subscriptions
                .instruments
                .load()
                .contains_key(&instrument_id)
        );

        subscriptions.retire_instrument(instrument_id).await;

        assert!(!subscriptions.token_meta.contains_key(&token_id));
        assert!(subscriptions.token_meta.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn retire_expired_retires_watchlisted_runtime_state_once() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let ws_client = PolymarketMarketPoolHandle::test_single_shard(tx, &["0xTOKEN_WATCHED"]);

        let subscriptions = DataSubscriptions::new(
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(Mutex::new(())),
            ws_client,
            false,
        );

        let inst = seed_expired_instrument("0xTOKEN_WATCHED", "0xCOND-WATCHED");
        let instrument_id = inst.id();
        let token_id = Ustr::from(inst.raw_symbol().as_str());
        seed_cached_instrument(&subscriptions.instruments, &subscriptions.token_meta, &inst);
        upsert_resolve_watch_entry_from_instrument(
            &subscriptions.watchlist,
            &inst,
            PositionId::new("P-1"),
        );
        seed_runtime_state(
            &inst,
            &subscriptions.order_books,
            &subscriptions.last_quotes,
            &subscriptions.active_quote_subs,
            &subscriptions.active_delta_subs,
            &subscriptions.active_trade_subs,
            &subscriptions.book_sync,
            &subscriptions.pending_auto_loads,
            &subscriptions.ws_open_tokens,
        );

        let now_ns = get_atomic_clock_realtime().get_time_ns();
        subscriptions.retire_expired(now_ns).await;

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
        assert!(
            subscriptions
                .instruments
                .load()
                .contains_key(&instrument_id)
        );
        assert!(!subscriptions.token_meta.contains_key(&token_id));

        subscriptions.retire_expired(now_ns).await;

        assert!(
            rx.try_recv().is_err(),
            "watchlisted expired instruments should retire live runtime state only once",
        );
        assert!(
            subscriptions
                .instruments
                .load()
                .contains_key(&instrument_id)
        );
        assert!(!subscriptions.token_meta.contains_key(&token_id));
        assert!(!subscriptions.order_books.contains_key(&instrument_id));
        assert!(!subscriptions.last_quotes.contains_key(&instrument_id));
        assert!(!subscriptions.active_quote_subs.contains(&instrument_id));
        assert!(!subscriptions.active_delta_subs.contains(&instrument_id));
        assert!(!subscriptions.active_trade_subs.contains(&instrument_id));
        assert!(!subscriptions.book_sync.book_gated(instrument_id));
        assert!(subscriptions.pending_auto_loads.lock().is_empty());
        assert!(!subscriptions.ws_open_tokens.contains(&token_id));
    }

    #[rstest]
    #[tokio::test]
    async fn retire_expired_watchlisted_resolution_subscription_keeps_ws_owner() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let ws_client = PolymarketMarketPoolHandle::test_single_shard(tx, &["0xTOKEN_WATCHED"]);

        let mut subscriptions = DataSubscriptions::new(
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(Mutex::new(())),
            ws_client,
            false,
        );
        subscriptions.subscribe_new_markets = true;
        let instrument = seed_expired_instrument("0xTOKEN_WATCHED", "0xCOND-WATCHED");
        let instrument_id = instrument.id();
        let token_id = Ustr::from(instrument.raw_symbol().as_str());
        seed_cached_instrument(
            &subscriptions.instruments,
            &subscriptions.token_meta,
            &instrument,
        );
        assert!(upsert_data_resolve_watch_entry_from_instrument(
            &subscriptions.watchlist,
            &instrument,
        ));
        subscriptions
            .active_instrument_status_subs
            .insert(instrument_id);
        subscriptions.ws_open_tokens.insert(token_id);

        subscriptions
            .retire_expired(get_atomic_clock_realtime().get_time_ns())
            .await;

        assert!(
            subscriptions
                .active_instrument_status_subs
                .contains(&instrument_id)
        );
        assert!(subscriptions.ws_open_tokens.contains(&token_id));
        assert!(rx.try_recv().is_err());
        assert!(
            !subscriptions.has_live_runtime_state(instrument_id, Some(token_id.as_str())),
            "resolution-only WS ownership must not schedule another no-op retirement",
        );
    }
}
