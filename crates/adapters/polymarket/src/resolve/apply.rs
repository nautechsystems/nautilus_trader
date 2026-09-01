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
use nautilus_common::messages::DataEvent;
use nautilus_core::{AtomicMap, AtomicSet, time::AtomicTime};
use nautilus_model::{
    data::{Data as NautilusData, InstrumentClose, InstrumentStatus},
    enums::{InstrumentCloseType, MarketStatusAction},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    types::Price,
};
use parking_lot::Mutex;
use ustr::Ustr;

use super::{
    parsing::{
        StrictResolvedMarket, build_resolved_market_from_clob_market, build_strict_resolved_market,
    },
    watchlist::{ResolveWatchEntry, TrackedInstrument},
};
use crate::{
    common::consts::GAMMA_CONDITION_IDS_BATCH_SIZE,
    http::{
        clob::PolymarketClobPublicClient, gamma::PolymarketGammaHttpClient,
        query::GetGammaMarketsParams,
    },
};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ResolveApplyBatchStats {
    pub(crate) fetched_markets: usize,
    pub(crate) resolved_markets: usize,
    pub(crate) skipped_non_binary_markets: usize,
    pub(crate) clob_fallback_successes: usize,
    pub(crate) emitted_condition_ids: Vec<String>,
    pub(crate) failed_condition_ids: Vec<String>,
    pub(crate) error: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResolveBatchErrorMode {
    Continue,
    StopOnFirstError,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingResolution {
    pub(crate) winning_asset_id: String,
    pub(crate) winning_outcome: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolveApplyResult {
    Applied { emitted_closes: usize },
    Deferred,
    Ignored,
}

#[derive(Clone)]
pub(crate) struct ResolveContext {
    pub(crate) clock: &'static AtomicTime,
    pub(crate) data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    pub(crate) instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    pub(crate) watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    pub(crate) apply_mutex: Arc<Mutex<()>>,
    pub(crate) active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) active_status_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) active_close_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) closed_condition_ids: Arc<Mutex<AHashSet<String>>>,
    pub(crate) ws_open_tokens: Arc<AtomicSet<Ustr>>,
    pub(crate) ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    pub(crate) ws: crate::websocket::pool::PolymarketMarketPoolHandle,
    pub(crate) pending_resolutions: Arc<DashMap<String, PendingResolution>>,
}

