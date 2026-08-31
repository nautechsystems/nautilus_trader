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

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use nautilus_common::messages::DataEvent;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

use super::{
    PolymarketDataClient,
    instruments::{
        apply_live_instrument, publish_cached_condition_closed, query_positive_closed_condition_ids,
    },
    runtime::{
        is_condition_closed, register_closed_condition_for_live_data, retire_closed_condition_state,
    },
    subscriptions::{
        resolve_token_id_from, sync_ws_subscription_with_resolution_and_terminal_async,
    },
};
use crate::{
    common::consts::GAMMA_CONDITION_IDS_BATCH_SIZE, filters::market_closed,
    http::query::GetGammaMarketsParams, providers::extract_condition_id,
    resolve::upsert_data_resolve_watch_entry_from_instrument,
};

#[derive(Debug)]
enum AutoLoadOutcome {
    Open(Vec<InstrumentAny>),
    Closed,
    Unknown,
}

struct AutoLoadScheduledGuard {
    scheduled: Arc<AtomicBool>,
    armed: bool,
}

impl AutoLoadScheduledGuard {
    fn new(scheduled: Arc<AtomicBool>) -> Self {
        Self {
            scheduled,
            armed: true,
        }
    }

    fn release(&mut self) {
        if self.armed {
            self.scheduled.store(false, Ordering::Release);
            self.armed = false;
        }
    }
}

impl Drop for AutoLoadScheduledGuard {
    fn drop(&mut self) {
        self.release();
    }
}

impl PolymarketDataClient {
    pub(super) fn queue_pending_load(&self, instrument_id: InstrumentId) {
        if extract_condition_id(&instrument_id).is_ok_and(|condition_id| {
            is_condition_closed(&self.closed_condition_ids, &condition_id)
        }) {
            return;
        }

        {
            let mut pending = self.pending_auto_loads.lock();
            pending.insert(instrument_id);
        }

        self.ensure_auto_load_task();
    }

    pub(super) fn drop_pending_if_unwanted(&self, instrument_id: InstrumentId) {
        if self.active_quote_subs.contains(&instrument_id)
            || self.active_delta_subs.contains(&instrument_id)
            || self.active_trade_subs.contains(&instrument_id)
            || self.active_instrument_status_subs.contains(&instrument_id)
            || self.active_instrument_close_subs.contains(&instrument_id)
        {
            return;
        }

        let mut pending = self.pending_auto_loads.lock();
        pending.remove(&instrument_id);
    }

    pub(super) fn drop_local_data_state_if_unwanted(&self, instrument_id: InstrumentId) {
        // Stale book/quote leaks across resubscribes
        if !self.active_delta_subs.contains(&instrument_id) {
            self.order_books.remove(&instrument_id);
        }

        if !self.active_quote_subs.contains(&instrument_id) {
            self.last_quotes.remove(&instrument_id);
        }
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
        let closed_condition_ids = self.closed_condition_ids.clone();
        let scheduled = self.auto_load_scheduled.clone();
        let debounce_ms = self.config.auto_load_debounce_ms;
        let max_retries = self.config.auto_load_max_retries;
        let base_secs = self.config.auto_load_retry_delay_initial_secs;
        let max_secs = self.config.auto_load_retry_delay_max_secs;
        let http = self.provider.http_client().clone();
        let filters = self.provider.filters();
        let instruments = self.instruments.clone();
        let instrument_update_state = self.instrument_update_state.clone();
        let token_meta = self.token_meta.clone();
        let active_quote_subs = self.active_quote_subs.clone();
        let active_delta_subs = self.active_delta_subs.clone();
        let active_trade_subs = self.active_trade_subs.clone();
        let active_instrument_status_subs = self.active_instrument_status_subs.clone();
        let active_instrument_close_subs = self.active_instrument_close_subs.clone();
        let ws_open_tokens = self.ws_open_tokens.clone();
        let ws_sub_mutex = self.ws_sub_mutex.clone();
        let ws_client = self.ws_client.handle();
        let data_sender = self.data_sender.clone();
        let cancellation = self.cancellation_token.clone();
        let order_books = self.order_books.clone();
        let last_quotes = self.last_quotes.clone();
        let resolve_poll_watchlist = self.resolve_poll_watchlist.clone();
        let pending_snapshot_after_tick_change = self.pending_snapshot_after_tick_change.clone();
        let scheduled_guard = AutoLoadScheduledGuard::new(scheduled);

        let future = async move {
            let mut scheduled_guard = scheduled_guard;

            // Coalesce concurrent misses into one Gamma call.
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(debounce_ms)) => {}
                () = cancellation.cancelled() => return,
            }

