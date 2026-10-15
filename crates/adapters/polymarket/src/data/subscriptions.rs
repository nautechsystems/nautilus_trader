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

use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use nautilus_common::cache::InstrumentLookupError;
use nautilus_core::{AtomicMap, AtomicSet};
use nautilus_model::{
    data::QuoteTick,
    enums::BookType,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use parking_lot::Mutex;
use ustr::Ustr;

use super::instruments::TokenMeta;
use crate::{
    book::sync::BookSyncTracker,
    resolve::{
        ResolveWatchEntry, StrictResolvedMarket, remove_data_resolve_watch_entry,
        upsert_data_resolve_watch_entry_from_instrument,
    },
    websocket::pool::PolymarketMarketPoolHandle,
};

#[derive(Clone, Debug)]
pub(crate) struct DataSubscriptions {
    pub(super) instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    pub(super) watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    pub(super) deferred_resolutions: Arc<AtomicMap<InstrumentId, StrictResolvedMarket>>,
    pub(super) owner_lock: Arc<Mutex<()>>,
    pub(super) ws_client: PolymarketMarketPoolHandle,
    pub(super) subscribe_new_markets: bool,
    pub(super) token_meta: Arc<DashMap<Ustr, TokenMeta>>,
    pub(super) order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    pub(super) last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    pub(super) active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) active_instrument_status_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) active_instrument_close_subs: Arc<AtomicSet<InstrumentId>>,
    pub(super) book_sync: BookSyncTracker,
    pub(super) ws_open_tokens: Arc<AtomicSet<Ustr>>,
    pub(super) ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    pub(super) pending_auto_loads: Arc<Mutex<AHashSet<InstrumentId>>>,
    pub(super) closed_condition_ids: Arc<Mutex<AHashSet<String>>>,
}

impl DataSubscriptions {
    pub(crate) fn new(
        instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
        deferred_resolutions: Arc<AtomicMap<InstrumentId, StrictResolvedMarket>>,
        owner_lock: Arc<Mutex<()>>,
        ws_client: PolymarketMarketPoolHandle,
        subscribe_new_markets: bool,
    ) -> Self {
        Self {
            instruments,
            watchlist,
            deferred_resolutions,
            owner_lock,
            ws_client,
            subscribe_new_markets,
            token_meta: Arc::new(DashMap::new()),
            order_books: Arc::new(DashMap::new()),
            last_quotes: Arc::new(DashMap::new()),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_delta_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            active_instrument_status_subs: Arc::new(AtomicSet::new()),
            active_instrument_close_subs: Arc::new(AtomicSet::new()),
            book_sync: BookSyncTracker::default(),
            ws_open_tokens: Arc::new(AtomicSet::new()),
            ws_sub_mutex: Arc::new(tokio::sync::Mutex::new(())),
            pending_auto_loads: Arc::new(Mutex::new(AHashSet::new())),
            closed_condition_ids: Arc::new(Mutex::new(AHashSet::new())),
        }
    }

    pub(super) fn add_live_subscription_intent(
        &self,
        instrument_id: InstrumentId,
        kind: SubscriptionKind,
    ) -> bool {
        self.add_live_subscription_intent_with_state(instrument_id, kind, || {})
    }

