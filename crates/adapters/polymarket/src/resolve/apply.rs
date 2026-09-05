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
use dashmap::{DashMap, mapref::entry::Entry};
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
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    parsing::{
        StrictResolvedMarket, build_resolved_market_from_clob_market, build_strict_resolved_market,
    },
    watchlist::{ResolveWatchEntry, TrackedInstrument},
};
use crate::{
    common::consts::{GAMMA_CONDITION_IDS_BATCH_SIZE, POLYMARKET_PRICE_PRECISION},
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

#[derive(Clone, Debug)]
pub(crate) struct PendingResolution {
    pub(crate) winning_asset_id: String,
    pub(crate) winning_outcome: String,
    claim_id: Arc<()>,
    claims: usize,
}

pub(crate) struct PendingResolutionGuard {
    pending_resolutions: Arc<DashMap<String, PendingResolution>>,
    condition_id: String,
    claim_id: Arc<()>,
}

impl PendingResolutionGuard {
    // WebSocket dispatch claims exclusively to deduplicate tasks, while application
    // joins matching claims so one caller cannot clear another caller's barrier.
    pub(crate) fn try_claim(
        pending_resolutions: Arc<DashMap<String, PendingResolution>>,
        condition_id: String,
        winning_asset_id: &str,
        winning_outcome: &str,
        share_existing: bool,
    ) -> Option<Self> {
        let claim_id = match pending_resolutions.entry(condition_id.clone()) {
            Entry::Occupied(mut entry) => {
                if !share_existing {
                    return None;
                }
                let current = entry.get_mut();

                if current.winning_asset_id != winning_asset_id
                    || current.winning_outcome != winning_outcome
                {
                    log::warn!(
                        "Ignoring conflicting resolution for condition_id={condition_id}: existing winner={} ({}) received winner={winning_asset_id} ({winning_outcome})",
                        current.winning_asset_id,
                        current.winning_outcome,
                    );
                    return None;
                }
                current.claims += 1;
                current.claim_id.clone()
            }
            Entry::Vacant(entry) => {
                let claim_id = Arc::new(());
                entry.insert(PendingResolution {
                    winning_asset_id: winning_asset_id.to_string(),
                    winning_outcome: winning_outcome.to_string(),
                    claim_id: claim_id.clone(),
                    claims: 1,
                });
                claim_id
            }
        };

        Some(Self {
            pending_resolutions,
            condition_id,
            claim_id,
        })
    }
}

impl Drop for PendingResolutionGuard {
    fn drop(&mut self) {
        let Entry::Occupied(mut entry) = self.pending_resolutions.entry(self.condition_id.clone())
        else {
            return;
        };

        if !Arc::ptr_eq(&entry.get().claim_id, &self.claim_id) {
            return;
        }

        let current = entry.get_mut();
        current.claims -= 1;

        if current.claims == 0 {
            entry.remove();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolveApplyResult {
    Applied { emitted_closes: usize },
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
    pub(crate) deferred_resolutions: Arc<AtomicMap<InstrumentId, StrictResolvedMarket>>,
    pub(crate) subscribe_new_markets: bool,
    pub(crate) cancellation_token: CancellationToken,
}

pub(crate) async fn pause_and_reconcile_resolve_watch_entries(
    ctx: &ResolveContext,
    condition_ids: &[String],
) {
    let mut targets = {
        let _guard = ctx.apply_mutex.lock();
        super::watchlist::pause_resolve_watch_entries(&ctx.watchlist, condition_ids);

        // Include already paused entries so interrupted cleanup converges after reconnect
        ctx.watchlist
            .load()
            .values()
            .filter(|entry| entry.paused)
            .flat_map(|entry| entry.tracked.values())
            .filter(|tracked| {
                ctx.ws_open_tokens
                    .contains(&Ustr::from(tracked.token_id.as_str()))
            })
            .map(|tracked| (tracked.instrument_id, tracked.token_id.clone()))
            .collect::<Vec<_>>()
    };
    targets.sort_unstable_by(|left, right| left.1.cmp(&right.1));

    for (instrument_id, token_id) in targets {
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
            ctx.watchlist.clone(),
            ctx.subscribe_new_markets,
        )
        .await;
    }
}

pub(crate) async fn fetch_and_apply_resolutions_by_condition_ids(
    gamma_client: &PolymarketGammaHttpClient,
    clob_public_client: &PolymarketClobPublicClient,
    ctx: &ResolveContext,
    condition_ids: &[String],
    error_mode: ResolveBatchErrorMode,
    include_pending_intents: bool,
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
                    let result =
                        apply_fetched_resolution(ctx, resolved, include_pending_intents).await;

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
                    let result =
                        apply_fetched_resolution(ctx, &resolved, include_pending_intents).await;

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

async fn apply_fetched_resolution(
    ctx: &ResolveContext,
    resolved: &StrictResolvedMarket,
    include_pending_intents: bool,
) -> ResolveApplyResult {
    if include_pending_intents {
        apply_condition_resolution(
            ctx,
            &resolved.condition_id,
            &resolved.winning_asset_id,
            &resolved.winning_outcome,
        )
        .await
    } else {
        apply_watched_condition_resolution(ctx, resolved).await
    }
}

pub(crate) async fn apply_condition_resolution(
    ctx: &ResolveContext,
    condition_id: &str,
    winning_asset_id: &str,
    winning_outcome: &str,
) -> ResolveApplyResult {
    apply_condition_resolution_with_owners(
        ctx,
        condition_id,
        winning_asset_id,
        winning_outcome,
        ResolveOwnerSelection::IncludePendingIntents,
    )
    .await
}

pub(crate) async fn apply_condition_resolution_with_assets(
    ctx: &ResolveContext,
    condition_id: &str,
    winning_asset_id: &str,
    winning_outcome: &str,
    asset_ids: &[String],
) -> ResolveApplyResult {
    apply_condition_resolution_with_owners(
        ctx,
        condition_id,
        winning_asset_id,
        winning_outcome,
        ResolveOwnerSelection::PayloadAssets(asset_ids),
    )
    .await
}

// Auto-load admits data owners only after parsing and filtering, while existing
// watched data and position owners can settle even if this payload is unusable.
pub(crate) async fn apply_watched_condition_resolution(
    ctx: &ResolveContext,
    resolution: &StrictResolvedMarket,
) -> ResolveApplyResult {
    apply_condition_resolution_with_owners(
        ctx,
        &resolution.condition_id,
        &resolution.winning_asset_id,
        &resolution.winning_outcome,
        ResolveOwnerSelection::Watched(&resolution.asset_ids),
    )
    .await
}

#[derive(Clone, Copy)]
enum ResolveOwnerSelection<'a> {
    Watched(&'a [String]),
    IncludePendingIntents,
    PayloadAssets(&'a [String]),
    AdmittedInstrument(&'a InstrumentAny),
}

// Called only after auto-load has parsed and filtered this instrument. A known
// outcome is replayed without recreating live routing or a terminal position watch.
pub(crate) async fn admit_data_resolution_instrument(
    ctx: &ResolveContext,
    instrument: &InstrumentAny,
) {
    let resolution = {
        let _guard = ctx.apply_mutex.lock();
        if ctx.cancellation_token.is_cancelled()
            || (!ctx.active_status_subs.contains(&instrument.id())
                && !ctx.active_close_subs.contains(&instrument.id()))
        {
            return;
        }
        let resolution = ctx.deferred_resolutions.get_cloned(&instrument.id());
        if resolution.is_none() {
            super::watchlist::upsert_data_resolve_watch_entry_from_instrument(
                &ctx.watchlist,
                instrument,
            );
        }
        resolution
    };

    if let Some(resolution) = resolution {
        apply_condition_resolution_with_owners(
            ctx,
            &resolution.condition_id,
            &resolution.winning_asset_id,
            &resolution.winning_outcome,
            ResolveOwnerSelection::AdmittedInstrument(instrument),
        )
        .await;
    }
}

async fn apply_condition_resolution_with_owners(
    ctx: &ResolveContext,
    condition_id: &str,
    winning_asset_id: &str,
    winning_outcome: &str,
    owner_selection: ResolveOwnerSelection<'_>,
) -> ResolveApplyResult {
    if ctx.cancellation_token.is_cancelled() {
        return ResolveApplyResult::Ignored;
    }

    let Some(_pending_guard) = PendingResolutionGuard::try_claim(
        ctx.pending_resolutions.clone(),
        condition_id.to_string(),
        winning_asset_id,
        winning_outcome,
        true,
    ) else {
        return ResolveApplyResult::Ignored;
    };

    apply_condition_resolution_inner(
        ctx,
        condition_id,
        winning_asset_id,
        winning_outcome,
        owner_selection,
    )
    .await
}

async fn apply_condition_resolution_inner(
    ctx: &ResolveContext,
    condition_id: &str,
    winning_asset_id: &str,
    winning_outcome: &str,
    owner_selection: ResolveOwnerSelection<'_>,
) -> ResolveApplyResult {
    let condition_id_string = condition_id.to_string();
    let reconcile_guard = tokio::select! {
        () = ctx.cancellation_token.cancelled() => return ResolveApplyResult::Ignored,
        guard = ctx.ws_sub_mutex.lock() => guard,
    };
    let (reconciliation_targets, emitted_closes) = {
        let _guard = ctx.apply_mutex.lock();

        // Reset takes the same lock, so an old context cannot publish after reset returns
        if ctx.cancellation_token.is_cancelled() {
            return ResolveApplyResult::Ignored;
        }

        let entry = ctx.watchlist.get_cloned(&condition_id_string);

        // Ownership can grow while waiting for reconciliation, so validate the retained
        // payload again against every known leg before emitting or removing any owner.
        if let ResolveOwnerSelection::PayloadAssets(asset_ids) = owner_selection
            && entry.as_ref().is_some_and(|entry| {
                entry
                    .tracked
                    .values()
                    .any(|tracked| !asset_ids.contains(&tracked.token_id))
            })
        {
            log::warn!(
                "Ignoring resolution for condition_id={condition_id}: payload assets conflict with known resolution owners"
            );
            return ResolveApplyResult::Ignored;
        }

        let matches_condition = |instrument_id: &InstrumentId| {
            crate::providers::extract_condition_id(instrument_id)
                .is_ok_and(|candidate| candidate == condition_id)
        };
        let watched_instrument_ids: AHashSet<InstrumentId> = entry
            .iter()
            .flat_map(|entry| entry.tracked.values())
            .map(|tracked| tracked.instrument_id)
            .collect();
        let active_status_ids: AHashSet<InstrumentId> = ctx
            .active_status_subs
            .load()
            .iter()
            .filter(|instrument_id| {
                matches_condition(instrument_id) || watched_instrument_ids.contains(instrument_id)
            })
            .copied()
            .collect();
        let active_close_ids: AHashSet<InstrumentId> = ctx
            .active_close_subs
            .load()
            .iter()
            .filter(|instrument_id| {
                matches_condition(instrument_id) || watched_instrument_ids.contains(instrument_id)
            })
            .copied()
            .collect();

        if entry.is_none() && active_status_ids.is_empty() && active_close_ids.is_empty() {
            log::debug!(
                "Ignoring resolution for condition_id={condition_id}: no local resolution owner"
            );
            return ResolveApplyResult::Ignored;
        }

        let ts_init = ctx.clock.get_time_ns();
        let reason = Ustr::from(&format!("Winner: {winning_asset_id} ({winning_outcome})"));
        let loaded = ctx.instruments.load();
        let mut tracked_instruments: AHashMap<InstrumentId, TrackedInstrument> = entry
            .iter()
            .flat_map(|entry| entry.tracked.values())
            .map(|tracked| (tracked.instrument_id, tracked.clone()))
            .collect();

        if matches!(
            owner_selection,
            ResolveOwnerSelection::IncludePendingIntents
                | ResolveOwnerSelection::AdmittedInstrument(_)
        ) {
            for instrument_id in active_status_ids.union(&active_close_ids).copied() {
                if tracked_instruments.contains_key(&instrument_id) {
                    continue;
                }

                let admitted = match owner_selection {
                    ResolveOwnerSelection::AdmittedInstrument(instrument) => {
                        if instrument.id() != instrument_id {
                            continue;
                        }
                        Some(instrument)
                    }
                    _ => None,
                };
                let token_id = admitted
                    .or_else(|| loaded.get(&instrument_id))
                    .map(|instrument| instrument.raw_symbol().as_str().to_string())
                    .or_else(|| crate::providers::extract_token_id(&instrument_id).ok());
                let Some(token_id) = token_id else {
                    log::error!(
                        "Cannot apply resolution for {instrument_id}: token ID is unavailable"
                    );
                    continue;
                };

                let price_precision = admitted
                    .or_else(|| loaded.get(&instrument_id))
                    .map_or(POLYMARKET_PRICE_PRECISION, |instrument| {
                        instrument.price_precision()
                    });
                tracked_instruments.insert(
                    instrument_id,
                    TrackedInstrument {
                        instrument_id,
                        token_id,
                        price_precision,
                        open_position_ids: AHashSet::new(),
                        has_data_subscription: true,
                    },
                );
            }
        }

        let mut deferred_ids = active_status_ids
            .union(&active_close_ids)
            .filter(|id| {
                !tracked_instruments.contains_key(id) && ctx.deferred_resolutions.contains_key(id)
            })
            .copied()
            .collect::<AHashSet<_>>();

        if let ResolveOwnerSelection::Watched(asset_ids)
        | ResolveOwnerSelection::PayloadAssets(asset_ids) = owner_selection
        {
            for instrument_id in active_status_ids.union(&active_close_ids).copied() {
                if tracked_instruments.contains_key(&instrument_id)
                    || !crate::providers::extract_token_id(&instrument_id)
                        .is_ok_and(|token_id| asset_ids.contains(&token_id))
                {
                    continue;
                }
                deferred_ids.insert(instrument_id);
                ctx.deferred_resolutions.rcu(|entries| {
                    entries
                        .entry(instrument_id)
                        .or_insert_with(|| StrictResolvedMarket {
                            condition_id: condition_id_string.clone(),
                            asset_ids: asset_ids.to_vec(),
                            winning_asset_id: winning_asset_id.to_string(),
                            winning_outcome: winning_outcome.to_string(),
                        });
                });
            }
        }

        if tracked_instruments.is_empty() && deferred_ids.is_empty() {
            ctx.watchlist.remove(&condition_id_string);
            return ResolveApplyResult::Ignored;
        }

        if let ResolveOwnerSelection::AdmittedInstrument(instrument) = owner_selection
            && tracked_instruments.contains_key(&instrument.id())
        {
            let mut instrument = instrument.clone();
            if let InstrumentAny::BinaryOption(binary) = &mut instrument {
                crate::filters::set_market_closed(binary, true);
            }

            if let Err(e) = ctx.data_sender.send(DataEvent::Instrument(instrument)) {
                log::error!("Failed to emit admitted resolution instrument: {e}");
                return ResolveApplyResult::Ignored;
            }
        }

        let mut emitted_closes = 0;

        for tracked in tracked_instruments.values() {
            let position_owned = !tracked.open_position_ids.is_empty();
            if position_owned || active_status_ids.contains(&tracked.instrument_id) {
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
                    return ResolveApplyResult::Ignored;
                }
            }

            if !(position_owned || active_close_ids.contains(&tracked.instrument_id)) {
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
                return ResolveApplyResult::Ignored;
            }
            emitted_closes += 1;
        }

        let mut reconciliation_targets: AHashMap<InstrumentId, String> = tracked_instruments
            .values()
            .map(|tracked| (tracked.instrument_id, tracked.token_id.clone()))
            .collect();

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
                    .or_else(|| crate::providers::extract_token_id(&instrument_id).ok());

                if let Some(token_id) = token_id {
                    reconciliation_targets
                        .entry(instrument_id)
                        .or_insert(token_id);
                }
            }
        }

        // Clear settlement state before releasing the corresponding subscription intents
        ctx.watchlist.remove(&condition_id_string);

        for instrument_id in active_status_ids.union(&active_close_ids) {
            if !deferred_ids.contains(instrument_id) {
                ctx.deferred_resolutions.remove(instrument_id);
            }
        }

        for subscriptions in [&ctx.active_status_subs, &ctx.active_close_subs] {
            subscriptions.rcu(|entries| {
                entries.retain(|instrument_id| {
                    deferred_ids.contains(instrument_id)
                        || (!matches_condition(instrument_id)
                            && !tracked_instruments.contains_key(instrument_id))
                });
            });
        }

        for subscriptions in [
            &ctx.active_quote_subs,
            &ctx.active_delta_subs,
            &ctx.active_trade_subs,
        ] {
            subscriptions.rcu(|entries| {
                entries.retain(|instrument_id| {
                    !matches_condition(instrument_id)
                        && !tracked_instruments.contains_key(instrument_id)
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
        (reconciliation_targets, emitted_closes)
    };
    drop(reconcile_guard);

    let mut reconciliation_targets: Vec<(InstrumentId, String)> =
        reconciliation_targets.into_iter().collect();
    reconciliation_targets.sort_unstable_by(|left, right| left.1.cmp(&right.1));

    for (instrument_id, token_id) in reconciliation_targets {
        if !ctx.ws_open_tokens.contains(&Ustr::from(token_id.as_str())) {
            continue;
        }
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
            ctx.watchlist.clone(),
            ctx.subscribe_new_markets,
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
    use nautilus_model::identifiers::{InstrumentId, PositionId};
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
            deferred_resolutions: Arc::new(AtomicMap::new()),
            subscribe_new_markets: false,
            cancellation_token: CancellationToken::new(),
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
    fn pending_resolution_claim_rejects_duplicates_and_conflicting_outcomes() {
        let pending = Arc::new(DashMap::new());
        let claim = PendingResolutionGuard::try_claim(
            pending.clone(),
            "0xCOND".to_string(),
            "0xYES",
            "Yes",
            false,
        )
        .unwrap();

        for (winning_asset_id, winning_outcome, share_existing) in [
            ("0xYES", "Yes", false),
            ("0xNO", "No", true),
            ("0xYES", "No", true),
        ] {
            assert!(
                PendingResolutionGuard::try_claim(
                    pending.clone(),
                    "0xCOND".to_string(),
                    winning_asset_id,
                    winning_outcome,
                    share_existing,
                )
                .is_none()
            );
        }

        assert_eq!(pending.get("0xCOND").unwrap().winning_asset_id, "0xYES");
        drop(claim);
        assert!(pending.is_empty());
    }

    #[rstest]
    fn pending_resolution_guard_preserves_replacement_claim() {
        let pending = Arc::new(DashMap::new());
        let old_claim = PendingResolutionGuard::try_claim(
            pending.clone(),
            "0xCOND".to_string(),
            "0xYES",
            "Yes",
            false,
        )
        .unwrap();
        pending.clear();
        let replacement = PendingResolutionGuard::try_claim(
            pending.clone(),
            "0xCOND".to_string(),
            "0xYES",
            "Yes",
            false,
        )
        .unwrap();

        drop(old_claim);
        assert!(pending.contains_key("0xCOND"));
        drop(replacement);
        assert!(pending.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn resolution_without_watch_entry_completes_active_data_subscription_intent() {
        let (ctx, mut data_rx) = make_resolve_context();
        let instrument_id = InstrumentId::from("0xCOND-0xYES.POLYMARKET");
        ctx.active_status_subs.insert(instrument_id);
        ctx.active_close_subs.insert(instrument_id);

        assert_eq!(
            apply_condition_resolution(&ctx, "0xCOND", "0xYES", "Yes").await,
            ResolveApplyResult::Applied { emitted_closes: 1 }
        );

        assert!(!ctx.active_status_subs.contains(&instrument_id));
        assert!(!ctx.active_close_subs.contains(&instrument_id));
        assert!(!ctx.pending_resolutions.contains_key("0xCOND"));
        assert!(ctx.closed_condition_ids.lock().contains("0xCOND"));
        assert!(matches!(
            data_rx.try_recv(),
            Ok(DataEvent::InstrumentStatus(_))
        ));
        assert!(matches!(
            data_rx.try_recv(),
            Ok(DataEvent::Data(NautilusData::InstrumentClose(_)))
        ));
    }

    #[rstest]
    #[tokio::test]
    async fn direct_resolution_installs_pending_barrier_before_ws_reconciliation() {
        let (ctx, _data_rx) = make_resolve_context();
        let instrument_id = InstrumentId::from("0xCOND-0xYES.POLYMARKET");
        ctx.active_status_subs.insert(instrument_id);
        ctx.watchlist.insert(
            "0xCOND".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND".to_string(),
                expiration_ns: UnixNanos::default(),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        instrument_id,
                        token_id: "0xYES".to_string(),
                        price_precision: 3,
                        open_position_ids: AHashSet::new(),
                        has_data_subscription: true,
                    },
                )]),
                paused: false,
            },
        );

        let reconcile_guard = ctx.ws_sub_mutex.lock().await;
        let apply_ctx = ctx.clone();
        let apply_task = tokio::spawn(async move {
            apply_condition_resolution(&apply_ctx, "0xCOND", "0xYES", "Yes").await
        });
        tokio::task::yield_now().await;

        assert!(ctx.pending_resolutions.contains_key("0xCOND"));

        drop(reconcile_guard);
        assert_eq!(
            apply_task.await.expect("resolution task"),
            ResolveApplyResult::Applied { emitted_closes: 0 }
        );
        assert!(!ctx.pending_resolutions.contains_key("0xCOND"));
    }

    #[rstest]
    #[tokio::test]
    async fn deferred_auto_load_keeps_manual_resolution_pending_barrier() {
        let (ctx, mut data_rx) = make_resolve_context();
        let instrument_id = InstrumentId::from("0xCOND-0xYES.POLYMARKET");
        ctx.active_status_subs.insert(instrument_id);
        ctx.active_close_subs.insert(instrument_id);
        let resolution = StrictResolvedMarket {
            condition_id: "0xCOND".to_string(),
            asset_ids: vec!["0xYES".to_string(), "0xNO".to_string()],
            winning_asset_id: "0xYES".to_string(),
            winning_outcome: "Yes".to_string(),
        };
        let reconcile_guard = ctx.ws_sub_mutex.lock().await;
        let mut auto_load = Box::pin(apply_watched_condition_resolution(&ctx, &resolution));
        let mut manual = Box::pin(apply_condition_resolution(&ctx, "0xCOND", "0xYES", "Yes"));
        assert!(futures_util::poll!(&mut auto_load).is_pending());
        assert!(futures_util::poll!(&mut manual).is_pending());

        drop(reconcile_guard);
        assert_eq!(
            auto_load.await,
            ResolveApplyResult::Applied { emitted_closes: 0 }
        );
        assert!(ctx.deferred_resolutions.contains_key(&instrument_id));
        assert!(ctx.pending_resolutions.contains_key("0xCOND"));
        assert!(data_rx.try_recv().is_err());
        assert_eq!(
            manual.await,
            ResolveApplyResult::Applied { emitted_closes: 1 }
        );

        assert!(ctx.deferred_resolutions.is_empty());
        assert!(ctx.pending_resolutions.is_empty());
        assert!(ctx.closed_condition_ids.lock().contains("0xCOND"));
        assert_eq!(std::iter::from_fn(|| data_rx.try_recv().ok()).count(), 2);
    }

    #[rstest]
    #[case::first(true)]
    #[case::second(false)]
    #[tokio::test]
    async fn cancelling_overlapping_resolution_preserves_pending_barrier(
        #[case] cancel_first: bool,
    ) {
        let (ctx, mut data_rx) = make_resolve_context();
        let instrument_id = InstrumentId::from("0xCOND-0xYES.POLYMARKET");
        ctx.active_status_subs.insert(instrument_id);
        ctx.active_close_subs.insert(instrument_id);
        let reconcile_guard = ctx.ws_sub_mutex.lock().await;
        let mut first = Box::pin(apply_condition_resolution(&ctx, "0xCOND", "0xYES", "Yes"));
        let mut second = Box::pin(apply_condition_resolution(&ctx, "0xCOND", "0xYES", "Yes"));
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());

        let remaining = if cancel_first {
            drop(first);
            second
        } else {
            drop(second);
            first
        };

        assert!(ctx.pending_resolutions.contains_key("0xCOND"));
        assert!(data_rx.try_recv().is_err());
        drop(reconcile_guard);
        assert_eq!(
            remaining.await,
            ResolveApplyResult::Applied { emitted_closes: 1 }
        );

        assert!(ctx.pending_resolutions.is_empty());
        assert!(ctx.closed_condition_ids.lock().contains("0xCOND"));
        assert_eq!(std::iter::from_fn(|| data_rx.try_recv().ok()).count(), 2);
    }

    #[rstest]
    #[tokio::test]
    async fn resolution_clears_pending_barrier_when_watch_disappears() {
        let (ctx, _data_rx) = make_resolve_context();
        let instrument_id = InstrumentId::from("0xCOND-0xYES.POLYMARKET");
        ctx.watchlist.insert(
            "0xCOND".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND".to_string(),
                expiration_ns: UnixNanos::default(),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        instrument_id,
                        token_id: "0xYES".to_string(),
                        price_precision: 3,
                        open_position_ids: AHashSet::from_iter([PositionId::new("P-1")]),
                        has_data_subscription: false,
                    },
                )]),
                paused: false,
            },
        );
        let pending_guard = PendingResolutionGuard::try_claim(
            ctx.pending_resolutions.clone(),
            "0xCOND".to_string(),
            "0xYES",
            "Yes",
            false,
        )
        .expect("pending WebSocket claim");

        let reconcile_guard = ctx.ws_sub_mutex.lock().await;
        let apply_ctx = ctx.clone();

        let apply_task = tokio::spawn(async move {
            let _pending_guard = pending_guard;
            apply_condition_resolution(&apply_ctx, "0xCOND", "0xYES", "Yes").await
        });
        tokio::task::yield_now().await;
        ctx.watchlist.remove(&"0xCOND".to_string());
        drop(reconcile_guard);

        assert_eq!(
            apply_task.await.expect("resolution task"),
            ResolveApplyResult::Ignored
        );
        assert!(!ctx.pending_resolutions.contains_key("0xCOND"));
    }
}
