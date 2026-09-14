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

use ahash::AHashSet;
use nautilus_common::cache::InstrumentLookupError;
use nautilus_core::{AtomicMap, AtomicSet};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use parking_lot::Mutex;
use ustr::Ustr;

use crate::resolve::ResolveWatchEntry;

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

// Reconciles the WS subscription for `instrument_id` with the union of caller
// intents. Holds `ws_sub_mutex` across the async WS send so concurrent
// subscribe/unsubscribe calls arrive at the WS handler in mutex-release order;
// that makes the final wire state consistent with the last writer.
#[allow(
    clippy::too_many_arguments,
    reason = "shared state comes in as Arc refs"
)]
pub(crate) async fn sync_ws_subscription_with_resolution_and_terminal_async(
    instrument_id: InstrumentId,
    token_id_str: String,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    active_instrument_status_subs: Arc<AtomicSet<InstrumentId>>,
    active_instrument_close_subs: Arc<AtomicSet<InstrumentId>>,
    closed_condition_ids: Arc<Mutex<AHashSet<String>>>,
    ws_open_tokens: Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    ws: crate::websocket::pool::PolymarketMarketPoolHandle,
    resolve_watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    subscribe_new_markets: bool,
) {
    let token_id = Ustr::from(token_id_str.as_str());
    let _guard = ws_sub_mutex.lock().await;

    let condition_id = crate::providers::extract_condition_id(&instrument_id).ok();
    let is_terminal = condition_id
        .as_ref()
        .is_some_and(|condition_id| closed_condition_ids.lock().contains(condition_id));
    // Only the enabled venue feed can carry resolution events, and a paused
    // watch retains manual recovery ownership rather than a wire subscription.
    let wants_resolution = subscribe_new_markets
        && (active_instrument_status_subs.contains(&instrument_id)
            || active_instrument_close_subs.contains(&instrument_id))
        && {
            let watchlist = resolve_watchlist.load();
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
                .is_some_and(&has_active_data_owner)
                || watchlist.values().any(has_active_data_owner)
        };
    let wants_subscribe = wants_resolution
        || (!is_terminal
            && (active_quote_subs.contains(&instrument_id)
                || active_delta_subs.contains(&instrument_id)
                || active_trade_subs.contains(&instrument_id)));
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

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::websocket::{handler::HandlerCommand, pool::PolymarketMarketPoolHandle};

    type ActiveSet = Arc<AtomicSet<InstrumentId>>;
    type OpenTokens = Arc<AtomicSet<Ustr>>;
    type WsMutex = Arc<tokio::sync::Mutex<()>>;
    type ClosedConditions = Arc<Mutex<AHashSet<String>>>;

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

    fn make_state() -> (
        ActiveSet,
        ActiveSet,
        ActiveSet,
        ActiveSet,
        ActiveSet,
        ClosedConditions,
        OpenTokens,
        WsMutex,
    ) {
        (
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(Mutex::new(AHashSet::new())),
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
        let (quotes, deltas, trades, status, close, closed, open, mutex) = make_state();

        let inst = instrument_id();
        quotes.insert(inst);

        sync_ws_subscription_with_resolution_and_terminal_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes.clone(),
            deltas,
            trades,
            status,
            close,
            closed,
            open.clone(),
            mutex,
            ws,
            Arc::new(AtomicMap::new()),
            false,
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
    #[case::canonical("0xCOND-0xTOKEN")]
    #[case::legacy("0xTOKEN")]
    #[case::metadata_condition("0xALIAS-0xTOKEN")]
    #[tokio::test]
    async fn sync_ws_subscribes_when_resolution_intent_present_and_ws_closed(#[case] symbol: &str) {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, status, close, closed, open, mutex) = make_state();
        let inst = InstrumentId::from(format!("{symbol}.POLYMARKET").as_str());
        status.insert(inst);

        sync_ws_subscription_with_resolution_and_terminal_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            status,
            close,
            closed,
            open.clone(),
            mutex,
            ws,
            resolution_watch(inst),
            true,
        )
        .await;

        assert!(open.contains(&Ustr::from(symbol)));
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
        let (quotes, deltas, trades, status, close, closed, open, mutex) = make_state();
        let inst = instrument_id();
        closed.lock().insert("0xCOND".to_string());

        if status_intent {
            status.insert(inst);
        }

        if close_intent {
            close.insert(inst);
        }

        sync_ws_subscription_with_resolution_and_terminal_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            status,
            close,
            closed,
            open.clone(),
            mutex,
            ws,
            resolution_watch(inst),
            true,
        )
        .await;

        assert!(open.contains(&token_ustr()));
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
        let (quotes, deltas, trades, status, close, closed, open, mutex) = make_state();
        let inst = instrument_id();
        let watchlist = resolution_watch(inst);
        watchlist.rcu(|entries| entries.get_mut("0xCOND").unwrap().paused = paused);
        status.insert(inst);
        closed.lock().insert("0xCOND".to_string());
        open.insert(token_ustr());

        sync_ws_subscription_with_resolution_and_terminal_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            status,
            close,
            closed,
            open.clone(),
            mutex,
            ws,
            watchlist,
            enabled,
        )
        .await;

        let retained = enabled && !paused;
        assert_eq!(open.contains(&token_ustr()), retained);
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
        let (quotes, deltas, trades, status, close, closed, open, mutex) = make_state();
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
        status.store(targets.iter().map(|(id, _)| *id).collect());
        open.store(
            targets
                .iter()
                .map(|(_, token_id)| Ustr::from(token_id.as_str()))
                .collect(),
        );
        let started = std::time::Instant::now();

        for (instrument_id, token_id) in &targets {
            sync_ws_subscription_with_resolution_and_terminal_async(
                *instrument_id,
                token_id.clone(),
                quotes.clone(),
                deltas.clone(),
                trades.clone(),
                status.clone(),
                close.clone(),
                closed.clone(),
                open.clone(),
                mutex.clone(),
                ws.clone(),
                watchlist.clone(),
                true,
            )
            .await;
        }

        eprintln!(
            "Reconciled {count} active resolution watches in {:?}",
            started.elapsed()
        );
        assert_eq!(open.len(), count);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_unsubscribes_when_intent_absent_and_ws_open() {
        let (ws, mut rx) = make_handle_with_assigned(&["0xCOND-0xTOKEN"]);
        let (quotes, deltas, trades, status, close, closed, open, mutex) = make_state();

        let inst = instrument_id();
        open.insert(token_ustr());

        sync_ws_subscription_with_resolution_and_terminal_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            status,
            close,
            closed,
            open.clone(),
            mutex,
            ws,
            Arc::new(AtomicMap::new()),
            false,
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
        let (quotes, deltas, trades, status, close, closed, open, mutex) = make_state();

        let inst = instrument_id();

        if want {
            quotes.insert(inst);
        }

        if is_open_initial {
            open.insert(token_ustr());
        }

        sync_ws_subscription_with_resolution_and_terminal_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            status,
            close,
            closed,
            open.clone(),
            mutex,
            ws,
            Arc::new(AtomicMap::new()),
            false,
        )
        .await;

        assert_eq!(open.contains(&token_ustr()), is_open_initial);
        assert_eq!(rx.try_recv().is_ok(), expect_command);
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_rolls_back_open_tokens_on_send_failure() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        drop(rx);
        let ws = PolymarketMarketPoolHandle::test_single_shard(tx, &[]);

        let (quotes, deltas, trades, status, close, closed, open, mutex) = make_state();

        let inst = instrument_id();
        quotes.insert(inst);

        sync_ws_subscription_with_resolution_and_terminal_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            status,
            close,
            closed,
            open.clone(),
            mutex,
            ws,
            Arc::new(AtomicMap::new()),
            false,
        )
        .await;

        assert!(!open.contains(&token_ustr()));
    }

    #[rstest]
    #[case::any_kind(true, false, false)]
    #[case::another_kind(false, true, false)]
    #[case::third_kind(false, false, true)]
    #[tokio::test]
    async fn sync_ws_opens_for_any_active_kind(#[case] q: bool, #[case] d: bool, #[case] t: bool) {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, status, close, closed, open, mutex) = make_state();

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

        sync_ws_subscription_with_resolution_and_terminal_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            status,
            close,
            closed,
            open.clone(),
            mutex,
            ws,
            Arc::new(AtomicMap::new()),
            false,
        )
        .await;

        assert!(open.contains(&token_ustr()));
        assert!(matches!(
            rx.try_recv(),
            Ok(HandlerCommand::SubscribeMarket(_))
        ));
    }
}