            // Drain pending and release `scheduled` so new misses spawn a fresh
            // task in parallel rather than piggybacking on this batch's budget.
            let mut batch: AHashSet<InstrumentId> = {
                let mut guard = pending.lock();
                let snapshot = guard.iter().copied().collect();
                guard.clear();
                snapshot
            };
            scheduled_guard.release();

            if batch.is_empty() {
                return;
            }

            log::debug!(
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
                        || active_instrument_status_subs.contains(id)
                        || active_instrument_close_subs.contains(id)
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

                // Gamma caps `condition_ids=` filters at ~100; classify every
                // condition independently before mutating runtime state.
                let mut outcomes: AHashMap<String, AutoLoadOutcome> = condition_ids
                    .iter()
                    .cloned()
                    .map(|condition_id| (condition_id, AutoLoadOutcome::Unknown))
                    .collect();
                let mut transient: AHashSet<String> = AHashSet::new();
                let mut failed_condition_ids: AHashSet<String> = AHashSet::new();
                let mut batch_returned_any = false;

                for chunk in condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
                    let params = GetGammaMarketsParams {
                        condition_ids: Some(chunk.to_vec()),
                        ..Default::default()
                    };

                    let normal_result = tokio::select! {
                        result = http.request_instruments_by_params_with_transient(params) => result,
                        () = cancellation.cancelled() => return,
                    };

                    if cancellation.is_cancelled() {
                        return;
                    }

                    match normal_result {
                        Ok((insts, trans)) => {
                            batch_returned_any |= !insts.is_empty() || !trans.is_empty();
                            let mut open_by_condition: AHashMap<String, Vec<InstrumentAny>> =
                                AHashMap::new();
                            let mut explicitly_closed = AHashSet::new();

                            for instrument in insts {
                                if let Ok(condition_id) = extract_condition_id(&instrument.id()) {
                                    if market_closed(&instrument) == Some(true) {
                                        explicitly_closed.insert(condition_id);
                                    } else {
                                        open_by_condition
                                            .entry(condition_id)
                                            .or_default()
                                            .push(instrument);
                                    }
                                }
                            }
                            let classified_condition_ids: AHashSet<String> = open_by_condition
                                .keys()
                                .cloned()
                                .chain(explicitly_closed.iter().cloned())
                                .collect();
                            let probe_condition_ids: Vec<String> = chunk
                                .iter()
                                .filter(|id| !classified_condition_ids.contains(*id))
                                .cloned()
                                .collect();

                            for (condition_id, instruments) in open_by_condition {
                                if !explicitly_closed.contains(&condition_id) {
                                    outcomes
                                        .insert(condition_id, AutoLoadOutcome::Open(instruments));
                                }
                            }

                            for condition_id in explicitly_closed {
                                outcomes.insert(condition_id, AutoLoadOutcome::Closed);
                            }
                            transient.extend(trans);

                            if probe_condition_ids.is_empty() {
                                continue;
                            }

                            let closed_result = tokio::select! {
                                result = query_positive_closed_condition_ids(
                                    &http,
                                    &probe_condition_ids,
                                ) => result,
                                () = cancellation.cancelled() => return,
                            };

                            if cancellation.is_cancelled() {
                                return;
                            }

                            match closed_result {
                                Ok(closed_ids) => {
                                    batch_returned_any |= !closed_ids.is_empty();
                                    for condition_id in closed_ids {
                                        outcomes.insert(condition_id, AutoLoadOutcome::Closed);
                                    }
                                }
                                Err(e) => {
                                    log::error!(
                                        "Auto-load closed-market probe failed for {} condition_id(s): {e:?}",
                                        probe_condition_ids.len(),
                                    );
                                    failed_condition_ids.extend(probe_condition_ids);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "Auto-load batch failed for chunk of {} condition_id(s): {e:?}",
                                chunk.len(),
                            );
                            failed_condition_ids.extend(chunk.iter().cloned());
                        }
                    }
                }

                if cancellation.is_cancelled() {
                    return;
                }

                for (condition_id, outcome) in &outcomes {
                    match outcome {
                        AutoLoadOutcome::Open(loaded) => {
                            if cancellation.is_cancelled() {
                                continue;
                            }

                            for instrument in loaded {
                                if !filters.iter().all(|f| f.accept(instrument)) {
                                    log::debug!(
                                        "Auto-loaded instrument {} filtered out",
                                        instrument.id()
                                    );
                                    continue;
                                }

                                let instrument_id = instrument.id();
                                apply_live_instrument(
                                    &closed_condition_ids,
                                    &instrument_update_state,
                                    &instruments,
                                    &token_meta,
                                    instrument,
                                    |instrument| {
                                        if let Err(e) = data_sender
                                            .send(DataEvent::Instrument(instrument.clone()))
                                        {
                                            log::error!(
                                                "Failed to emit auto-loaded instrument {instrument_id}: {e}"
                                            );
                                        }
                                    },
                                );

                                let status_subscribed =
                                    active_instrument_status_subs.contains(&instrument_id);
                                let close_subscribed =
                                    active_instrument_close_subs.contains(&instrument_id);

                                if status_subscribed || close_subscribed {
                                    let loaded = instruments.load();
                                    if let Some(instrument) = loaded.get(&instrument_id) {
                                        upsert_data_resolve_watch_entry_from_instrument(
                                            &resolve_poll_watchlist,
                                            instrument,
                                        );
                                    }
                                }
                            }
                        }
                        AutoLoadOutcome::Closed => {
                            if !register_closed_condition_for_live_data(
                                &closed_condition_ids,
                                &ws_sub_mutex,
                                condition_id,
                                Some(&cancellation),
                            )
                            .await
                            {
                                return;
                            }

                            if cancellation.is_cancelled() {
                                return;
                            }

                            // Registration above made the marker visible, so hold no guard here
                            publish_cached_condition_closed(
                                condition_id,
                                &instruments,
                                &data_sender,
                            );

                            retire_closed_condition_state(
                                condition_id,
                                batch.iter().copied(),
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
                                &pending,
                                &ws_open_tokens,
                                &ws_sub_mutex,
                                &ws_client,
                                Some(&cancellation),
                            )
                            .await;
                        }
                        AutoLoadOutcome::Unknown => {}
                    }
                }

                if cancellation.is_cancelled() {
                    return;
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
                let mut next_batch: AHashSet<InstrumentId> = AHashSet::new();

                for id in &batch {
                    if cancellation.is_cancelled() {
                        return;
                    }

                    let condition_id = match extract_condition_id(id) {
                        Ok(condition_id) => condition_id,
                        Err(_) => continue,
                    };

                    match outcomes.get(&condition_id) {
                        Some(AutoLoadOutcome::Open(_)) => {
                            if loaded_ids.contains(id)
                                && let Ok(token_id) = resolve_token_id_from(&instruments, *id)
                            {
                                sync_ws_subscription_with_resolution_and_terminal_async(
                                    *id,
                                    token_id,
                                    active_quote_subs.clone(),
                                    active_delta_subs.clone(),
                                    active_trade_subs.clone(),
                                    active_instrument_status_subs.clone(),
                                    active_instrument_close_subs.clone(),
                                    closed_condition_ids.clone(),
                                    ws_open_tokens.clone(),
                                    ws_sub_mutex.clone(),
                                    ws_client.clone(),
                                )
                                .await;
                            } else {
                                next_batch.insert(*id);
                            }
                        }
                        Some(AutoLoadOutcome::Closed) => {
                            // Terminal for live data; settlement metadata is
                            // retained by `retire_local_instrument_state`.
                        }
                        Some(AutoLoadOutcome::Unknown) | None => {
                            next_batch.insert(*id);
                        }
                    }
                }

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
                        let reason = extract_condition_id(id).map_or(
                            "invalid condition_id",
                            |condition_id| {
                                if failed_condition_ids.contains(&condition_id) {
                                    "Gamma fetch failed"
                                } else if transient.contains(&condition_id) {
                                    "no usable token_id (CLOB lifecycle race)"
                                } else {
                                    absent_reason
                                }
                            },
                        );

                        log::error!(
                            "Cannot find instrument for {id}: {reason} after {max_retries} retries"
                        );
                    }
                    return;
                }

                let delay =
                    crate::common::retry::auto_load_retry_delay(attempt, base_secs, max_secs);
                let kind = if next_batch.iter().any(|id| {
                    extract_condition_id(id)
                        .is_ok_and(|condition_id| failed_condition_ids.contains(&condition_id))
                }) {
                    "fetch failure or transient"
                } else {
                    "transient"
                };
                log::debug!(
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
        };

        if let Err(e) = self.tasks.spawn(future) {
            log::debug!("Skipping Polymarket data task after shutdown began: {e}");
        }
    }
}
