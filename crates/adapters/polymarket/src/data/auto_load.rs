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

use std::time::Duration;

use ahash::AHashSet;
use nautilus_common::{live::get_runtime, messages::DataEvent};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

use super::{
    PolymarketDataClient,
    instruments::cache_instrument,
    subscriptions::{resolve_token_id_from, sync_ws_subscription_async},
};
use crate::{
    common::consts::GAMMA_CONDITION_IDS_BATCH_SIZE,
    data_runtime::{is_instrument_expired, retire_local_instrument_state},
    http::query::GetGammaMarketsParams,
    providers::extract_condition_id,
};

impl PolymarketDataClient {
    pub(super) fn queue_pending_load(&self, instrument_id: InstrumentId) {
        {
            let mut pending = self
                .pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned");
            pending.insert(instrument_id);
        }

        self.ensure_auto_load_task();
    }

    pub(super) fn drop_pending_if_unwanted(&self, instrument_id: InstrumentId) {
        if self.active_quote_subs.contains(&instrument_id)
            || self.active_delta_subs.contains(&instrument_id)
            || self.active_trade_subs.contains(&instrument_id)
        {
            return;
        }

        let mut pending = self
            .pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned");
        pending.remove(&instrument_id);
    }

    pub(super) fn drop_local_book_state_if_unwanted(&self, instrument_id: InstrumentId) {
        // Stale book/quote leaks across resubscribes
        if self.active_quote_subs.contains(&instrument_id)
            || self.active_delta_subs.contains(&instrument_id)
        {
            return;
        }

        self.order_books.remove(&instrument_id);
        self.last_quotes.remove(&instrument_id);
    }

    fn ensure_auto_load_task(&self) {
        if self
            .auto_load_scheduled
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }

        let pending = self.pending_auto_loads.clone();
        let scheduled = self.auto_load_scheduled.clone();
        let debounce_ms = self.config.auto_load_debounce_ms;
        let max_retries = self.config.auto_load_max_retries;
        let base_secs = self.config.auto_load_retry_delay_initial_secs;
        let max_secs = self.config.auto_load_retry_delay_max_secs;
        let http = self.provider.http_client().clone();
        let filters = self.provider.filters();
        let instruments = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let active_quote_subs = self.active_quote_subs.clone();
        let active_delta_subs = self.active_delta_subs.clone();
        let active_trade_subs = self.active_trade_subs.clone();
        let ws_open_tokens = self.ws_open_tokens.clone();
        let ws_sub_mutex = self.ws_sub_mutex.clone();
        let ws_client = self.ws_client.clone_subscription_handle();
        let data_sender = self.data_sender.clone();
        let clock = self.clock;
        let cancellation = self.cancellation_token.clone();
        let order_books = self.order_books.clone();
        let last_quotes = self.last_quotes.clone();
        let resolve_poll_watchlist = self.resolve_poll_watchlist.clone();
        let pending_snapshot_after_tick_change = self.pending_snapshot_after_tick_change.clone();

        get_runtime().spawn(async move {
            // Coalesce concurrent misses into one Gamma call.
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(debounce_ms)) => {}
                () = cancellation.cancelled() => {
                    scheduled.store(false, std::sync::atomic::Ordering::Release);
                    return;
                }
            }

            // Drain pending and release `scheduled` so new misses spawn a fresh
            // task in parallel rather than piggybacking on this batch's budget.
            let mut batch: AHashSet<InstrumentId> = {
                let mut guard = pending.lock().expect("pending_auto_loads mutex poisoned");
                let snapshot = guard.iter().copied().collect();
                guard.clear();
                snapshot
            };
            scheduled.store(false, std::sync::atomic::Ordering::Release);

            if batch.is_empty() {
                return;
            }

            log::info!(
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

                // Gamma caps `condition_ids=` filters at ~100; chunk and merge.
                let mut loaded: Vec<InstrumentAny> = Vec::new();
                let mut transient: AHashSet<String> = AHashSet::new();
                let mut batch_returned_any = false;
                let mut chunk_failed = false;

                for chunk in condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
                    let params = GetGammaMarketsParams {
                        condition_ids: Some(chunk.join(",")),
                        ..Default::default()
                    };

                    match http
                        .request_instruments_by_params_with_transient(params)
                        .await
                    {
                        Ok((insts, trans)) => {
                            batch_returned_any |= !insts.is_empty() || !trans.is_empty();
                            loaded.extend(insts);
                            transient.extend(trans);
                        }
                        Err(e) => {
                            log::error!(
                                "Auto-load batch failed for chunk of {} condition_id(s): {e:?}",
                                chunk.len(),
                            );
                            chunk_failed = true;
                            break;
                        }
                    }
                }

                // A chunk failure leaves the batch's state unknown; count it
                // against the retry budget instead of dropping the subscription.
                let next_batch: AHashSet<InstrumentId> = if chunk_failed {
                    batch.clone()
                } else {
                    let mut retired_expired_ids = AHashSet::new();

                    for inst in loaded {
                        if !filters.iter().all(|f| f.accept(&inst)) {
                            log::debug!("Auto-loaded instrument {} filtered out", inst.id());
                            continue;
                        }

                        if is_instrument_expired(&inst, clock.get_time_ns()) {
                            log::debug!("Skipping expired auto-loaded instrument {}", inst.id());
                            retired_expired_ids.insert(inst.id());
                            retire_local_instrument_state(
                                inst.id(),
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
                            )
                            .await;
                            continue;
                        }

                        cache_instrument(&instruments, &token_meta, &inst);

                        let instrument_id = inst.id();
                        if let Err(e) = data_sender.send(DataEvent::Instrument(inst)) {
                            log::error!(
                                "Failed to emit auto-loaded instrument {instrument_id}: {e}"
                            );
                        }
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
                    let mut next: AHashSet<InstrumentId> = AHashSet::new();

                    for id in &batch {
                        let cid = match extract_condition_id(id) {
                            Ok(c) => c,
                            Err(_) => continue,
                        };

                        if loaded_ids.contains(id) {
                            if let Ok(token_id) = resolve_token_id_from(&instruments, *id) {
                                sync_ws_subscription_async(
                                    *id,
                                    token_id,
                                    active_quote_subs.clone(),
                                    active_delta_subs.clone(),
                                    active_trade_subs.clone(),
                                    ws_open_tokens.clone(),
                                    ws_sub_mutex.clone(),
                                    ws_client.clone(),
                                )
                                .await;
                            }
                        } else if retired_expired_ids.contains(id) {
                            // Expired instruments are terminal for live auto-load:
                            // retire any residual runtime state and stop retrying.
                        } else if transient.contains(&cid) {
                            // CLOB still hydrating: retry within the budget.
                            next.insert(*id);
                        } else {
                            // Absent from bulk response (same observable state as a
                            // 404 in the single-market path): also transient.
                            next.insert(*id);
                        }
                    }

                    next
                };

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
                        let reason = if chunk_failed {
                            "Gamma fetch failed"
                        } else if extract_condition_id(id)
                            .is_ok_and(|condition_id| transient.contains(&condition_id))
                        {
                            "no usable token_id (CLOB lifecycle race)"
                        } else {
                            absent_reason
                        };

                        log::error!(
                            "Cannot find instrument for {id}: {reason} after {max_retries} retries"
                        );
                    }
                    return;
                }

                let delay =
                    crate::common::retry::auto_load_retry_delay(attempt, base_secs, max_secs);
                let kind = if chunk_failed {
                    "chunk failure"
                } else {
                    "transient"
                };
                log::info!(
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
        });
    }
}