    pub(super) fn add_delta_subscription_intent(
        &self,
        instrument_id: InstrumentId,
        compute_effective_deltas: bool,
    ) -> bool {
        self.add_live_subscription_intent_with_state(
            instrument_id,
            SubscriptionKind::Deltas,
            || {
                if compute_effective_deltas {
                    self.order_books
                        .entry(instrument_id)
                        .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));
                }
            },
        )
    }

    pub(super) fn add_resolution_subscription_intent(
        &self,
        instrument_id: InstrumentId,
        kind: SubscriptionKind,
    ) -> bool {
        let subscriptions = self.intent(kind);
        let _guard = self.owner_lock.lock();

        if !self.add_live_subscription_intent(instrument_id, kind) {
            return false;
        }

        if let Some(instrument) = self.instruments.load().get(&instrument_id)
            && !upsert_data_resolve_watch_entry_from_instrument(&self.watchlist, instrument)
        {
            subscriptions.remove(&instrument_id);
            log::warn!(
                "Ignoring Polymarket resolution subscription for unsupported cached instrument {instrument_id}"
            );
            return false;
        }

        true
    }

    pub(super) fn unsubscribe(
        &self,
        instrument_id: InstrumentId,
        kind: SubscriptionKind,
    ) -> Option<String> {
        let token_id = match kind {
            SubscriptionKind::Status | SubscriptionKind::Close => {
                self.remove_resolution_subscription_intent(instrument_id, kind)
            }
            _ => {
                self.intent(kind).remove(&instrument_id);
                resolve_token_id_from(&self.instruments, instrument_id).ok()
            }
        };

        if kind == SubscriptionKind::Deltas {
            self.book_sync.remove(instrument_id);
        }

        self.drop_pending_if_unwanted(instrument_id);
        if matches!(kind, SubscriptionKind::Deltas | SubscriptionKind::Quotes) {
            self.drop_local_data_state_if_unwanted(instrument_id);
        }

        token_id
    }

    pub(super) fn has_intent(&self, instrument_id: InstrumentId) -> bool {
        self.intents()
            .iter()
            .any(|set| set.contains(&instrument_id))
    }

    pub(crate) fn status_subscriptions(&self) -> &Arc<AtomicSet<InstrumentId>> {
        &self.active_instrument_status_subs
    }

    pub(crate) fn close_subscriptions(&self) -> &Arc<AtomicSet<InstrumentId>> {
        &self.active_instrument_close_subs
    }

    pub(crate) fn is_condition_closed(&self, condition_id: &str) -> bool {
        self.closed_condition_ids.lock().contains(condition_id)
    }

    pub(crate) fn is_token_open(&self, token_id: &str) -> bool {
        self.ws_open_tokens.contains(&Ustr::from(token_id))
    }

    pub(crate) async fn lock_reconciliation(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.ws_sub_mutex.lock().await
    }

    pub(crate) fn extend_reconciliation_targets(
        &self,
        condition_id: &str,
        instruments: &AHashMap<InstrumentId, InstrumentAny>,
        targets: &mut AHashMap<InstrumentId, String>,
    ) {
        for subscriptions in self.intents() {
            for instrument_id in subscriptions.load().iter().copied() {
                if !crate::providers::extract_condition_id(&instrument_id)
                    .is_ok_and(|candidate| candidate == condition_id)
                {
                    continue;
                }

                let token_id = instruments
                    .get(&instrument_id)
                    .map(|instrument| instrument.raw_symbol().as_str().to_string())
                    .or_else(|| crate::providers::extract_token_id(&instrument_id).ok());
                if let Some(token_id) = token_id {
                    targets.entry(instrument_id).or_insert(token_id);
                }
            }
        }
    }

    // Strict resolution holds the reconciliation and resolution locks before releasing intents
    pub(crate) fn finish_resolution(
        &self,
        condition_id: &str,
        tracked_ids: &AHashSet<InstrumentId>,
        deferred_ids: &AHashSet<InstrumentId>,
    ) -> bool {
        let matches_condition = |id: &InstrumentId| {
            crate::providers::extract_condition_id(id)
                .is_ok_and(|candidate| candidate == condition_id)
        };

        for subscriptions in [
            &self.active_instrument_status_subs,
            &self.active_instrument_close_subs,
        ] {
            subscriptions.rcu(|entries| {
                entries.retain(|id| {
                    deferred_ids.contains(id)
                        || (!matches_condition(id) && !tracked_ids.contains(id))
                });
            });
        }

        for subscriptions in [
            &self.active_quote_subs,
            &self.active_delta_subs,
            &self.active_trade_subs,
        ] {
            subscriptions.rcu(|entries| {
                entries.retain(|id| !matches_condition(id) && !tracked_ids.contains(id));
            });
        }

        self.closed_condition_ids
            .lock()
            .insert(condition_id.to_string())
    }

    // Hold the mutex across wire sends so subscription commands arrive in last-writer order
    pub(crate) async fn reconcile(&self, instrument_id: InstrumentId, token_id_str: String) {
        let token_id = Ustr::from(token_id_str.as_str());
        let _guard = self.ws_sub_mutex.lock().await;

        let condition_id = crate::providers::extract_condition_id(&instrument_id).ok();
        let is_terminal = condition_id
            .as_ref()
            .is_some_and(|condition_id| self.is_condition_closed(condition_id));

        // Only the enabled venue feed can carry resolution events, and a paused
        // watch retains manual recovery ownership rather than a wire subscription.
        let wants_resolution = self.subscribe_new_markets
            && (self.active_instrument_status_subs.contains(&instrument_id)
                || self.active_instrument_close_subs.contains(&instrument_id))
            && {
                let watchlist = self.watchlist.load();

                let has_active_data_owner = |entry: &ResolveWatchEntry| {
                    !entry.paused
                        && entry.tracked.values().any(|tracked| {
                            tracked.instrument_id == instrument_id && tracked.has_data_subscription
                        })
                };

                // Canonical IDs locate their watch directly, legacy IDs still use watch metadata
                condition_id
                    .as_ref()
                    .and_then(|condition_id| watchlist.get(condition_id))
                    .is_some_and(has_active_data_owner)
                    || watchlist.values().any(has_active_data_owner)
            };

        let wants_subscribe = wants_resolution
            || (!is_terminal
                && (self.active_quote_subs.contains(&instrument_id)
                    || self.active_delta_subs.contains(&instrument_id)
                    || self.active_trade_subs.contains(&instrument_id)));
        let is_open = self.ws_open_tokens.contains(&token_id);
        if wants_subscribe && !is_open {
            self.ws_open_tokens.insert(token_id);

            if let Err(e) = self.ws_client.subscribe_market(vec![token_id_str]).await {
                log::error!("Failed to subscribe to market data: {e:?}");
                // Roll back tracked WS state so a retry can take effect.
                self.ws_open_tokens.remove(&token_id);
            }
        } else if !wants_subscribe && is_open {
            self.ws_open_tokens.remove(&token_id);

            if let Err(e) = self.ws_client.unsubscribe_market(vec![token_id_str]).await {
                log::error!("Failed to unsubscribe from market data: {e:?}");
            }
        }
    }

    fn intent(&self, kind: SubscriptionKind) -> &Arc<AtomicSet<InstrumentId>> {
        match kind {
            SubscriptionKind::Deltas => &self.active_delta_subs,
            SubscriptionKind::Quotes => &self.active_quote_subs,
            SubscriptionKind::Trades => &self.active_trade_subs,
            SubscriptionKind::Status => &self.active_instrument_status_subs,
            SubscriptionKind::Close => &self.active_instrument_close_subs,
        }
    }

    fn drop_pending_if_unwanted(&self, instrument_id: InstrumentId) {
        if self.has_intent(instrument_id) {
            return;
        }

        let mut pending = self.pending_auto_loads.lock();
        pending.remove(&instrument_id);
    }

    fn drop_local_data_state_if_unwanted(&self, instrument_id: InstrumentId) {
        // Stale book/quote leaks across resubscribes
        if !self.active_delta_subs.contains(&instrument_id) {
            self.order_books.remove(&instrument_id);
        }

        if !self.active_quote_subs.contains(&instrument_id) {
            self.last_quotes.remove(&instrument_id);
        }
    }

    fn remove_resolution_subscription_intent(
        &self,
        instrument_id: InstrumentId,
        kind: SubscriptionKind,
    ) -> Option<String> {
        let subscriptions = self.intent(kind);
        let _guard = self.owner_lock.lock();

        let token_id = resolve_token_id_from(&self.instruments, instrument_id)
            .ok()
            .or_else(|| {
                self.watchlist
                    .load()
                    .values()
                    .flat_map(|entry| entry.tracked.values())
                    .find(|tracked| tracked.instrument_id == instrument_id)
                    .map(|tracked| tracked.token_id.clone())
            });

        subscriptions.remove(&instrument_id);
        let has_data_subscription = self.active_instrument_status_subs.contains(&instrument_id)
            || self.active_instrument_close_subs.contains(&instrument_id);
        if !has_data_subscription {
            self.deferred_resolutions.remove(&instrument_id);
        }

        remove_data_resolve_watch_entry(&self.watchlist, instrument_id, has_data_subscription);
        token_id
    }

    fn add_live_subscription_intent_with_state(
        &self,
        instrument_id: InstrumentId,
        kind: SubscriptionKind,
        initialize_state: impl FnOnce(),
    ) -> bool {
        let subscriptions = self.intent(kind);

        let Ok(condition_id) = crate::providers::extract_condition_id(&instrument_id) else {
            subscriptions.insert(instrument_id);
            initialize_state();
            return true;
        };

        let closed = self.closed_condition_ids.lock();

        if closed.contains(&condition_id) {
            log::debug!(
                "Ignoring live subscription for terminally closed Polymarket condition {condition_id}"
            );
            return false;
        }

        subscriptions.insert(instrument_id);
        initialize_state();
        true
    }

    pub(super) fn intents(&self) -> [&Arc<AtomicSet<InstrumentId>>; 5] {
        [
            &self.active_quote_subs,
            &self.active_delta_subs,
            &self.active_trade_subs,
            &self.active_instrument_status_subs,
            &self.active_instrument_close_subs,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SubscriptionKind {
    Deltas,
    Quotes,
    Trades,
    Status,
    Close,
}

pub(crate) fn resolve_token_id_from(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument_id: InstrumentId,
) -> anyhow::Result<String> {
    let loaded = instruments.load();
    let instrument = loaded
        .get(&instrument_id)
        .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;
    Ok(instrument.raw_symbol().as_str().to_string())
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::websocket::{handler::HandlerCommand, pool::PolymarketMarketPoolHandle};

    fn make_handle() -> (
        PolymarketMarketPoolHandle,
        tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    ) {
        make_handle_with_assigned(&[])
    }

    // Builds a single-shard pool handle with `assigned` tokens pre-owned, matching
    // the pool state a prior subscribe would leave. Needed for unsubscribe cases,
    // which route only for tokens the pool already owns.
    fn make_handle_with_assigned(
        assigned: &[&str],
    ) -> (
        PolymarketMarketPoolHandle,
        tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        (
            PolymarketMarketPoolHandle::test_single_shard(tx, assigned),
            rx,
        )
    }

    fn make_subscriptions(ws_client: PolymarketMarketPoolHandle) -> DataSubscriptions {
        DataSubscriptions::new(
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(Mutex::new(())),
            ws_client,
            false,
        )
    }

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET")
    }

    fn token_ustr() -> Ustr {
        Ustr::from("0xCOND-0xTOKEN")
    }

    fn resolution_watch(instrument_id: InstrumentId) -> Arc<AtomicMap<String, ResolveWatchEntry>> {
        let watchlist = Arc::new(AtomicMap::new());
        watchlist.insert(
            "0xCOND".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND".to_string(),
                expiration_ns: UnixNanos::from(u64::MAX),
                tracked: ahash::AHashMap::from_iter([(
                    "0xTOKEN".to_string(),
                    crate::resolve::TrackedInstrument {
                        instrument_id,
                        token_id: "0xTOKEN".to_string(),
                        price_precision: 2,
                        open_position_ids: AHashSet::new(),
                        has_data_subscription: true,
                    },
                )]),
                paused: false,
            },
        );
        watchlist
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_subscribes_when_intent_present_and_ws_closed() {
        let (ws, mut rx) = make_handle();
        let mut subscriptions = make_subscriptions(ws);

        let inst = instrument_id();
        subscriptions.active_quote_subs.insert(inst);

        subscriptions.watchlist = Arc::new(AtomicMap::new());
        subscriptions.subscribe_new_markets = false;
        subscriptions
            .reconcile(inst, inst.symbol.as_str().to_string())
            .await;

        assert!(subscriptions.ws_open_tokens.contains(&token_ustr()));

        match rx.try_recv().expect("expected SubscribeMarket command") {
            HandlerCommand::SubscribeMarket(ids) => {
                assert_eq!(ids, vec![inst.symbol.as_str().to_string()]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case::canonical("0xCOND-0xTOKEN")]
    #[case::legacy("0xTOKEN")]
    #[case::metadata_condition("0xALIAS-0xTOKEN")]
    #[tokio::test]
    async fn sync_ws_subscribes_when_resolution_intent_present_and_ws_closed(#[case] symbol: &str) {
        let (ws, mut rx) = make_handle();
        let mut subscriptions = make_subscriptions(ws);
        let inst = InstrumentId::from(format!("{symbol}.POLYMARKET").as_str());
        subscriptions.active_instrument_status_subs.insert(inst);

        subscriptions.watchlist = resolution_watch(inst);
        subscriptions.subscribe_new_markets = true;
        subscriptions
            .reconcile(inst, inst.symbol.as_str().to_string())
            .await;

        assert!(subscriptions.ws_open_tokens.contains(&Ustr::from(symbol)));
        assert!(matches!(
            rx.try_recv(),
            Ok(HandlerCommand::SubscribeMarket(ids)) if ids == vec![inst.symbol.as_str().to_string()]
        ));
    }

    #[rstest]
    #[case::status(true, false)]
    #[case::close(false, true)]
    #[tokio::test]
    async fn sync_ws_keeps_terminal_condition_open_for_resolution_intent(
        #[case] status_intent: bool,
        #[case] close_intent: bool,
    ) {
        let (ws, mut rx) = make_handle();
        let mut subscriptions = make_subscriptions(ws);
        let inst = instrument_id();
        subscriptions
            .closed_condition_ids
            .lock()
            .insert("0xCOND".to_string());

        if status_intent {
            subscriptions.active_instrument_status_subs.insert(inst);
        }

        if close_intent {
            subscriptions.active_instrument_close_subs.insert(inst);
        }

        subscriptions.watchlist = resolution_watch(inst);
        subscriptions.subscribe_new_markets = true;
        subscriptions
            .reconcile(inst, inst.symbol.as_str().to_string())
            .await;

        assert!(subscriptions.ws_open_tokens.contains(&token_ustr()));
        assert!(matches!(
            rx.try_recv(),
            Ok(HandlerCommand::SubscribeMarket(ids)) if ids == vec![inst.symbol.as_str().to_string()]
        ));
    }

    #[rstest]
    #[case::disabled(false, false)]
    #[case::paused(true, true)]
    #[case::enabled(true, false)]
    #[tokio::test]
    async fn resolution_wire_ownership_requires_enabled_unpaused_watch(
        #[case] enabled: bool,
        #[case] paused: bool,
    ) {
        let (ws, mut rx) = make_handle_with_assigned(&["0xCOND-0xTOKEN"]);
        let mut subscriptions = make_subscriptions(ws);
        let inst = instrument_id();
        let watchlist = resolution_watch(inst);
        watchlist.rcu(|entries| entries.get_mut("0xCOND").unwrap().paused = paused);
        subscriptions.active_instrument_status_subs.insert(inst);
        subscriptions
            .closed_condition_ids
            .lock()
            .insert("0xCOND".to_string());
        subscriptions.ws_open_tokens.insert(token_ustr());

        subscriptions.watchlist = watchlist;
        subscriptions.subscribe_new_markets = enabled;
        subscriptions
            .reconcile(inst, inst.symbol.as_str().to_string())
            .await;

        let retained = enabled && !paused;
        assert_eq!(
            subscriptions.ws_open_tokens.contains(&token_ustr()),
            retained
        );

        if retained {
            assert!(rx.try_recv().is_err());
        } else {
            assert!(
                matches!(rx.try_recv(), Ok(HandlerCommand::UnsubscribeMarket(ids))
                if ids == vec![inst.symbol.as_str().to_string()])
            );
        }
    }

    #[rstest]
    #[tokio::test]
    async fn resolution_reconciliation_preserves_large_active_watchlist(
        #[values(1_000, 10_000)] count: usize,
    ) {
        let (ws, mut rx) = make_handle();
        let mut subscriptions = make_subscriptions(ws);
        let watchlist = Arc::new(AtomicMap::new());
        let mut entries = ahash::AHashMap::new();
        let mut targets = Vec::new();

        for index in 0..count {
            let condition_id = format!("0xCOND{index}");
            let token_id = format!("0xTOKEN{index}");
            let instrument_id =
                InstrumentId::from(format!("{condition_id}-{token_id}.POLYMARKET").as_str());
            entries.insert(
                condition_id.clone(),
                ResolveWatchEntry {
                    condition_id,
                    expiration_ns: UnixNanos::from(u64::MAX),
                    tracked: ahash::AHashMap::from_iter([(
                        token_id.clone(),
                        crate::resolve::TrackedInstrument {
                            instrument_id,
                            token_id: token_id.clone(),
                            price_precision: 4,
                            open_position_ids: AHashSet::new(),
                            has_data_subscription: true,
                        },
                    )]),
                    paused: false,
                },
            );
            targets.push((instrument_id, token_id));
        }
        watchlist.store(entries);
        subscriptions
            .active_instrument_status_subs
            .store(targets.iter().map(|(id, _)| *id).collect());
        subscriptions.ws_open_tokens.store(
            targets
                .iter()
                .map(|(_, token_id)| Ustr::from(token_id.as_str()))
                .collect(),
        );
        let started = std::time::Instant::now();

        for (instrument_id, token_id) in &targets {
            subscriptions.watchlist = watchlist.clone();
            subscriptions.subscribe_new_markets = true;
            subscriptions
                .reconcile(*instrument_id, token_id.clone())
                .await;
        }

        eprintln!(
            "Reconciled {count} active resolution watches in {:?}",
            started.elapsed()
        );
        assert_eq!(subscriptions.ws_open_tokens.len(), count);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_unsubscribes_when_intent_absent_and_ws_open() {
        let (ws, mut rx) = make_handle_with_assigned(&["0xCOND-0xTOKEN"]);
        let mut subscriptions = make_subscriptions(ws);

        let inst = instrument_id();
        subscriptions.ws_open_tokens.insert(token_ustr());

        subscriptions.watchlist = Arc::new(AtomicMap::new());
        subscriptions.subscribe_new_markets = false;
        subscriptions
            .reconcile(inst, inst.symbol.as_str().to_string())
            .await;

        assert!(!subscriptions.ws_open_tokens.contains(&token_ustr()));

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
        let mut subscriptions = make_subscriptions(ws);

        let inst = instrument_id();
        if want {
            subscriptions.active_quote_subs.insert(inst);
        }

        if is_open_initial {
            subscriptions.ws_open_tokens.insert(token_ustr());
        }

        subscriptions.watchlist = Arc::new(AtomicMap::new());
        subscriptions.subscribe_new_markets = false;
        subscriptions
            .reconcile(inst, inst.symbol.as_str().to_string())
            .await;

        assert_eq!(
            subscriptions.ws_open_tokens.contains(&token_ustr()),
            is_open_initial
        );
        assert_eq!(rx.try_recv().is_ok(), expect_command);
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_rolls_back_open_tokens_on_send_failure() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        drop(rx);
        let ws = PolymarketMarketPoolHandle::test_single_shard(tx, &[]);

        let mut subscriptions = make_subscriptions(ws);

        let inst = instrument_id();
        subscriptions.active_quote_subs.insert(inst);

        subscriptions.watchlist = Arc::new(AtomicMap::new());
        subscriptions.subscribe_new_markets = false;
        subscriptions
            .reconcile(inst, inst.symbol.as_str().to_string())
            .await;

        assert!(!subscriptions.ws_open_tokens.contains(&token_ustr()));
    }

    #[rstest]
    #[case::any_kind(true, false, false)]
    #[case::another_kind(false, true, false)]
    #[case::third_kind(false, false, true)]
    #[tokio::test]
    async fn sync_ws_opens_for_any_active_kind(#[case] q: bool, #[case] d: bool, #[case] t: bool) {
        let (ws, mut rx) = make_handle();
        let mut subscriptions = make_subscriptions(ws);

        let inst = instrument_id();
        if q {
            subscriptions.active_quote_subs.insert(inst);
        }

        if d {
            subscriptions.active_delta_subs.insert(inst);
        }

        if t {
            subscriptions.active_trade_subs.insert(inst);
        }

        subscriptions.watchlist = Arc::new(AtomicMap::new());
        subscriptions.subscribe_new_markets = false;
        subscriptions
            .reconcile(inst, inst.symbol.as_str().to_string())
            .await;

        assert!(subscriptions.ws_open_tokens.contains(&token_ustr()));
        assert!(matches!(
            rx.try_recv(),
            Ok(HandlerCommand::SubscribeMarket(_))
        ));
    }
}
