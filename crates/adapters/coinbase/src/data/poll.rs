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

//! REST-polling support for derivatives-only data streams.
//!
//! Coinbase Advanced Trade does not publish index prices or funding rates on
//! any WebSocket channel, so the data client fills the gap by periodically
//! fetching `/products/{id}` and emitting [`IndexPriceUpdate`] and
//! [`FundingRateUpdate`] events. One polling task runs per instrument with
//! at least one active subscription; subscribe / unsubscribe on any
//! supported kind flips a flag on the shared state rather than spinning up
//! additional loops.
//!
//! Keeping the polling internals in their own module isolates them from the
//! WS-driven subscription paths in [`super`] and lets the client delegate to
//! a narrow [`DerivPollManager`] API.

use std::{
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use ahash::AHashMap;
use nautilus_common::{live::get_runtime, messages::DataEvent};
use nautilus_core::{MUTEX_POISONED, UnixNanos, time::AtomicTime};
use nautilus_model::{
    data::{Data, FundingRateUpdate, IndexPriceUpdate},
    identifiers::InstrumentId,
    types::Price,
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::http::{client::CoinbaseHttpClient, models::Product};

/// Per-instrument polling flags plus the cancellation handle for whichever
/// polling task is (or most recently was) live for this instrument.
///
/// The `cancel` token is replaced on every `spawn_task` so `shutdown()` can
/// cancel the previous task cleanly, `resume()` can start a new one, and
/// the flags survive disconnect/reconnect so the data-client adapter's
/// suppressed re-subscribe commands don't leave streams dark after a
/// round-trip.
#[derive(Debug)]
struct DerivPollState {
    emit_index: bool,
    emit_funding: bool,
    cancel: CancellationToken,
}

/// Coordinates REST-polled derivatives subscriptions for the Coinbase data
/// client.
///
/// Holds clones of the shared dependencies a polling task needs (HTTP
/// client, data-event sender, clock) plus a mutex-guarded map of
/// per-instrument state. Subscribing is idempotent: the first subscription
/// for an instrument spawns a shared task that fires on
/// `interval_secs`, and subsequent subscribes (for other supported kinds)
/// simply flip additional flags on the existing state. Unsubscribing clears
/// the requested flag and cancels the task once no flags remain.
///
/// `shutdown()` cancels all live tasks but preserves the subscription
/// flags, and `resume()` re-spawns tasks for any entries that survived so
/// subscriptions outlive a `disconnect()` / `connect()` cycle.
#[derive(Debug)]
pub(crate) struct DerivPollManager {
    polls: Arc<Mutex<AHashMap<InstrumentId, DerivPollState>>>,
    http_client: CoinbaseHttpClient,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
    interval_secs: u64,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl DerivPollManager {
    pub(crate) fn new(
        http_client: CoinbaseHttpClient,
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        clock: &'static AtomicTime,
        interval_secs: u64,
    ) -> Self {
        Self {
            polls: Arc::new(Mutex::new(AHashMap::new())),
            http_client,
            data_sender,
            clock,
            interval_secs: interval_secs.max(1),
            tasks: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn subscribe_index(&self, instrument_id: InstrumentId) {
        self.register(instrument_id, true, false);
    }

    pub(crate) fn subscribe_funding(&self, instrument_id: InstrumentId) {
        self.register(instrument_id, false, true);
    }

    pub(crate) fn unsubscribe_index(&self, instrument_id: InstrumentId) {
        self.unregister(instrument_id, true, false);
    }

    pub(crate) fn unsubscribe_funding(&self, instrument_id: InstrumentId) {
        self.unregister(instrument_id, false, true);
    }

    /// Cancels every active polling task but keeps the subscription flags
    /// in the map so [`Self::resume`] can re-spawn them after reconnect.
    /// Safe to call multiple times.
    pub(crate) fn shutdown(&self) {
        {
            let mut polls = self.polls.lock().expect(MUTEX_POISONED);
            for entry in polls.values_mut() {
                entry.cancel.cancel();
                // Replace the now-cancelled token so a later `resume()` can
                // spawn a fresh task that listens on a live token.
                entry.cancel = CancellationToken::new();
            }
        }

        let mut tasks = self.tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    /// Spawns polling tasks for every entry with at least one active flag.
    /// Called from `connect()` so subscriptions made before a
    /// `disconnect()` remain live after the client reconnects: the data
    /// engine suppresses duplicate subscribe commands, so the caller does
    /// not re-issue them.
    pub(crate) fn resume(&self) {
        let entries: Vec<(InstrumentId, CancellationToken)> = {
            let polls = self.polls.lock().expect(MUTEX_POISONED);
            polls
                .iter()
                .filter(|(_, state)| state.emit_index || state.emit_funding)
                .map(|(id, state)| (*id, state.cancel.clone()))
                .collect()
        };

        for (instrument_id, token) in entries {
            self.spawn_task(instrument_id, token);
        }
    }

    fn register(&self, instrument_id: InstrumentId, want_index: bool, want_funding: bool) {
        let (token, is_new) = {
            let mut polls = self.polls.lock().expect(MUTEX_POISONED);
            let is_new = !polls.contains_key(&instrument_id);
            let entry = polls
                .entry(instrument_id)
                .or_insert_with(|| DerivPollState {
                    emit_index: false,
                    emit_funding: false,
                    cancel: CancellationToken::new(),
                });

            if want_index {
                entry.emit_index = true;
            }

            if want_funding {
                entry.emit_funding = true;
            }

            (entry.cancel.clone(), is_new)
        };

        // Prune any completed poll handles before possibly pushing a new
        // one so the task vec stays bounded under subscribe/unsubscribe
        // churn on a long-lived client.
        self.reap_finished_tasks();

        if is_new {
            self.spawn_task(instrument_id, token);
        }
    }

    fn unregister(&self, instrument_id: InstrumentId, drop_index: bool, drop_funding: bool) {
        let mut polls = self.polls.lock().expect(MUTEX_POISONED);
        let should_cancel = match polls.get_mut(&instrument_id) {
            Some(entry) => {
                if drop_index {
                    entry.emit_index = false;
                }

                if drop_funding {
                    entry.emit_funding = false;
                }

                !entry.emit_index && !entry.emit_funding
            }
            None => false,
        };

        if should_cancel && let Some(entry) = polls.remove(&instrument_id) {
            entry.cancel.cancel();
        }
        drop(polls);

        // Cancelled tasks finish asynchronously; drop any that already
        // completed on this pass, and any prior cycles still sitting in
        // the vec. This keeps `tasks.len()` bounded by the number of
        // currently live poll loops.
        self.reap_finished_tasks();
    }

    fn reap_finished_tasks(&self) {
        let mut tasks = self.tasks.lock().expect(MUTEX_POISONED);
        tasks.retain(|handle| !handle.is_finished());
    }

    fn spawn_task(&self, instrument_id: InstrumentId, cancel: CancellationToken) {
        let interval_secs = self.interval_secs;
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let polls = Arc::clone(&self.polls);
        let clock = self.clock;
        let product_id = instrument_id.symbol.inner();

        let handle = get_runtime().spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    _ = interval.tick() => {
                        let (emit_index, emit_funding) = {
                            let state = polls.lock().expect(MUTEX_POISONED);
                            match state.get(&instrument_id) {
                                Some(entry) => (entry.emit_index, entry.emit_funding),
                                None => break,
                            }
                        };

                        if !emit_index && !emit_funding {
                            continue;
                        }

                        // Preempt the REST call on cancellation: a late
                        // unsubscribe must not have to wait for the in-
                        // flight request before the task exits.
                        let result = tokio::select! {
                            () = cancel.cancelled() => break,
                            res = http_client.request_raw_product(product_id.as_str()) => res,
                        };

                        match result {
                            Ok(product) => {
                                // Re-check the current flags after the
                                // await: unsubscribe may have flipped or
                                // cleared them while the request was in
                                // flight, and we must not emit for a kind
                                // the caller just turned off.
                                let (still_index, still_funding) = {
                                    let state = polls.lock().expect(MUTEX_POISONED);
                                    match state.get(&instrument_id) {
                                        Some(entry) => (entry.emit_index, entry.emit_funding),
                                        None => break,
                                    }
                                };
                                emit_deriv_updates(
                                    instrument_id,
                                    &product,
                                    emit_index && still_index,
                                    emit_funding && still_funding,
                                    clock.get_time_ns(),
                                    &sender,
                                );
                            }
                            Err(e) => log::warn!(
                                "Coinbase derivatives poll failed for {instrument_id}: {e}"
                            ),
                        }
                    }
                }
            }

            log::debug!("Coinbase derivatives poll task stopped for {instrument_id}");
        });

        self.tasks.lock().expect(MUTEX_POISONED).push(handle);
    }
}

/// Emits [`IndexPriceUpdate`] and/or [`FundingRateUpdate`] from the product
/// payload based on which subscription flags are active.
pub(crate) fn emit_deriv_updates(
    instrument_id: InstrumentId,
    product: &Product,
    emit_index: bool,
    emit_funding: bool,
    ts_now: UnixNanos,
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) {
    let Some(details) = product.future_product_details.as_ref() else {
        log::debug!("Skipping derivatives update for {instrument_id}: not a futures product");
        return;
    };

    if emit_index
        && let Some(raw) = details.index_price.as_deref().filter(|s| !s.is_empty())
        && let Ok(decimal) = Decimal::from_str(raw)
        && let Ok(price) = Price::from_decimal_dp(decimal, precision_from_index(raw))
    {
        let update = IndexPriceUpdate::new(instrument_id, price, ts_now, ts_now);
        if let Err(e) = sender.send(DataEvent::Data(Data::IndexPriceUpdate(update))) {
            log::error!("Failed to send IndexPriceUpdate for {instrument_id}: {e}");
        }
    }

    if emit_funding && !details.funding_rate.is_empty() {
        match Decimal::from_str(&details.funding_rate) {
            Ok(rate) => {
                let next_funding = details
                    .funding_time
                    .as_deref()
                    .and_then(|ts| crate::http::parse::parse_rfc3339_timestamp(ts).ok());
                let interval = details
                    .funding_interval
                    .as_deref()
                    .and_then(parse_funding_interval_minutes);
                let update = FundingRateUpdate::new(
                    instrument_id,
                    rate,
                    interval,
                    next_funding,
                    ts_now,
                    ts_now,
                );

                if let Err(e) = sender.send(DataEvent::FundingRate(update)) {
                    log::error!("Failed to send FundingRateUpdate for {instrument_id}: {e}");
                }
            }
            Err(e) => log::warn!(
                "Failed to parse funding_rate='{}' for {instrument_id}: {e}",
                details.funding_rate
            ),
        }
    }
}

/// Derives price precision from the decimal representation returned by the
/// venue. Coinbase publishes `index_price` with more decimals than the
/// instrument's tick size, so reusing the product's tick precision would
/// silently truncate updates; the per-field precision keeps the full value.
pub(crate) fn precision_from_index(value: &str) -> u8 {
    value
        .split_once('.')
        .map_or(0, |(_, frac)| frac.trim_end_matches('0').len() as u8)
}

/// Parses a Coinbase `funding_interval` string (e.g. `"3600s"`) into
/// minutes. Returns `None` for missing or malformed values so the venue
/// interval simply stays unset on the emitted update.
pub(crate) fn parse_funding_interval_minutes(raw: &str) -> Option<u16> {
    if raw.is_empty() {
        return None;
    }
    let trimmed = raw.strip_suffix('s').unwrap_or(raw);
    let secs: u64 = trimmed.parse().ok()?;
    u16::try_from(secs / 60).ok()
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::DataEvent;
    use nautilus_core::UnixNanos;
    use nautilus_model::{data::Data, identifiers::InstrumentId};
    use rstest::rstest;
    use rust_decimal::Decimal;
    use tokio_util::sync::CancellationToken;

    use super::{
        DerivPollState, emit_deriv_updates, parse_funding_interval_minutes, precision_from_index,
    };
    use crate::{
        common::testing::load_test_fixture,
        http::models::{Product, ProductsResponse},
    };

    fn perp_product() -> Product {
        let json = load_test_fixture("http_products_future.json");
        let response: ProductsResponse = serde_json::from_str(&json).unwrap();
        response
            .products
            .into_iter()
            .find(|p| p.display_name.contains("PERP"))
            .expect("fixture has a PERP product")
    }

    #[rstest]
    fn test_parse_funding_interval_accepts_seconds_suffix() {
        assert_eq!(parse_funding_interval_minutes("3600s"), Some(60));
        assert_eq!(parse_funding_interval_minutes("60"), Some(1));
    }

    #[rstest]
    fn test_parse_funding_interval_rejects_empty_and_garbage() {
        assert_eq!(parse_funding_interval_minutes(""), None);
        assert_eq!(parse_funding_interval_minutes("not-a-number"), None);
    }

    #[rstest]
    fn test_precision_from_index_trims_trailing_zeros() {
        assert_eq!(precision_from_index("79190.103206"), 6);
        assert_eq!(precision_from_index("79190"), 0);
        assert_eq!(precision_from_index("79190.00"), 0);
        assert_eq!(precision_from_index("0.00001"), 5);
    }

    #[rstest]
    fn test_emit_deriv_updates_sends_index_and_funding() {
        let mut product = perp_product();
        let details = product.future_product_details.as_mut().unwrap();
        // The shared future fixture leaves funding fields off the PERP row
        // because Coinbase only returns them when a contract is live; seed
        // the minimum needed for this test so both event paths exercise
        // the full build + send sequence.
        details.index_price = Some("79190.103206".to_string());
        details.funding_rate = "0.000004".to_string();
        details.funding_time = Some("2026-04-22T15:00:00Z".to_string());
        details.funding_interval = Some("3600s".to_string());

        let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let ts = UnixNanos::default();

        emit_deriv_updates(instrument_id, &product, true, true, ts, &tx);

        let mut got_index = None;
        let mut got_funding = None;

        while let Ok(evt) = rx.try_recv() {
            match evt {
                DataEvent::Data(Data::IndexPriceUpdate(ip)) => {
                    got_index = Some(ip);
                }
                DataEvent::FundingRate(fr) => got_funding = Some(fr),
                _ => {}
            }
        }

        let ip = got_index.expect("index update emitted");
        assert_eq!(ip.instrument_id, instrument_id);
        assert_eq!(
            ip.value.as_decimal(),
            Decimal::from_str_exact("79190.103206").unwrap()
        );

        let fr = got_funding.expect("funding update emitted");
        assert_eq!(fr.instrument_id, instrument_id);
        assert_eq!(fr.rate, Decimal::from_str_exact("0.000004").unwrap());
        assert_eq!(fr.interval, Some(60));
        assert!(fr.next_funding_ns.is_some());
    }

    #[rstest]
    fn test_emit_deriv_updates_skips_when_flag_off() {
        let product = perp_product();
        let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emit_deriv_updates(
            instrument_id,
            &product,
            false,
            false,
            UnixNanos::default(),
            &tx,
        );
        assert!(rx.try_recv().is_err(), "neither flag on => no events");
    }

    #[rstest]
    fn test_emit_deriv_updates_no_op_for_non_futures_product() {
        let json = load_test_fixture("http_product.json");
        let product: Product = serde_json::from_str(&json).unwrap();
        let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emit_deriv_updates(
            instrument_id,
            &product,
            true,
            true,
            UnixNanos::default(),
            &tx,
        );
        assert!(
            rx.try_recv().is_err(),
            "spot product has no future_product_details; must emit nothing"
        );
    }

    // Shutdown must preserve subscription flags so reconnect resumes the
    // stream without the caller re-issuing subscribe commands (the data
    // engine's adapter already suppresses duplicates).
    #[rstest]
    #[tokio::test]
    async fn test_manager_shutdown_preserves_flags_for_resume() {
        let manager = make_manager(60);
        let instrument_id = perp_id();

        manager.subscribe_index(instrument_id);
        manager.subscribe_funding(instrument_id);

        // Capture a clone of the live token before shutdown so we can
        // verify both that the old token was cancelled and that the entry
        // now holds a fresh, uncancelled replacement.
        let old_token = manager
            .polls
            .lock()
            .unwrap()
            .get(&instrument_id)
            .expect("entry after subscribe")
            .cancel
            .clone();
        assert!(!old_token.is_cancelled(), "token is live before shutdown");
        assert_eq!(
            manager.tasks.lock().unwrap().len(),
            1,
            "one shared task spawned for two subscriptions on the same instrument"
        );

        manager.shutdown();

        let polls = manager.polls.lock().unwrap();
        let entry = polls.get(&instrument_id).expect("shutdown preserves entry");
        assert!(entry.emit_index);
        assert!(entry.emit_funding);
        assert!(
            old_token.is_cancelled(),
            "shutdown must cancel the previously-live token"
        );
        assert!(
            !entry.cancel.is_cancelled(),
            "shutdown must swap in a fresh token so resume() can spawn"
        );
        assert!(
            manager.tasks.lock().unwrap().is_empty(),
            "shutdown must drain the task vec"
        );
    }

    // Subscribing both kinds for the same instrument must share one task
    // (one register call opens the entry; the second just flips a flag).
    // A regression that always spawns would leak tasks on every subscribe.
    #[rstest]
    #[tokio::test]
    async fn test_manager_subscribe_same_instrument_shares_task() {
        let manager = make_manager(60);
        let instrument_id = perp_id();

        manager.subscribe_index(instrument_id);
        manager.subscribe_funding(instrument_id);

        let polls = manager.polls.lock().unwrap();
        assert_eq!(polls.len(), 1, "single entry for one instrument");
        let entry = polls.get(&instrument_id).unwrap();
        assert!(entry.emit_index && entry.emit_funding);
        drop(polls);

        assert_eq!(
            manager.tasks.lock().unwrap().len(),
            1,
            "two subscribes for the same id must share one poll task"
        );
    }

    // Unsubscribing one kind while the other remains active keeps the task
    // alive. Only the requested flag flips off.
    #[rstest]
    #[case::drop_index_keep_funding(true, false, false, true)]
    #[case::drop_funding_keep_index(false, true, true, false)]
    #[tokio::test]
    async fn test_manager_unsubscribe_keeps_other_kind_alive(
        #[case] drop_index: bool,
        #[case] drop_funding: bool,
        #[case] expect_index: bool,
        #[case] expect_funding: bool,
    ) {
        let manager = make_manager(60);
        let instrument_id = perp_id();

        manager.subscribe_index(instrument_id);
        manager.subscribe_funding(instrument_id);

        if drop_index {
            manager.unsubscribe_index(instrument_id);
        }

        if drop_funding {
            manager.unsubscribe_funding(instrument_id);
        }

        let polls = manager.polls.lock().unwrap();
        let entry = polls
            .get(&instrument_id)
            .expect("entry remains while one flag is active");
        assert_eq!(entry.emit_index, expect_index);
        assert_eq!(entry.emit_funding, expect_funding);
        assert!(
            !entry.cancel.is_cancelled(),
            "the shared task must stay alive while a flag remains"
        );
    }

    // Unsubscribe then subscribe for the same instrument must start a
    // fresh task with a live cancel token; the entry that survives the
    // unsubscribe→subscribe round trip must not share state with the
    // cancelled cycle.
    #[rstest]
    #[tokio::test]
    async fn test_manager_resubscribe_after_unsubscribe_spawns_fresh_task() {
        let manager = make_manager(60);
        let instrument_id = perp_id();

        manager.subscribe_index(instrument_id);
        let first_token = manager
            .polls
            .lock()
            .unwrap()
            .get(&instrument_id)
            .unwrap()
            .cancel
            .clone();

        manager.unsubscribe_index(instrument_id);
        assert!(first_token.is_cancelled());

        manager.subscribe_index(instrument_id);
        let polls = manager.polls.lock().unwrap();
        let entry = polls
            .get(&instrument_id)
            .expect("re-subscribe re-inserts the entry");
        assert!(entry.emit_index);
        assert!(!entry.emit_funding);
        // The new token must be live, while the prior cycle's token is
        // still cancelled: together these prove the manager installed a
        // distinct token for the fresh task rather than reusing the
        // cancelled one.
        assert!(
            !entry.cancel.is_cancelled(),
            "re-subscribe must install a fresh, uncancelled token"
        );
        assert!(
            first_token.is_cancelled(),
            "prior token must stay cancelled so any leftover task exits"
        );
    }

    // Unsubscribing the last active kind removes the entry and cancels
    // the shared task.
    #[rstest]
    #[tokio::test]
    async fn test_manager_unsubscribe_last_flag_removes_entry() {
        let manager = make_manager(60);
        let instrument_id = perp_id();

        manager.subscribe_index(instrument_id);
        let token = manager
            .polls
            .lock()
            .unwrap()
            .get(&instrument_id)
            .unwrap()
            .cancel
            .clone();

        manager.unsubscribe_index(instrument_id);

        assert!(
            !manager.polls.lock().unwrap().contains_key(&instrument_id),
            "entry removed when last flag flips off"
        );
        assert!(
            token.is_cancelled(),
            "task token cancelled so the poll loop exits"
        );
    }

    // Under steady-state subscribe / unsubscribe churn, completed poll
    // handles must not accumulate in the task vec. Without the reap step
    // a long-lived client that repeatedly flips subscriptions leaks one
    // JoinHandle per cycle.
    //
    // The test asserts the invariant through the public surface only:
    // it never calls `reap_finished_tasks()` directly, so any regression
    // that removes the reap from `register` or `unregister` would fail
    // here.
    #[rstest]
    #[tokio::test]
    async fn test_manager_does_not_leak_task_handles_on_churn() {
        let manager = make_manager(60);
        let instrument_id = perp_id();

        for _ in 0..20 {
            manager.subscribe_index(instrument_id);
            manager.unsubscribe_index(instrument_id);
            // Let each cancelled task notice the token flip and return
            // so the next register's reap can drop its handle.
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        // After the churn loop the manager may still be holding the
        // final cycle's handle because nothing has reaped since it was
        // cancelled. Wait for that task to finish, then trigger one more
        // subscribe: register's leading reap sweeps every accumulated
        // handle before pushing its own.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        manager.subscribe_index(instrument_id);

        assert!(
            manager.tasks.lock().unwrap().len() <= 1,
            "task vec should stay bounded under subscribe/unsubscribe churn, \
             was {}",
            manager.tasks.lock().unwrap().len()
        );

        manager.unsubscribe_index(instrument_id);
    }

    // The full "disconnect then reconnect" trip: shutdown drops tasks but
    // keeps flags, and resume() re-spawns one task per surviving entry.
    #[rstest]
    #[tokio::test]
    async fn test_manager_resume_respawns_tasks_for_surviving_entries() {
        let manager = make_manager(60);
        let instrument_id = perp_id();

        manager.subscribe_index(instrument_id);
        manager.subscribe_funding(instrument_id);
        manager.shutdown();
        assert!(manager.tasks.lock().unwrap().is_empty());

        manager.resume();

        let polls = manager.polls.lock().unwrap();
        let entry = polls
            .get(&instrument_id)
            .expect("entry survives shutdown + resume");
        assert!(entry.emit_index && entry.emit_funding);
        drop(polls);

        assert_eq!(
            manager.tasks.lock().unwrap().len(),
            1,
            "resume spawns one task per entry with any active flag"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_manager_resume_skips_entries_with_no_active_flag() {
        let manager = make_manager(60);
        let instrument_id = perp_id();

        // Seed an entry with both flags false (only reachable via direct
        // insertion; `subscribe_*` always sets a flag). Simulate the case
        // where a future change leaves an orphan entry in the map.
        manager.polls.lock().unwrap().insert(
            instrument_id,
            DerivPollState {
                emit_index: false,
                emit_funding: false,
                cancel: CancellationToken::new(),
            },
        );

        manager.resume();
        assert!(
            manager.tasks.lock().unwrap().is_empty(),
            "resume must not spawn for zero-flag entries"
        );
    }

    // Partial-flag emission matrix. Closes the `(true, false)` and
    // `(false, true)` cases the existing happy-path test did not cover.
    #[rstest]
    #[case::only_index(true, false)]
    #[case::only_funding(false, true)]
    fn test_emit_deriv_updates_partial_flags(#[case] emit_index: bool, #[case] emit_funding: bool) {
        let product = seeded_perp_product();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emit_deriv_updates(
            perp_id(),
            &product,
            emit_index,
            emit_funding,
            UnixNanos::default(),
            &tx,
        );

        let mut got_index = false;
        let mut got_funding = false;

        while let Ok(evt) = rx.try_recv() {
            match evt {
                DataEvent::Data(Data::IndexPriceUpdate(_)) => {
                    got_index = true;
                }
                DataEvent::FundingRate(_) => got_funding = true,
                _ => {}
            }
        }

        assert_eq!(got_index, emit_index);
        assert_eq!(got_funding, emit_funding);
    }

    // Malformed / absent wire fields must short-circuit without panicking
    // and without emitting anything for that kind.
    #[rstest]
    fn test_emit_deriv_updates_drops_missing_index_price() {
        let mut product = seeded_perp_product();
        product.future_product_details.as_mut().unwrap().index_price = None;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emit_deriv_updates(perp_id(), &product, true, false, UnixNanos::default(), &tx);
        assert!(rx.try_recv().is_err(), "index_price=None must not emit");
    }

    #[rstest]
    fn test_emit_deriv_updates_drops_empty_funding_rate() {
        let mut product = seeded_perp_product();
        product
            .future_product_details
            .as_mut()
            .unwrap()
            .funding_rate = String::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emit_deriv_updates(perp_id(), &product, false, true, UnixNanos::default(), &tx);
        assert!(rx.try_recv().is_err(), "empty funding_rate must not emit");
    }

    #[rstest]
    fn test_emit_deriv_updates_handles_malformed_funding_rate_without_panic() {
        let mut product = seeded_perp_product();
        product
            .future_product_details
            .as_mut()
            .unwrap()
            .funding_rate = "not-a-decimal".to_string();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emit_deriv_updates(perp_id(), &product, true, true, UnixNanos::default(), &tx);

        // Malformed funding_rate path logs a warning and does not emit a
        // FundingRateUpdate; the index path still emits.
        let mut got_index = false;
        let mut got_funding = false;

        while let Ok(evt) = rx.try_recv() {
            match evt {
                DataEvent::Data(Data::IndexPriceUpdate(_)) => {
                    got_index = true;
                }
                DataEvent::FundingRate(_) => got_funding = true,
                _ => {}
            }
        }
        assert!(got_index, "valid index path must still emit");
        assert!(!got_funding, "malformed funding_rate must be dropped");
    }

    // Funding must emit even when the venue does not report funding_time
    // or funding_interval; those fields just carry through as None on the
    // Nautilus event.
    #[rstest]
    fn test_emit_funding_without_time_or_interval() {
        let mut product = seeded_perp_product();
        let details = product.future_product_details.as_mut().unwrap();
        details.funding_time = None;
        details.funding_interval = None;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        emit_deriv_updates(perp_id(), &product, false, true, UnixNanos::default(), &tx);

        let mut got_funding = None;

        while let Ok(evt) = rx.try_recv() {
            if let DataEvent::FundingRate(fr) = evt {
                got_funding = Some(fr);
            }
        }
        let fr = got_funding.expect("funding emitted with rate only");
        assert!(fr.next_funding_ns.is_none());
        assert!(fr.interval.is_none());
    }

    fn perp_id() -> InstrumentId {
        InstrumentId::from("BIP-20DEC30-CDE.COINBASE")
    }

    fn make_manager(interval_secs: u64) -> super::DerivPollManager {
        use crate::{common::enums::CoinbaseEnvironment, http::client::CoinbaseHttpClient};
        let http = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 5, None, None).unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let clock = nautilus_core::time::get_atomic_clock_realtime();
        super::DerivPollManager::new(http, tx, clock, interval_secs)
    }

    fn seeded_perp_product() -> Product {
        let mut product = perp_product();
        let details = product.future_product_details.as_mut().unwrap();
        details.index_price = Some("79190.103206".to_string());
        details.funding_rate = "0.000004".to_string();
        details.funding_time = Some("2026-04-22T15:00:00Z".to_string());
        details.funding_interval = Some("3600s".to_string());
        product
    }
}