pub(crate) async fn fetch_and_apply_resolutions_by_condition_ids(
    gamma_client: &PolymarketGammaHttpClient,
    clob_public_client: &PolymarketClobPublicClient,
    ctx: &ResolveContext,
    condition_ids: &[String],
    error_mode: ResolveBatchErrorMode,
) -> ResolveApplyBatchStats {
    let mut stats = ResolveApplyBatchStats::default();
    let mut unique_condition_ids = condition_ids.to_vec();
    unique_condition_ids.sort();
    unique_condition_ids.dedup();

    for chunk in unique_condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
        let mut unresolved_in_chunk: Vec<String> = chunk.to_vec();
        let params = GetGammaMarketsParams {
            condition_ids: Some(chunk.to_vec()),
            closed: Some(true),
            ..Default::default()
        };

        match gamma_client.request_markets_by_params(params).await {
            Ok(markets) => {
                stats.fetched_markets += markets.len();
                let mut skipped_in_chunk = 0;
                let resolved_by_condition = markets
                    .into_iter()
                    .filter_map(|market| match build_strict_resolved_market(&market) {
                        Some(resolved) => Some((resolved.condition_id.clone(), resolved)),
                        None => {
                            skipped_in_chunk += 1;
                            None
                        }
                    })
                    .collect::<ahash::AHashMap<String, StrictResolvedMarket>>();
                stats.skipped_non_binary_markets += skipped_in_chunk;

                for condition_id in chunk {
                    let Some(resolved) = resolved_by_condition.get(condition_id) else {
                        continue;
                    };

                    stats.resolved_markets += 1;
                    let result = apply_condition_resolution(
                        ctx,
                        &resolved.condition_id,
                        &resolved.winning_asset_id,
                        &resolved.winning_outcome,
                    )
                    .await;

                    if let ResolveApplyResult::Applied { emitted_closes } = result
                        && emitted_closes > 0
                    {
                        stats
                            .emitted_condition_ids
                            .push(resolved.condition_id.clone());
                    }
                }

                unresolved_in_chunk
                    .retain(|condition_id| !resolved_by_condition.contains_key(condition_id));
            }
            Err(e) => {
                let message = format!(
                    "Resolve request failed for {} condition_id(s): {e}",
                    chunk.len()
                );
                log::warn!("{message}");
            }
        }

        for condition_id in unresolved_in_chunk {
            match clob_public_client.get_market(&condition_id).await {
                Ok(market) => {
                    let Some(resolved) = build_resolved_market_from_clob_market(&market) else {
                        continue;
                    };

                    log::debug!(
                        "Resolve fallback via CLOB succeeded for condition_id={}",
                        resolved.condition_id
                    );
                    stats.clob_fallback_successes += 1;
                    stats.resolved_markets += 1;
                    let result = apply_condition_resolution(
                        ctx,
                        &resolved.condition_id,
                        &resolved.winning_asset_id,
                        &resolved.winning_outcome,
                    )
                    .await;

                    if let ResolveApplyResult::Applied { emitted_closes } = result
                        && emitted_closes > 0
                    {
                        stats
                            .emitted_condition_ids
                            .push(resolved.condition_id.clone());
                    }
                }
                Err(e) => {
                    let message = format!(
                        "Resolve fallback via CLOB failed for condition_id={condition_id}: {e}"
                    );
                    log::warn!("{message}");
                    if stats.error.is_none() {
                        stats.error = Some(message);
                    }
                    stats.failed_condition_ids.push(condition_id);
                }
            }
        }

        if error_mode == ResolveBatchErrorMode::StopOnFirstError
            && !stats.failed_condition_ids.is_empty()
        {
            break;
        }
    }

    stats.failed_condition_ids.sort();
    stats.failed_condition_ids.dedup();
    stats.emitted_condition_ids.sort();
    stats.emitted_condition_ids.dedup();

    if !unique_condition_ids.is_empty() {
        log::debug!(
            "Polymarket resolve batch requested={} fetched={} resolved={} emitted={} skipped_non_binary={} clob_fallback_successes={} failed={}",
            unique_condition_ids.len(),
            stats.fetched_markets,
            stats.resolved_markets,
            stats.emitted_condition_ids.len(),
            stats.skipped_non_binary_markets,
            stats.clob_fallback_successes,
            stats.failed_condition_ids.len(),
        );
    }

    stats
}

