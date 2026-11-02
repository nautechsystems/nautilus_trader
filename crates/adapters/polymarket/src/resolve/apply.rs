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
use nautilus_common::{live::sender::EventSender, messages::DataEvent};
use nautilus_core::{AtomicMap, time::AtomicTime};
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
    watchlist::{ResolveWatchlist, TrackedInstrument},
};
use crate::{
    common::consts::{
        GAMMA_CONDITION_IDS_BATCH_SIZE, MARKET_VERSION_KEY, MARKET_VERSION_V2,
        POLYMARKET_PRICE_PRECISION, SUPPORTED_MARKET_VERSION,
    },
    http::{
        clob::PolymarketClobPublicClient, gamma::PolymarketGammaHttpClient, models::GammaMarket,
        query::GetGammaMarketsParams,
    },
    providers::extract_condition_id,
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
    pub(crate) data_sender: EventSender<DataEvent>,
    pub(crate) instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    pub(crate) watchlist: Arc<ResolveWatchlist>,
    pub(crate) apply_mutex: Arc<Mutex<()>>,
    pub(crate) pending_resolutions: Arc<DashMap<String, PendingResolution>>,
    pub(crate) deferred_resolutions: Arc<AtomicMap<InstrumentId, StrictResolvedMarket>>,
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) subscriptions: crate::data::DataSubscriptions,
}

impl ResolveContext {
    pub(crate) async fn sync_ws_subscription(&self, instrument_id: InstrumentId, token_id: String) {
        self.subscriptions.reconcile(instrument_id, token_id).await;
    }
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
            .read()
            .values()
            .filter(|entry| entry.paused)
            .flat_map(|entry| entry.tracked.values())
            .filter(|tracked| ctx.subscriptions.is_token_open(&tracked.token_id))
            .map(|tracked| (tracked.instrument_id, tracked.token_id.clone()))
            .collect::<Vec<_>>()
    };
    targets.sort_unstable_by(|left, right| left.1.cmp(&right.1));

    for (instrument_id, token_id) in targets {
        ctx.sync_ws_subscription(instrument_id, token_id).await;
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

        let ctf_only = chunk.iter().all(|condition_id| {
            ctx.watchlist.read().get(condition_id).is_some_and(|entry| {
                !entry.tracked.is_empty()
                    && entry
                        .tracked
                        .values()
                        .all(|tracked| tracked.market_version == SUPPORTED_MARKET_VERSION)
            })
        });

        let mut gamma_failures = AHashMap::new();

        match request_resolution_markets(gamma_client, chunk, ctf_only).await {
            Ok(lookup) => {
                let ResolutionMarketLookup { markets, failures } = lookup;
                gamma_failures = failures;
                stats.fetched_markets += markets.len();
                // Gamma's non-CTF evidence must never fall back to a CTF result
                let no_ctf_fallback: AHashSet<_> = markets
                    .iter()
                    .filter(|market| market.version.as_deref() != Some(SUPPORTED_MARKET_VERSION))
                    .map(|market| market.condition_id.clone())
                    .collect();
                unresolved_in_chunk.retain(|id| !no_ctf_fallback.contains(id));
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

                    apply_fetched_resolution(ctx, resolved, include_pending_intents, &mut stats)
                        .await;
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
                gamma_failures.extend(chunk.iter().cloned().map(|id| (id, message.clone())));
            }
        }

        for condition_id in unresolved_in_chunk {
            if !allow_ctf_fallback(ctx, &condition_id) {
                record_resolution_failure(
                    &mut stats,
                    &condition_id,
                    gamma_failures.get(&condition_id),
                );
                continue;
            }

            match clob_public_client.get_market(&condition_id).await {
                Ok(market) => {
                    if market.condition_id != condition_id {
                        log::warn!(
                            "Ignoring CLOB resolution with conflicting condition ID {} for {condition_id}",
                            market.condition_id
                        );
                        record_resolution_failure(
                            &mut stats,
                            &condition_id,
                            gamma_failures.get(&condition_id),
                        );
                        continue;
                    }

                    let Some(resolved) = build_resolved_market_from_clob_market(&market) else {
                        if market.version.as_deref() != Some(SUPPORTED_MARKET_VERSION) {
                            record_resolution_failure(
                                &mut stats,
                                &condition_id,
                                gamma_failures.get(&condition_id),
                            );
                        }

                        continue;
                    };

                    log::debug!(
                        "Resolve fallback via CLOB succeeded for condition_id={}",
                        resolved.condition_id
                    );
                    stats.clob_fallback_successes += 1;
                    apply_fetched_resolution(ctx, &resolved, include_pending_intents, &mut stats)
                        .await;
                }
                Err(e) => {
                    let message = format!(
                        "Resolve fallback via CLOB failed for condition_id={condition_id}: {e}"
                    );
                    log::warn!("{message}");
                    record_resolution_failure(&mut stats, &condition_id, Some(&message));
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

fn record_resolution_failure(
    stats: &mut ResolveApplyBatchStats,
    condition_id: &str,
    message: Option<&String>,
) {
    if let Some(message) = message {
        stats.error.get_or_insert_with(|| message.clone());
        stats.failed_condition_ids.push(condition_id.to_string());
    }
}

pub(crate) struct ResolutionMarketLookup {
    pub(crate) markets: Vec<GammaMarket>,
    pub(crate) failures: AHashMap<String, String>,
}

pub(crate) async fn request_resolution_markets(
    client: &PolymarketGammaHttpClient,
    condition_ids: &[String],
    ctf_only: bool,
) -> anyhow::Result<ResolutionMarketLookup> {
    let mut failures = AHashMap::new();

    let mut markets = client
        .request_markets_by_params(GetGammaMarketsParams {
            condition_ids: Some(condition_ids.to_vec()),
            closed: ctf_only.then_some(true),
            ..Default::default()
        })
        .await?;

    if !ctf_only {
        let missing: Vec<_> = condition_ids
            .iter()
            .filter(|id| !markets.iter().any(|market| market.condition_id == **id))
            .cloned()
            .collect();

        if !missing.is_empty() {
            match client
                .request_markets_by_params(GetGammaMarketsParams {
                    condition_ids: Some(missing.clone()),
                    closed: Some(true),
                    ..Default::default()
                })
                .await
            {
                Ok(closed) => markets.extend(closed),
                Err(e) => {
                    let message = format!("Closed-market resolution lookup failed: {e}");
                    log::warn!("{message}");
                    failures.extend(missing.into_iter().map(|id| (id, message.clone())));
                }
            }
        }
    }

    Ok(ResolutionMarketLookup { markets, failures })
}

fn allow_ctf_fallback(ctx: &ResolveContext, condition_id: &str) -> bool {
    !(ctx.watchlist.read().get(condition_id).is_some_and(|entry| {
        entry
            .tracked
            .values()
            .any(|tracked| tracked.market_version != SUPPORTED_MARKET_VERSION)
    }) || ctx.instruments.load().values().any(|instrument| {
        extract_condition_id(&instrument.id()).is_ok_and(|id| id == condition_id)
            && instrument
                .info()
                .and_then(|info| info.get_str(MARKET_VERSION_KEY))
                != Some(SUPPORTED_MARKET_VERSION)
    }))
}

async fn apply_fetched_resolution(
    ctx: &ResolveContext,
    resolved: &StrictResolvedMarket,
    include_pending_intents: bool,
    stats: &mut ResolveApplyBatchStats,
) {
    stats.resolved_markets += 1;

    let result = if include_pending_intents && resolved.market_version == SUPPORTED_MARKET_VERSION {
        apply_condition_resolution(
            ctx,
            &resolved.condition_id,
            &resolved.winning_asset_id,
            &resolved.winning_outcome,
        )
        .await
    } else if include_pending_intents {
        apply_condition_resolution_with_owners(
            ctx,
            &resolved.condition_id,
            &resolved.winning_asset_id,
            &resolved.winning_outcome,
            ResolveOwnerSelection::IncludePendingAssets(&resolved.asset_ids),
            &resolved.market_version,
            Some(resolved),
        )
        .await
    } else {
        apply_watched_condition_resolution(ctx, resolved).await
    };

    if let ResolveApplyResult::Applied { emitted_closes } = result
        && emitted_closes > 0
    {
        stats
            .emitted_condition_ids
            .push(resolved.condition_id.clone());
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
        SUPPORTED_MARKET_VERSION,
        None,
    )
    .await
}

pub(crate) async fn apply_condition_resolution_with_assets(
    ctx: &ResolveContext,
    condition_id: &str,
    winning_asset_id: &str,
    winning_outcome: &str,
    asset_ids: &[String],
    market_version: &str,
    evidence: Option<&StrictResolvedMarket>,
) -> ResolveApplyResult {
    apply_condition_resolution_with_owners(
        ctx,
        condition_id,
        winning_asset_id,
        winning_outcome,
        ResolveOwnerSelection::PayloadAssets(asset_ids),
        market_version,
        evidence,
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
        &resolution.market_version,
        Some(resolution),
    )
    .await
}

#[derive(Clone, Copy)]
enum ResolveOwnerSelection<'a> {
    Watched(&'a [String]),
    IncludePendingIntents,
    IncludePendingAssets(&'a [String]),
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
            || (!ctx
                .subscriptions
                .status_subscriptions()
                .contains(&instrument.id())
                && !ctx
                    .subscriptions
                    .close_subscriptions()
                    .contains(&instrument.id()))
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
            &resolution.market_version,
            Some(&resolution),
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
    market_version: &str,
    evidence: Option<&StrictResolvedMarket>,
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
        market_version,
        evidence,
    )
    .await
}

async fn apply_condition_resolution_inner(
    ctx: &ResolveContext,
    condition_id: &str,
    winning_asset_id: &str,
    winning_outcome: &str,
    owner_selection: ResolveOwnerSelection<'_>,
    market_version: &str,
    evidence: Option<&StrictResolvedMarket>,
) -> ResolveApplyResult {
    let condition_id_string = condition_id.to_string();
    let reconcile_guard = tokio::select! {
        () = ctx.cancellation_token.cancelled() => return ResolveApplyResult::Ignored,
        guard = ctx.subscriptions.lock_reconciliation() => guard,
    };
    let (reconciliation_targets, emitted_closes) = {
        let _guard = ctx.apply_mutex.lock();
        // Metadata writers hold this boundary through publication; keep the validated mapping
        // stable until every terminal event and the terminal marker have been published.
        let mut terminal_conditions = ctx.subscriptions.lock_closed_conditions();

        // Reset takes the same locks, so an old context cannot publish after reset returns
        if ctx.cancellation_token.is_cancelled() {
            return ResolveApplyResult::Ignored;
        }

        let entry = ctx.watchlist.read().get(&condition_id_string).cloned();

        let matches_condition = |instrument_id: &InstrumentId| {
            extract_condition_id(instrument_id).is_ok_and(|candidate| candidate == condition_id)
        };
        let watched_instrument_ids: AHashSet<InstrumentId> = entry
            .iter()
            .flat_map(|entry| entry.tracked.values())
            .map(|tracked| tracked.instrument_id)
            .collect();

        let active_status_ids: AHashSet<InstrumentId> = ctx
            .subscriptions
            .status_subscriptions()
            .iter()
            .map(|id| *id.key())
            .filter(|instrument_id| {
                matches_condition(instrument_id) || watched_instrument_ids.contains(instrument_id)
            })
            .collect();

        let active_close_ids: AHashSet<InstrumentId> = ctx
            .subscriptions
            .close_subscriptions()
            .iter()
            .map(|id| *id.key())
            .filter(|instrument_id| {
                matches_condition(instrument_id) || watched_instrument_ids.contains(instrument_id)
            })
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
                | ResolveOwnerSelection::IncludePendingAssets(_)
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
                        market_version: market_version.into(),
                        instrument_id,
                        token_id,
                        price_precision,
                        open_position_ids: AHashSet::new(),
                        has_data_subscription: true,
                    },
                );
            }
        }

        // Ownership can grow while waiting, so check every assembled owner before publication
        if !resolution_owners_match(
            &tracked_instruments,
            &loaded,
            owner_selection,
            market_version,
            evidence,
        ) {
            log::warn!(
                "Ignoring resolution for {condition_id}: metadata conflicts with resolution owners"
            );
            return ResolveApplyResult::Ignored;
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
                            market_version: market_version.into(),
                            condition_id: condition_id_string.clone(),
                            asset_ids: evidence.map_or_else(
                                || asset_ids.to_vec(),
                                |resolved| resolved.asset_ids.clone(),
                            ),
                            outcomes: evidence
                                .map_or_else(Vec::new, |resolved| resolved.outcomes.clone()),
                            winning_asset_id: winning_asset_id.to_string(),
                            winning_outcome: winning_outcome.to_string(),
                        });
                });
            }
        }

        if tracked_instruments.is_empty() && deferred_ids.is_empty() {
            ctx.watchlist.write().remove(&condition_id_string);
            return ResolveApplyResult::Ignored;
        }

        if let ResolveOwnerSelection::AdmittedInstrument(instrument) = owner_selection
            && tracked_instruments.contains_key(&instrument.id())
        {
            let mut instrument = instrument.clone();
            if let InstrumentAny::BinaryOption(binary) = &mut instrument {
                crate::filters::set_market_closed(binary, true);
                binary.ts_init = ts_init;
                binary.ts_event = ts_init;
            }

            if let Err(e) = ctx.data_sender.send(DataEvent::Data(instrument.into())) {
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

                if let Err(e) = ctx
                    .data_sender
                    .send(DataEvent::Data(NautilusData::InstrumentStatus(status)))
                {
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

        ctx.subscriptions.extend_reconciliation_targets(
            condition_id,
            &loaded,
            &mut reconciliation_targets,
        );

        // Clear settlement state before releasing the corresponding subscription intents
        ctx.watchlist.write().remove(&condition_id_string);

        for instrument_id in active_status_ids.union(&active_close_ids) {
            if !deferred_ids.contains(instrument_id) {
                ctx.deferred_resolutions.remove(instrument_id);
            }
        }

        let tracked_ids = tracked_instruments.keys().copied().collect();
        ctx.subscriptions
            .finish_resolution(condition_id, &tracked_ids, &deferred_ids);

        if terminal_conditions.insert(condition_id_string.clone()) {
            log::info!("Market resolved for condition {condition_id}, reconciling live data state");
        }
        (reconciliation_targets, emitted_closes)
    };

    drop(reconcile_guard);

    let mut reconciliation_targets: Vec<(InstrumentId, String)> =
        reconciliation_targets.into_iter().collect();
    reconciliation_targets.sort_unstable_by(|left, right| left.1.cmp(&right.1));

    for (instrument_id, token_id) in reconciliation_targets {
        if !ctx.subscriptions.is_token_open(&token_id) {
            continue;
        }

        ctx.sync_ws_subscription(instrument_id, token_id).await;
    }

    ResolveApplyResult::Applied { emitted_closes }
}

fn resolution_owners_match(
    tracked: &AHashMap<InstrumentId, TrackedInstrument>,
    loaded: &AHashMap<InstrumentId, InstrumentAny>,
    selection: ResolveOwnerSelection<'_>,
    market_version: &str,
    evidence: Option<&StrictResolvedMarket>,
) -> bool {
    let asset_ids = match selection {
        ResolveOwnerSelection::PayloadAssets(ids)
        | ResolveOwnerSelection::Watched(ids)
        | ResolveOwnerSelection::IncludePendingAssets(ids) => Some(ids),
        _ => None,
    };

    let admitted = match selection {
        ResolveOwnerSelection::AdmittedInstrument(instrument) => Some(instrument),
        _ => None,
    };

    for tracked in tracked.values() {
        if tracked.market_version != market_version
            || asset_ids.is_some_and(|ids| !ids.contains(&tracked.token_id))
        {
            return false;
        }

        for instrument in loaded
            .get(&tracked.instrument_id)
            .into_iter()
            .chain(admitted.filter(|instrument| instrument.id() == tracked.instrument_id))
        {
            if instrument
                .info()
                .and_then(|info| info.get_str(MARKET_VERSION_KEY))
                != Some(market_version)
                || instrument.raw_symbol().as_str() != tracked.token_id
                || !resolution_outcome_matches(instrument, evidence)
            {
                return false;
            }
        }
    }

    true
}

fn resolution_outcome_matches(
    instrument: &InstrumentAny,
    evidence: Option<&StrictResolvedMarket>,
) -> bool {
    let Some(resolved) = evidence.filter(|resolved| resolved.market_version == MARKET_VERSION_V2)
    else {
        return true;
    };

    let InstrumentAny::BinaryOption(binary) = instrument else {
        return false;
    };

    let Some(index) = resolved
        .asset_ids
        .iter()
        .position(|id| id == binary.raw_symbol.as_str())
    else {
        return false;
    };

    binary.outcome.as_ref().is_some_and(|outcome| {
        resolved
            .outcomes
            .get(index)
            .is_some_and(|expected| outcome.as_str() == expected)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ahash::{AHashMap, AHashSet};
    use indexmap::IndexMap;
    use nautilus_common::messages::DataEvent;
    use nautilus_core::{AtomicMap, Params, UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::{
        identifiers::{InstrumentId, PositionId, Symbol},
        instruments::stubs::binary_option,
    };
    use parking_lot::Mutex;
    use rstest::rstest;

    use super::*;
    use crate::{filters::set_market_closed, resolve::ResolveWatchEntry};

    fn make_resolve_context() -> (
        ResolveContext,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (ws_tx, _ws_rx) = tokio::sync::mpsc::unbounded_channel();
        let instruments = Arc::new(AtomicMap::new());
        let watchlist = Arc::new(ResolveWatchlist::default());
        let owner_lock = Arc::new(Mutex::new(()));
        let deferred_resolutions = Arc::new(AtomicMap::new());

        let subscriptions = crate::data::DataSubscriptions::new(
            instruments.clone(),
            watchlist.clone(),
            deferred_resolutions.clone(),
            owner_lock.clone(),
            crate::websocket::pool::PolymarketMarketPoolHandle::test_single_shard(ws_tx, &[]),
            false,
        );

        let ctx = ResolveContext {
            subscriptions,
            clock: get_atomic_clock_realtime(),
            data_sender: data_tx.into(),
            instruments,
            watchlist,
            apply_mutex: owner_lock,
            pending_resolutions: Arc::new(DashMap::new()),
            deferred_resolutions,
            cancellation_token: CancellationToken::new(),
        };

        (ctx, data_rx)
    }

    #[rstest]
    #[case(Some("v1"), true)]
    #[case(Some("v2"), false)]
    #[case(None, false)]
    #[tokio::test]
    async fn current_cached_protocol_overrides_stale_watch(
        #[case] version: Option<&str>,
        #[case] accepted: bool,
        #[values("watched", "payload", "admitted")] ownership: &str,
    ) {
        let (ctx, mut data_rx) = make_resolve_context();
        let mut binary = binary_option();
        binary.id = InstrumentId::from("0xCOND-101.POLYMARKET");
        binary.raw_symbol = Symbol::new("101");
        binary.info = Some(Params::new(IndexMap::from([(
            "market_version".into(),
            serde_json::json!(version),
        )])));
        let instrument_id = binary.id;
        let mut admitted = binary.clone();
        admitted.info = Some(Params::new(IndexMap::from([(
            "market_version".into(),
            serde_json::json!("v1"),
        )])));
        let admitted = InstrumentAny::BinaryOption(admitted);
        ctx.instruments
            .insert(instrument_id, InstrumentAny::BinaryOption(binary));
        ctx.subscriptions
            .close_subscriptions()
            .insert(instrument_id);
        ctx.watchlist.write().insert(
            "0xCOND".into(),
            ResolveWatchEntry {
                condition_id: "0xCOND".into(),
                expiration_ns: UnixNanos::default(),
                paused: false,
                tracked: AHashMap::from_iter([(
                    "101".into(),
                    TrackedInstrument {
                        market_version: "v1".into(),
                        instrument_id,
                        token_id: "101".into(),
                        price_precision: 3,
                        open_position_ids: AHashSet::new(),
                        has_data_subscription: true,
                    },
                )]),
            },
        );

        let resolution = StrictResolvedMarket {
            market_version: "v1".into(),
            condition_id: "0xCOND".into(),
            asset_ids: vec!["101".into(), "202".into()],
            outcomes: Vec::new(),
            winning_asset_id: "101".into(),
            winning_outcome: "Yes".into(),
        };

        let result = if ownership == "admitted" {
            apply_condition_resolution_with_owners(
                &ctx,
                "0xCOND",
                "101",
                "Yes",
                ResolveOwnerSelection::AdmittedInstrument(&admitted),
                "v1",
                None,
            )
            .await
        } else if ownership == "watched" {
            apply_watched_condition_resolution(&ctx, &resolution).await
        } else {
            apply_condition_resolution_with_assets(
                &ctx,
                "0xCOND",
                "101",
                "Yes",
                &resolution.asset_ids,
                "v1",
                None,
            )
            .await
        };

        assert_eq!(
            result,
            if accepted {
                ResolveApplyResult::Applied { emitted_closes: 1 }
            } else {
                ResolveApplyResult::Ignored
            }
        );
        assert_eq!(ctx.subscriptions.is_condition_closed("0xCOND"), accepted);

        if accepted {
            let updated = if ownership == "admitted" {
                let DataEvent::Data(NautilusData::Instrument(updated)) =
                    data_rx.try_recv().unwrap()
                else {
                    panic!("expected admitted instrument");
                };

                Some(*updated)
            } else {
                None
            };

            let DataEvent::Data(NautilusData::InstrumentClose(close)) = data_rx.try_recv().unwrap()
            else {
                panic!("expected close")
            };

            assert_eq!(close.instrument_id, instrument_id);
            assert_eq!(close.close_price, Price::from("1.000"));
            assert_eq!(close.close_type, InstrumentCloseType::ContractExpired);
            assert_eq!(close.ts_event, close.ts_init);

            if let Some(updated) = updated {
                let InstrumentAny::BinaryOption(mut expected) = admitted else {
                    unreachable!()
                };

                set_market_closed(&mut expected, true);
                expected.ts_event = close.ts_init;
                expected.ts_init = close.ts_init;
                assert_eq!(updated, InstrumentAny::BinaryOption(expected));
            }
        }

        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn deferred_v2_resolution_retains_outcome_pairs(
        #[values(0, 1)] index: usize,
        #[values("none", "cached", "admitted")] conflict: &str,
    ) {
        let (ctx, mut data_rx) = make_resolve_context();

        let resolution = StrictResolvedMarket {
            market_version: "v2".into(),
            condition_id: "0xCOND".into(),
            asset_ids: vec!["101".into(), "202".into()],
            outcomes: vec!["Yes".into(), "No".into()],
            winning_asset_id: "101".into(),
            winning_outcome: "Yes".into(),
        };

        let mut binary = binary_option();
        binary.id =
            InstrumentId::from(format!("0xCOND-{}.POLYMARKET", resolution.asset_ids[index]));
        binary.raw_symbol = Symbol::new(&resolution.asset_ids[index]);
        binary.outcome = Some(Ustr::from(&resolution.outcomes[index]));
        binary.info = Some(Params::new(IndexMap::from([(
            "market_version".into(),
            serde_json::json!("v2"),
        )])));
        let instrument_id = binary.id;
        ctx.subscriptions
            .close_subscriptions()
            .insert(instrument_id);
        let reversed = vec!["202".into(), "101".into()];

        let result = apply_condition_resolution_with_assets(
            &ctx,
            "0xCOND",
            "101",
            "Yes",
            &reversed,
            "v2",
            Some(&resolution),
        )
        .await;

        assert_eq!(result, ResolveApplyResult::Applied { emitted_closes: 0 });
        assert_eq!(
            ctx.deferred_resolutions.get_cloned(&instrument_id),
            Some(resolution.clone())
        );
        assert!(data_rx.try_recv().is_err());
        let mut cached = binary.clone();
        if conflict == "cached" {
            cached.outcome = Some(Ustr::from("Conflict"));
        }

        if conflict == "admitted" {
            binary.outcome = Some(Ustr::from("Conflict"));
        }

        ctx.instruments
            .insert(instrument_id, InstrumentAny::BinaryOption(cached));
        let admitted = InstrumentAny::BinaryOption(binary);

        admit_data_resolution_instrument(&ctx, &admitted).await;

        if conflict == "none" {
            let DataEvent::Data(NautilusData::Instrument(updated)) = data_rx.try_recv().unwrap()
            else {
                panic!("expected instrument")
            };

            let DataEvent::Data(NautilusData::InstrumentClose(close)) = data_rx.try_recv().unwrap()
            else {
                panic!("expected close")
            };

            let InstrumentAny::BinaryOption(mut expected) = admitted else {
                unreachable!()
            };

            set_market_closed(&mut expected, true);
            expected.ts_event = close.ts_init;
            expected.ts_init = close.ts_init;
            assert_eq!(*updated, InstrumentAny::BinaryOption(expected));
            assert_eq!(close.instrument_id, instrument_id);
            assert_eq!(
                close.close_price,
                if index == 0 {
                    Price::from("1.000")
                } else {
                    Price::from("0.000")
                }
            );
            assert_eq!(close.close_type, InstrumentCloseType::ContractExpired);
            assert_eq!(close.ts_event, close.ts_init);
            assert!(!ctx.deferred_resolutions.contains_key(&instrument_id));
        } else {
            assert_eq!(
                ctx.deferred_resolutions.get_cloned(&instrument_id),
                Some(resolution)
            );
        }

        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn data_status_subscription_emits_status_without_close() {
        let (ctx, mut data_rx) = make_resolve_context();
        ctx.subscriptions
            .status_subscriptions()
            .insert(InstrumentId::from("0xCOND-0xYES.POLYMARKET"));
        ctx.watchlist.write().insert(
            "0xCOND".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND".to_string(),
                expiration_ns: UnixNanos::default(),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        market_version: "v1".into(),
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
            Ok(DataEvent::Data(NautilusData::InstrumentStatus(_)))
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
        ctx.subscriptions
            .status_subscriptions()
            .insert(instrument_id);
        ctx.subscriptions
            .close_subscriptions()
            .insert(instrument_id);

        assert_eq!(
            apply_condition_resolution(&ctx, "0xCOND", "0xYES", "Yes").await,
            ResolveApplyResult::Applied { emitted_closes: 1 }
        );

        assert!(
            !ctx.subscriptions
                .status_subscriptions()
                .contains(&instrument_id)
        );
        assert!(
            !ctx.subscriptions
                .close_subscriptions()
                .contains(&instrument_id)
        );
        assert!(!ctx.pending_resolutions.contains_key("0xCOND"));
        assert!(ctx.subscriptions.is_condition_closed("0xCOND"));
        assert!(matches!(
            data_rx.try_recv(),
            Ok(DataEvent::Data(NautilusData::InstrumentStatus(_)))
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
        ctx.subscriptions
            .status_subscriptions()
            .insert(instrument_id);
        ctx.watchlist.write().insert(
            "0xCOND".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND".to_string(),
                expiration_ns: UnixNanos::default(),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        market_version: "v1".into(),
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

        let reconcile_guard = ctx.subscriptions.lock_reconciliation().await;
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
        ctx.subscriptions
            .status_subscriptions()
            .insert(instrument_id);
        ctx.subscriptions
            .close_subscriptions()
            .insert(instrument_id);

        let resolution = StrictResolvedMarket {
            market_version: "v1".into(),
            condition_id: "0xCOND".to_string(),
            asset_ids: vec!["0xYES".to_string(), "0xNO".to_string()],
            outcomes: Vec::new(),
            winning_asset_id: "0xYES".to_string(),
            winning_outcome: "Yes".to_string(),
        };

        let reconcile_guard = ctx.subscriptions.lock_reconciliation().await;
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
        assert!(ctx.subscriptions.is_condition_closed("0xCOND"));
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
        ctx.subscriptions
            .status_subscriptions()
            .insert(instrument_id);
        ctx.subscriptions
            .close_subscriptions()
            .insert(instrument_id);
        let reconcile_guard = ctx.subscriptions.lock_reconciliation().await;
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
        assert!(ctx.subscriptions.is_condition_closed("0xCOND"));
        assert_eq!(std::iter::from_fn(|| data_rx.try_recv().ok()).count(), 2);
    }

    #[rstest]
    #[tokio::test]
    async fn resolution_clears_pending_barrier_when_watch_disappears() {
        let (ctx, _data_rx) = make_resolve_context();
        let instrument_id = InstrumentId::from("0xCOND-0xYES.POLYMARKET");
        ctx.watchlist.write().insert(
            "0xCOND".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND".to_string(),
                expiration_ns: UnixNanos::default(),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        market_version: "v1".into(),
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

        let reconcile_guard = ctx.subscriptions.lock_reconciliation().await;
        let apply_ctx = ctx.clone();

        let apply_task = tokio::spawn(async move {
            let _pending_guard = pending_guard;
            apply_condition_resolution(&apply_ctx, "0xCOND", "0xYES", "Yes").await
        });
        tokio::task::yield_now().await;
        ctx.watchlist.write().remove(&"0xCOND".to_string());
        drop(reconcile_guard);

        assert_eq!(
            apply_task.await.expect("resolution task"),
            ResolveApplyResult::Ignored
        );
        assert!(!ctx.pending_resolutions.contains_key("0xCOND"));
    }
}