pub(crate) async fn apply_condition_resolution(
    ctx: &ResolveContext,
    condition_id: &str,
    winning_asset_id: &str,
    winning_outcome: &str,
) -> ResolveApplyResult {
    let condition_id_string = condition_id.to_string();
    if !ctx.watchlist.contains_key(&condition_id_string) {
        let no_watch_result = {
            let _guard = ctx.apply_mutex.lock();
            if ctx.watchlist.contains_key(&condition_id_string) {
                None
            } else {
                let has_resolution_intent = [&ctx.active_status_subs, &ctx.active_close_subs]
                    .into_iter()
                    .any(|subscriptions| {
                        subscriptions.load().iter().any(|instrument_id| {
                            crate::providers::extract_condition_id(instrument_id)
                                .is_ok_and(|candidate| candidate == condition_id)
                        })
                    });

                if has_resolution_intent {
                    ctx.pending_resolutions.insert(
                        condition_id_string.clone(),
                        PendingResolution {
                            winning_asset_id: winning_asset_id.to_string(),
                            winning_outcome: winning_outcome.to_string(),
                        },
                    );
                    Some(ResolveApplyResult::Deferred)
                } else {
                    ctx.pending_resolutions.remove(&condition_id_string);
                    Some(ResolveApplyResult::Ignored)
                }
            }
        };

        if let Some(result) = no_watch_result {
            if result == ResolveApplyResult::Ignored {
                log::debug!(
                    "Ignoring resolution for condition_id={condition_id}: no local watch entry"
                );
            }
            return result;
        }
    }

    let reconcile_guard = ctx.ws_sub_mutex.lock().await;
    let (reconciliation_targets, emitted_closes) = {
        let _guard = ctx.apply_mutex.lock();
        let Some(entry) = ctx.watchlist.get_cloned(&condition_id_string) else {
            log::debug!(
                "Ignoring resolution for condition_id={condition_id}: no local watch entry"
            );
            return ResolveApplyResult::Ignored;
        };

        let active_resolution_ids: AHashSet<InstrumentId> =
            [&ctx.active_status_subs, &ctx.active_close_subs]
                .into_iter()
                .flat_map(|subscriptions| subscriptions.load().iter().copied().collect::<Vec<_>>())
                .filter(|instrument_id| {
                    crate::providers::extract_condition_id(instrument_id)
                        .is_ok_and(|candidate| candidate == condition_id)
                })
                .collect();
        let tracked_ids: AHashSet<InstrumentId> = entry
            .tracked
            .values()
            .map(|tracked| tracked.instrument_id)
            .collect();

        if !active_resolution_ids.is_subset(&tracked_ids) {
            ctx.pending_resolutions.insert(
                condition_id_string,
                PendingResolution {
                    winning_asset_id: winning_asset_id.to_string(),
                    winning_outcome: winning_outcome.to_string(),
                },
            );
            return ResolveApplyResult::Deferred;
        }

        if entry.tracked.is_empty() {
            ctx.watchlist.remove(&condition_id_string);
            ctx.pending_resolutions.remove(&condition_id_string);
            return ResolveApplyResult::Ignored;
        }

        let ts_init = ctx.clock.get_time_ns();
        let reason = Ustr::from(&format!("Winner: {winning_asset_id} ({winning_outcome})"));
        let tracked_instruments: Vec<TrackedInstrument> = entry.tracked.values().cloned().collect();
        let mut emitted_closes = 0;

        for tracked in &tracked_instruments {
            let position_owned = !tracked.open_position_ids.is_empty();
            if position_owned || ctx.active_status_subs.contains(&tracked.instrument_id) {
                let status = InstrumentStatus::new(
                    tracked.instrument_id,
                    MarketStatusAction::Close,
                    ts_init,
                    ts_init,
                    Some(reason),
                    None,
                    Some(false),
                    None,
                    None,
                );

                if let Err(e) = ctx.data_sender.send(DataEvent::InstrumentStatus(status)) {
                    log::error!(
                        "Failed to emit instrument status for {}: {e}",
                        tracked.instrument_id
                    );
                    ctx.pending_resolutions.remove(&condition_id_string);
                    return ResolveApplyResult::Ignored;
                }
            }

            if !(position_owned || ctx.active_close_subs.contains(&tracked.instrument_id)) {
                continue;
            }

            let close_price = if tracked.token_id == winning_asset_id {
                Price::from_decimal_dp(rust_decimal::Decimal::ONE, tracked.price_precision)
                    .expect("valid decimal close price")
            } else {
                Price::from_decimal_dp(rust_decimal::Decimal::ZERO, tracked.price_precision)
                    .expect("valid decimal close price")
            };
            let close = InstrumentClose::new(
                tracked.instrument_id,
                close_price,
                InstrumentCloseType::ContractExpired,
                ts_init,
                ts_init,
            );

            if let Err(e) = ctx
                .data_sender
                .send(DataEvent::Data(NautilusData::InstrumentClose(close)))
            {
                log::error!(
                    "Failed to emit instrument close for {}: {e}",
                    tracked.instrument_id
                );
                ctx.pending_resolutions.remove(&condition_id_string);
                return ResolveApplyResult::Ignored;
            }
            emitted_closes += 1;
        }

        let mut reconciliation_targets: AHashMap<InstrumentId, String> = tracked_instruments
            .iter()
            .map(|tracked| (tracked.instrument_id, tracked.token_id.clone()))
            .collect();
        let loaded = ctx.instruments.load();

        for subscriptions in [
            &ctx.active_quote_subs,
            &ctx.active_delta_subs,
            &ctx.active_trade_subs,
            &ctx.active_status_subs,
            &ctx.active_close_subs,
        ] {
            for instrument_id in subscriptions.load().iter().copied() {
                if !crate::providers::extract_condition_id(&instrument_id)
                    .is_ok_and(|candidate| candidate == condition_id)
                {
                    continue;
                }

                let token_id = loaded
                    .get(&instrument_id)
                    .map(|instrument| instrument.raw_symbol().as_str().to_string())
                    .or_else(|| {
                        instrument_id
                            .symbol
                            .as_str()
                            .rsplit_once('-')
                            .map(|(_, token_id)| token_id.to_string())
                    });

                if let Some(token_id) = token_id {
                    reconciliation_targets
                        .entry(instrument_id)
                        .or_insert(token_id);
                }
            }
        }

        ctx.watchlist.remove(&condition_id_string);
        for subscriptions in [&ctx.active_status_subs, &ctx.active_close_subs] {
            subscriptions.rcu(|entries| {
                entries.retain(|instrument_id| {
                    !crate::providers::extract_condition_id(instrument_id)
                        .is_ok_and(|candidate| candidate == condition_id)
                });
            });
        }
        let newly_closed = ctx
            .closed_condition_ids
            .lock()
            .insert(condition_id_string.clone());

        if newly_closed {
            log::info!("Market resolved for condition {condition_id}, reconciling live data state");
        }
        ctx.pending_resolutions.remove(&condition_id_string);

        (reconciliation_targets, emitted_closes)
    };
    drop(reconcile_guard);

    let mut reconciliation_targets: Vec<(InstrumentId, String)> =
        reconciliation_targets.into_iter().collect();
    reconciliation_targets.sort_unstable_by(|left, right| left.1.cmp(&right.1));

    for (instrument_id, token_id) in reconciliation_targets {
        crate::data::sync_ws_subscription_with_resolution_and_terminal_async(
            instrument_id,
            token_id,
            ctx.active_quote_subs.clone(),
            ctx.active_delta_subs.clone(),
            ctx.active_trade_subs.clone(),
            ctx.active_status_subs.clone(),
            ctx.active_close_subs.clone(),
            ctx.closed_condition_ids.clone(),
            ctx.ws_open_tokens.clone(),
            ctx.ws_sub_mutex.clone(),
            ctx.ws.clone(),
        )
        .await;
    }

    ResolveApplyResult::Applied { emitted_closes }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ahash::AHashSet;
    use nautilus_common::messages::DataEvent;
    use nautilus_core::{AtomicMap, UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::identifiers::InstrumentId;
    use parking_lot::Mutex;
    use rstest::rstest;

    use super::*;

    fn make_resolve_context() -> (
        ResolveContext,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (ws_tx, _ws_rx) = tokio::sync::mpsc::unbounded_channel();
        let ctx = ResolveContext {
            clock: get_atomic_clock_realtime(),
            data_sender: data_tx,
            instruments: Arc::new(AtomicMap::new()),
            watchlist: Arc::new(AtomicMap::new()),
            apply_mutex: Arc::new(Mutex::new(())),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_delta_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            active_status_subs: Arc::new(AtomicSet::new()),
            active_close_subs: Arc::new(AtomicSet::new()),
            closed_condition_ids: Arc::new(Mutex::new(AHashSet::new())),
            ws_open_tokens: Arc::new(AtomicSet::new()),
            ws_sub_mutex: Arc::new(tokio::sync::Mutex::new(())),
            ws: crate::websocket::pool::PolymarketMarketPoolHandle::test_single_shard(ws_tx, &[]),
            pending_resolutions: Arc::new(DashMap::new()),
        };

        (ctx, data_rx)
    }

    #[rstest]
    #[tokio::test]
    async fn data_status_subscription_emits_status_without_close() {
        let (ctx, mut data_rx) = make_resolve_context();
        ctx.active_status_subs
            .insert(InstrumentId::from("0xCOND-0xYES.POLYMARKET"));
        ctx.watchlist.insert(
            "0xCOND".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND".to_string(),
                expiration_ns: UnixNanos::default(),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        instrument_id: InstrumentId::from("0xCOND-0xYES.POLYMARKET"),
                        token_id: "0xYES".to_string(),
                        price_precision: 3,
                        open_position_ids: AHashSet::new(),
                        has_data_subscription: true,
                    },
                )]),
                paused: false,
            },
        );

        assert_eq!(
            apply_condition_resolution(&ctx, "0xCOND", "0xYES", "Yes").await,
            ResolveApplyResult::Applied { emitted_closes: 0 }
        );
        assert!(matches!(
            data_rx.try_recv(),
            Ok(DataEvent::InstrumentStatus(_))
        ));
        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn resolution_without_watch_entry_preserves_unrelated_data_subscription_intent() {
        let (ctx, _data_rx) = make_resolve_context();
        let instrument_id = InstrumentId::from("0xCOND-0xYES.POLYMARKET");
        ctx.active_status_subs.insert(instrument_id);
        ctx.active_close_subs.insert(instrument_id);

        assert_eq!(
            apply_condition_resolution(&ctx, "0xCOND", "0xYES", "Yes").await,
            ResolveApplyResult::Deferred
        );

        assert!(ctx.active_status_subs.contains(&instrument_id));
        assert!(ctx.active_close_subs.contains(&instrument_id));
        assert!(ctx.pending_resolutions.contains_key("0xCOND"));
    }
}
