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

use std::{collections::BTreeMap, future::Future, sync::Arc, time::Duration};

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use jiff::Timestamp;
use nautilus_common::{
    live::sender::EventSender, messages::DataEvent, providers::InstrumentProvider,
};
use nautilus_core::{AtomicMap, UnixNanos, time::AtomicTime};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use parking_lot::{Mutex, MutexGuard};
use rust_decimal::Decimal;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    DataSubscriptions, PolymarketDataClient, runtime::is_instrument_expired_and_not_reported_open,
};
use crate::{
    common::consts::GAMMA_CONDITION_IDS_BATCH_SIZE,
    filters::{
        InstrumentFilter, binary_market_closed, is_expired, market_closed, set_market_closed,
    },
    http::{
        gamma::PolymarketGammaHttpClient, models::GammaMarket, parse::is_market_closed,
        query::GetGammaMarketsParams,
    },
    providers::{extract_condition_id, validated_condition_id},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TokenMeta {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) price_precision: u8,
    pub(crate) size_precision: u8,
    pub(crate) min_order_size: Option<Ustr>,
    pub(crate) neg_risk: Option<bool>,
}

#[derive(Clone, Copy, Debug)]
struct LiveTick {
    tick_size: Decimal,
    ts_event: UnixNanos,
}

#[derive(Debug, Default)]
pub(super) struct InstrumentUpdateState {
    live_ticks: AHashMap<Ustr, LiveTick>,
    discovered_ids: AHashSet<InstrumentId>,
    metadata_versions: AHashMap<InstrumentId, u64>,
    pub(super) retired: bool,
}

impl InstrumentUpdateState {
    pub(super) fn record_metadata(&mut self, id: InstrumentId) {
        if let Some(version) = self.metadata_versions.get_mut(&id) {
            *version = version.wrapping_add(1);
        }
    }

    pub(super) fn retire_generation(&mut self) {
        self.retired = true;
    }

    pub(super) fn is_stale_tick(&self, token_id: &Ustr, ts_event: UnixNanos) -> bool {
        self.live_ticks
            .get(token_id)
            .is_some_and(|current| current.ts_event > ts_event)
    }

    pub(super) fn record_live_tick(
        &mut self,
        token_id: Ustr,
        tick_size: Decimal,
        ts_event: UnixNanos,
    ) {
        self.live_ticks.insert(
            token_id,
            LiveTick {
                tick_size,
                ts_event,
            },
        );
    }

    #[cfg(test)]
    pub(super) fn contains_live_tick(&self, token_id: &Ustr) -> bool {
        self.live_ticks.contains_key(token_id)
    }

    pub(super) fn compose_instrument(
        &self,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<InstrumentAny> {
        let token_id = Ustr::from(instrument.raw_symbol().as_str());
        let Some(live_tick) = self.live_ticks.get(&token_id) else {
            return Ok(instrument.clone());
        };

        crate::http::parse::rebuild_instrument_with_tick_size(
            instrument,
            &live_tick.tick_size.to_string(),
            live_tick.ts_event,
            instrument.ts_init(),
        )
    }
}

pub(super) fn cache_instrument_unchecked(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    instrument: &InstrumentAny,
) {
    cache_instruments_unchecked(instruments, token_meta, std::slice::from_ref(instrument));
}

pub(super) fn cache_instruments_unchecked(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    pending: &[InstrumentAny],
) {
    if pending.is_empty() {
        return;
    }

    for instrument in pending {
        token_meta.insert(
            instrument.raw_symbol().inner(),
            TokenMeta::from_instrument(instrument),
        );
    }

    instruments.rcu(|map| {
        for instrument in pending {
            map.insert(instrument.id(), instrument.clone());
        }
    });
}

// Applies one instrument while serializing live tick composition, cache mutation,
// and queued publication with terminal resolution.
pub(super) fn apply_live_instrument(
    closed_condition_ids: &Arc<Mutex<AHashSet<String>>>,
    instrument_update_state: &Arc<Mutex<InstrumentUpdateState>>,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    instrument: &InstrumentAny,
    apply: impl FnOnce(&InstrumentAny),
) -> bool {
    let mut update_state = instrument_update_state.lock();

    // Keep the guard through the callback so cache writes and queued publication stay ordered
    let applied = apply_live_instrument_locked(
        closed_condition_ids,
        &update_state,
        instruments,
        token_meta,
        instrument,
        apply,
    );

    if applied {
        update_state.record_metadata(instrument.id());
    }
    applied
}

pub(super) fn apply_live_instrument_locked(
    closed_condition_ids: &Arc<Mutex<AHashSet<String>>>,
    update_state: &MutexGuard<'_, InstrumentUpdateState>,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    instrument: &InstrumentAny,
    apply: impl FnOnce(&InstrumentAny),
) -> bool {
    if update_state.retired {
        return false;
    }

    if instruments
        .get_cloned(&instrument.id())
        .is_some_and(|cached| stale_metadata(instrument, &cached))
    {
        return false;
    }

    let instrument = match update_state.compose_instrument(instrument) {
        Ok(instrument) => instrument,
        Err(e) => {
            log::error!(
                "Failed to apply live tick to instrument {}: {e}",
                instrument.id()
            );
            return false;
        }
    };

    // Resolution validates the cached mapping under this guard through terminal publication
    let terminal_conditions = closed_condition_ids.lock();
    let is_terminal = extract_condition_id(&instrument.id())
        .is_ok_and(|condition_id| terminal_conditions.contains(&condition_id));

    if is_terminal {
        return false;
    }

    cache_instrument_unchecked(instruments, token_meta, &instrument);
    apply(&instrument);
    true
}

pub(super) fn stale_metadata(incoming: &InstrumentAny, cached: &InstrumentAny) -> bool {
    fn revision(instrument: &InstrumentAny) -> Option<Timestamp> {
        serde_json::from_str::<serde_json::Value>(instrument.info()?.get_str("gamma_market")?)
            .ok()?
            .get("updatedAt")?
            .as_str()?
            .parse()
            .ok()
    }
    revision(incoming)
        .zip(revision(cached))
        .is_some_and(|(incoming, cached)| incoming < cached)
}

pub(super) fn publish_cached_condition_closed(
    condition_id: &str,
    instrument_update_state: &Arc<Mutex<InstrumentUpdateState>>,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    data_sender: &EventSender<DataEvent>,
    clock: &AtomicTime,
) -> usize {
    let update_state = instrument_update_state.lock();
    if update_state.retired {
        return 0;
    }

    let ts_init = clock.get_time_ns();
    let mut updated_ids = Vec::new();
    instruments.rcu(|map| {
        updated_ids.clear();

        for (instrument_id, instrument) in map.iter_mut() {
            if !extract_condition_id(instrument_id).is_ok_and(|candidate| candidate == condition_id)
            {
                continue;
            }

            if let InstrumentAny::BinaryOption(binary) = instrument
                && binary_market_closed(binary) != Some(true)
            {
                set_market_closed(binary, true);
                binary.ts_init = ts_init;
                binary.ts_event = ts_init;
                updated_ids.push(binary.id);
            }
        }
    });

    publish_market_closure_updates(instruments, data_sender, &updated_ids);

    updated_ids.len()
}

impl TokenMeta {
    pub(crate) fn from_instrument(instrument: &InstrumentAny) -> Self {
        let info = match instrument {
            InstrumentAny::BinaryOption(binary) => binary.info.as_ref(),
            _ => None,
        };

        Self {
            instrument_id: instrument.id(),
            price_precision: instrument.price_precision(),
            size_precision: instrument.size_precision(),
            min_order_size: info
                .and_then(|params| params.get_str("min_order_size"))
                .map(Ustr::from),
            neg_risk: info.and_then(|params| params.get_bool("neg_risk")),
        }
    }
}

type SupplementalRefresh = (
    Vec<String>,
    Vec<Arc<dyn InstrumentFilter>>,
    tokio::sync::oneshot::Receiver<AHashSet<InstrumentId>>,
);

#[derive(Clone, Copy)]
pub(super) enum InstrumentSource<'a> {
    Discovery,
    Supplemental(&'a DataSubscriptions),
    Retained(&'a DataSubscriptions, &'a AHashMap<InstrumentId, u64>),
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared adapter state is held in Arcs"
)]
pub(super) fn cache_and_publish_instruments(
    closed_condition_ids: &Arc<Mutex<AHashSet<String>>>,
    instrument_update_state: &Arc<Mutex<InstrumentUpdateState>>,
    instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    data_sender: &EventSender<DataEvent>,
    now_ns: UnixNanos,
    instruments: &[InstrumentAny],
    source: InstrumentSource<'_>,
) -> usize {
    let mut update_state = instrument_update_state.lock();
    if update_state.retired {
        return 0;
    }
    let _owner_guard = match &source {
        InstrumentSource::Retained(subscriptions, _) => Some(subscriptions.owner_lock.lock()),
        _ => None,
    };
    let mut pending = Vec::with_capacity(instruments.len());
    let cached = instruments_cache.load();

    for instrument in instruments {
        let id = instrument.id();
        let excluded = match &source {
            InstrumentSource::Discovery => false,
            InstrumentSource::Supplemental(subscriptions) => {
                update_state.discovered_ids.contains(&id) || subscriptions.is_retained(id)
            }
            InstrumentSource::Retained(subscriptions, versions) => {
                !subscriptions.is_retained(id)
                    || update_state.discovered_ids.contains(&id)
                    || !versions.contains_key(&id)
                    || versions.get(&id) != update_state.metadata_versions.get(&id)
            }
        };

        if excluded
            || cached
                .get(&id)
                .is_some_and(|cached| stale_metadata(instrument, cached))
        {
            continue;
        }

        if is_instrument_expired_and_not_reported_open(instrument, now_ns)
            && !(matches!(source, InstrumentSource::Retained(..))
                && market_closed(instrument) == Some(true))
        {
            log::debug!(
                "Skipping expired instrument {} during live cache publish",
                instrument.id()
            );
            continue;
        }

        match update_state.compose_instrument(instrument) {
            Ok(instrument) => pending.push(instrument),
            Err(e) => log::error!(
                "Failed to apply live tick to instrument {}: {e}",
                instrument.id()
            ),
        }
    }
    drop(cached);
    let mut terminal_conditions = closed_condition_ids.lock();
    pending.retain(|instrument| {
        !extract_condition_id(&instrument.id())
            .is_ok_and(|condition_id| terminal_conditions.contains(&condition_id))
    });
    cache_instruments_unchecked(instruments_cache, token_meta, &pending);

    if let InstrumentSource::Retained(subscriptions, _) = source {
        refresh_watchlist_locked(subscriptions, &pending);
        for instrument in &pending {
            if market_closed(instrument) == Some(true)
                && let Ok(id) = extract_condition_id(&instrument.id())
            {
                terminal_conditions.insert(id);
            }
        }
    }
    let total = pending.len();

    // Keep the update guard through publication so live tick updates cannot overtake this batch
    for instrument in pending {
        let instrument_id = instrument.id();
        update_state.record_metadata(instrument_id);
        if matches!(source, InstrumentSource::Discovery) {
            update_state.discovered_ids.insert(instrument_id);
        }

        if let Err(e) = data_sender.send(DataEvent::Data(instrument.into())) {
            log::warn!("Failed to publish instrument {instrument_id}: {e}");
        }
    }

    total
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared adapter state is held in Arcs"
)]
pub(super) async fn refresh_scoped_instruments(
    http_client: PolymarketGammaHttpClient,
    instrument_config: Option<crate::config::PolymarketInstrumentProviderConfig>,
    filters: Vec<Arc<dyn InstrumentFilter>>,
    subscriptions: &DataSubscriptions,
    instrument_update_state: &Arc<Mutex<InstrumentUpdateState>>,
    data_sender: &EventSender<DataEvent>,
    clock: &'static AtomicTime,
    supplemental: &tokio::sync::mpsc::Sender<SupplementalRefresh>,
) -> anyhow::Result<usize> {
    instrument_update_state.lock().discovered_ids.clear();
    let requests =
        crate::providers::configured_requests(&instrument_config.unwrap_or_default(), &filters)?;
    let (done, completion) = tokio::sync::oneshot::channel();
    if !requests.retained_slugs.is_empty() {
        let _ = supplemental.try_send((requests.retained_slugs, filters.clone(), completion));
    }
    let mut seen = AHashSet::new();
    let mut total = 0;
    let result = crate::providers::fetch_required_instruments(
        &http_client,
        requests.required,
        &filters,
        &mut seen,
        |batch| {
            total += cache_and_publish_instruments(
                &subscriptions.closed_condition_ids,
                instrument_update_state,
                &subscriptions.instruments,
                &subscriptions.token_meta,
                data_sender,
                clock.get_time_ns(),
                batch,
                InstrumentSource::Discovery,
            );
            refresh_watchlist_deadlines(subscriptions, batch);
        },
    )
    .await;
    let _ = done.send(seen);
    result?;
    Ok(total)
}

async fn refresh_retained_instruments(
    http_client: &PolymarketGammaHttpClient,
    subscriptions: &DataSubscriptions,
    instrument_update_state: &Arc<Mutex<InstrumentUpdateState>>,
    data_sender: &EventSender<DataEvent>,
    clock: &AtomicTime,
) -> anyhow::Result<usize> {
    let retained = subscriptions.retained_instrument_ids();
    let mut conditions = BTreeMap::<String, Vec<InstrumentId>>::new();
    {
        let mut state = instrument_update_state.lock();
        state
            .metadata_versions
            .retain(|id, _| retained.contains(id));
        for id in retained {
            match validated_condition_id(&id) {
                Ok(condition) => {
                    state.metadata_versions.entry(id).or_default();
                    conditions.entry(condition).or_default().push(id);
                }
                Err(e) => {
                    log::warn!("Cannot refresh retained Polymarket instrument {id}: {e}");
                }
            }
        }
    }
    let conditions = conditions.into_iter().collect::<Vec<_>>();
    let http = http_client.for_retained_refresh();
    let mut pending = Vec::new();
    let mut versions = AHashMap::new();
    let mut failures = 0;

    for chunk in conditions.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
        let requested = {
            let state = instrument_update_state.lock();
            chunk
                .iter()
                .filter_map(|(condition, ids)| {
                    let mut needed = false;

                    for id in ids {
                        if subscriptions.is_retained(*id) && !state.discovered_ids.contains(id) {
                            versions.insert(*id, state.metadata_versions[id]);
                            needed = true;
                        }
                    }
                    needed.then_some(condition.clone())
                })
                .collect::<Vec<_>>()
        };

        if requested.is_empty() {
            continue;
        }

        match http
            .request_instruments_by_params(GetGammaMarketsParams {
                condition_ids: Some(requested.clone()),
                ..Default::default()
            })
            .await
        {
            Ok(mut batch) => {
                let returned = batch
                    .iter()
                    .filter_map(|instrument| extract_condition_id(&instrument.id()).ok())
                    .collect::<AHashSet<_>>();
                let missing = requested
                    .iter()
                    .filter(|id| !returned.contains(*id))
                    .cloned()
                    .collect::<Vec<_>>();

                if !missing.is_empty() {
                    match http
                        .request_instruments_by_params(GetGammaMarketsParams {
                            condition_ids: Some(missing.clone()),
                            closed: Some(true),
                            ..Default::default()
                        })
                        .await
                    {
                        Ok(closed) => batch.extend(
                            closed
                                .into_iter()
                                .filter(|instrument| market_closed(instrument) == Some(true)),
                        ),
                        Err(e) => {
                            log::warn!("Failed retained closed-market lookup: {e}");
                            failures += 1;
                        }
                    }

                    if missing.iter().any(|id| {
                        !batch.iter().any(|instrument| {
                            extract_condition_id(&instrument.id())
                                .is_ok_and(|returned| &returned == id)
                        })
                    }) {
                        log::warn!("Gamma omitted retained Polymarket metadata");
                        failures += 1;
                    }
                }
                pending.extend(batch.into_iter().filter(|instrument| {
                    versions.contains_key(&instrument.id())
                        && extract_condition_id(&instrument.id())
                            .is_ok_and(|id| requested.contains(&id))
                }));
            }
            Err(e) => {
                log::warn!("Failed retained Polymarket lookup: {e}");
                failures += 1;
            }
        }
    }
    let closing = pending
        .iter()
        .filter(|instrument| market_closed(instrument) == Some(true))
        .filter_map(|instrument| extract_condition_id(&instrument.id()).ok())
        .collect::<AHashSet<_>>();
    let total = {
        let _reconcile = if closing.is_empty() {
            None
        } else {
            Some(subscriptions.lock_reconciliation().await)
        };
        cache_and_publish_instruments(
            &subscriptions.closed_condition_ids,
            instrument_update_state,
            &subscriptions.instruments,
            &subscriptions.token_meta,
            data_sender,
            clock.get_time_ns(),
            &pending,
            InstrumentSource::Retained(subscriptions, &versions),
        )
    };

    for condition in closing {
        if subscriptions.is_condition_closed(&condition) {
            subscriptions.retire_condition(&condition, [], None).await;
        }
    }
    anyhow::ensure!(
        failures == 0,
        "Failed {failures} retained Polymarket lookup(s)"
    );
    Ok(total)
}

fn refresh_watchlist_deadlines(subscriptions: &DataSubscriptions, refreshed: &[InstrumentAny]) {
    let _guard = subscriptions.owner_lock.lock();
    let closed = subscriptions.closed_condition_ids.lock();
    let instruments = subscriptions.instruments.load();
    let accepted = refreshed
        .iter()
        .filter_map(|instrument| {
            let id = instrument.id();
            (!extract_condition_id(&id).is_ok_and(|id| closed.contains(&id)))
                .then(|| instruments.get(&id).cloned())
                .flatten()
        })
        .collect::<Vec<_>>();
    refresh_watchlist_locked(subscriptions, &accepted);
}

fn refresh_watchlist_locked(subscriptions: &DataSubscriptions, instruments: &[InstrumentAny]) {
    for instrument in instruments {
        if let Some(positions) = subscriptions.exposure_instruments.get(&instrument.id()) {
            for position in positions.value() {
                crate::resolve::upsert_resolve_watch_entry_from_instrument(
                    &subscriptions.watchlist,
                    instrument,
                    *position,
                );
            }
        }

        if let Ok(condition) = extract_condition_id(&instrument.id())
            && let Some(expiration) = instrument
                .expiration_ns()
                .filter(|value| value.as_u64() != 0)
            && let Some(entry) = subscriptions.watchlist.write().get_mut(&condition)
        {
            entry.expiration_ns = expiration;
        }
    }
}

// Queries Gamma's positive `closed=true` path and returns only conditions it confirms closed.
pub(super) async fn query_positive_closed_condition_ids(
    http: &PolymarketGammaHttpClient,
    condition_ids: &[String],
) -> anyhow::Result<Vec<String>> {
    Ok(query_positive_closed_markets(http, condition_ids)
        .await?
        .into_iter()
        .map(|market| market.condition_id)
        .collect())
}

// Retains closure evidence and outcomes independently of instrument parsability
pub(super) async fn query_positive_closed_markets(
    http: &PolymarketGammaHttpClient,
    condition_ids: &[String],
) -> anyhow::Result<Vec<GammaMarket>> {
    let requested = condition_ids
        .iter()
        .map(String::as_str)
        .collect::<AHashSet<_>>();
    let markets = http
        .request_markets_by_params(GetGammaMarketsParams {
            condition_ids: Some(condition_ids.to_vec()),
            closed: Some(true),
            ..Default::default()
        })
        .await?;

    Ok(markets
        .into_iter()
        .filter(|market| {
            market.closed == Some(true) && requested.contains(market.condition_id.as_str())
        })
        .collect())
}

// V2 finality can precede closed=true; CTF still requires positive closed evidence
//
// Condition IDs the unfiltered lookup does not return are re-queried with `closed=true`, so a
// market that lookup leaves out is still checked. A condition ID absent from both lookups is left
// out: closure was not observed.
async fn probe_closed_condition_ids(
    http: &PolymarketGammaHttpClient,
    condition_ids: &[String],
) -> anyhow::Result<Vec<String>> {
    let open = http
        .request_markets_by_params(GetGammaMarketsParams {
            condition_ids: Some(condition_ids.to_vec()),
            ..Default::default()
        })
        .await?;
    let returned = open
        .iter()
        .map(|market| market.condition_id.as_str())
        .collect::<AHashSet<_>>();
    let mut closed_ids = open
        .iter()
        .filter(|market| is_market_closed(market))
        .map(|market| market.condition_id.clone())
        .collect::<Vec<_>>();
    let missing = condition_ids
        .iter()
        .filter(|condition_id| !returned.contains(condition_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    if missing.is_empty() {
        return Ok(closed_ids);
    }

    closed_ids.extend(query_positive_closed_condition_ids(http, &missing).await?);

    Ok(closed_ids)
}

pub(super) async fn refresh_expired_market_closure(
    http: &PolymarketGammaHttpClient,
    cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    sender: &EventSender<DataEvent>,
    now_ns: UnixNanos,
    subscriptions: &super::subscriptions::DataSubscriptions,
    cancellation: Option<&CancellationToken>,
    clock: &AtomicTime,
) -> anyhow::Result<usize> {
    let mut carried: AHashMap<String, Vec<InstrumentId>> = AHashMap::new();

    for (id, instrument) in cache.load().iter() {
        if is_expired(instrument, now_ns)
            && market_closed(instrument) == Some(false)
            && let Ok(condition_id) = extract_condition_id(id)
        {
            carried.entry(condition_id).or_default().push(*id);
        }
    }

    let condition_ids = carried.keys().cloned().collect::<Vec<_>>();
    let chunks = condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE);
    let total_chunks = chunks.len();
    let mut closed_ids = AHashSet::new();
    let mut failed_chunks = 0;

    // A failed chunk must not discard closures the other chunks already confirmed.
    for chunk in chunks {
        match probe_closed_condition_ids(http, chunk).await {
            Ok(chunk_closed_ids) => closed_ids.extend(chunk_closed_ids),
            Err(e) => {
                failed_chunks += 1;
                log::warn!(
                    "Failed to probe market closure for {} condition ID(s): {e}",
                    chunk.len()
                );
            }
        }
    }

    let closing_ids = closed_ids
        .iter()
        .filter_map(|condition_id| carried.get(condition_id))
        .flatten()
        .copied()
        .collect::<Vec<_>>();

    // Serialize terminal application with reset. If reset wins the boundary, this old generation
    // must not mutate or publish from the cache it captured before the request.
    for condition_id in &closed_ids {
        if !subscriptions.mark_closed(condition_id, cancellation).await {
            return Ok(0);
        }
    }

    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        return Ok(0);
    }

    let mut updated_ids = Vec::new();

    // Compose against the latest cached value, so a concurrent tick size change is not discarded.
    // Guarded because `rcu` clones the whole cache, and most ticks have nothing to close.
    if !closing_ids.is_empty() {
        let ts_init = clock.get_time_ns();
        cache.rcu(|map| {
            updated_ids.clear();

            for instrument_id in &closing_ids {
                if let Some(InstrumentAny::BinaryOption(binary)) = map.get_mut(instrument_id)
                    && binary_market_closed(binary) != Some(true)
                {
                    set_market_closed(binary, true);
                    binary.ts_init = ts_init;
                    binary.ts_event = ts_init;
                    updated_ids.push(binary.id);
                }
            }
        });
    }

    publish_market_closure_updates(cache, sender, &updated_ids);

    if failed_chunks > 0 {
        anyhow::bail!(
            "Failed to probe market closure for {failed_chunks} of {total_chunks} condition ID chunk(s)"
        );
    }

    Ok(updated_ids.len())
}

fn publish_market_closure_updates(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    data_sender: &EventSender<DataEvent>,
    instrument_ids: &[InstrumentId],
) {
    for instrument_id in instrument_ids {
        // Retirement wins if the instrument was removed after the cache update
        if let Some(latest) = instruments.get_cloned(instrument_id)
            && let Err(e) = data_sender.send(DataEvent::Data(latest.into()))
        {
            log::warn!("Failed to publish market closure update for {instrument_id}: {e}");
        }
    }
}

impl PolymarketDataClient {
    pub(super) async fn bootstrap_instruments(&mut self) -> anyhow::Result<()> {
        self.provider.initialize(false).await?;

        let instruments = self
            .provider
            .store()
            .list_all()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let total = cache_and_publish_instruments(
            &self.subscriptions.closed_condition_ids,
            &self.instrument_update_state,
            &self.instruments,
            &self.subscriptions.token_meta,
            &self.data_sender,
            self.clock.get_time_ns(),
            &instruments,
            InstrumentSource::Discovery,
        );
        refresh_watchlist_deadlines(&self.subscriptions, &instruments);

        log::debug!("Published {total} Polymarket instruments to data engine");
        Ok(())
    }

    pub(super) fn register_instrument_refresh_task(&self) -> anyhow::Result<()> {
        let Some(interval_mins) = self.config.update_instruments_interval_mins else {
            return Ok(());
        };

        if interval_mins == 0 {
            return Ok(());
        }

        let interval = Duration::from_secs(interval_mins.saturating_mul(60));
        self.tasks
            .spawn(self.instrument_refresh_future(interval))
            .map_err(|e| {
                anyhow::anyhow!("failed to register Polymarket instrument refresh: {e}")
            })?;
        Ok(())
    }

    fn instrument_refresh_future(
        &self,
        interval: Duration,
    ) -> impl Future<Output = ()> + Send + use<> {
        let filters = self.provider.filters();
        let cancellation = self.cancellation_token.clone();
        let http_client = self.provider.http_client().clone();
        let instrument_config = self.config.instrument_config.clone();
        let instrument_update_state = self.instrument_update_state.clone();
        let subscriptions = self.subscriptions.clone();
        let data_sender = self.data_sender.clone();
        let clock = self.clock;

        async move {
            log::debug!("Polymarket instrument refresh task started");
            let (supplemental_tx, mut supplemental_rx) =
                tokio::sync::mpsc::channel::<SupplementalRefresh>(1);
            let discovery = async {
                loop {
                    tokio::select! {
                        () = tokio::time::sleep(interval) => {}
                        () = cancellation.cancelled() => break,
                    }
                    let result = tokio::select! {
                        () = cancellation.cancelled() => break,
                        result = refresh_scoped_instruments(
                            http_client.clone(), instrument_config.clone(), filters.clone(),
                            &subscriptions, &instrument_update_state, &data_sender, clock, &supplemental_tx,
                        ) => result,
                    };

                    if let Err(e) = result {
                        log::warn!("Failed to refresh Polymarket instruments: {e}");
                    }
                }
            };
            let retained = async {
                loop {
                    tokio::select! {
                        () = tokio::time::sleep(interval) => {}
                        () = cancellation.cancelled() => break,
                    }
                    let result = tokio::select! {
                        () = cancellation.cancelled() => break,
                        result = refresh_retained_instruments(
                            &http_client, &subscriptions, &instrument_update_state, &data_sender, clock,
                        ) => result,
                    };

                    if let Err(e) = result {
                        log::warn!("Failed to refresh retained Polymarket instruments: {e}");
                    }
                }
            };
            let supplemental = async {
                let http = http_client.for_retained_refresh();

                loop {
                    let Some((slugs, filters, completion)) = (tokio::select! {
                        () = cancellation.cancelled() => break,
                        job = supplemental_rx.recv() => job,
                    }) else {
                        break;
                    };
                    let slugs = {
                        let instruments = subscriptions.instruments.load();
                        let owned = subscriptions
                            .retained_instrument_ids()
                            .into_iter()
                            .filter_map(|id| {
                                instruments
                                    .get(&id)?
                                    .info()?
                                    .get_str("market_slug")
                                    .map(str::to_owned)
                            })
                            .collect::<AHashSet<_>>();
                        slugs
                            .into_iter()
                            .filter(|slug| !owned.contains(slug))
                            .collect()
                    };
                    let acquired = tokio::select! {
                        () = cancellation.cancelled() => break,
                        batch = crate::providers::fetch_retained_slugs(&http, slugs) => batch,
                    };
                    let seen = tokio::select! {
                        () = cancellation.cancelled() => break,
                        seen = completion => match seen { Ok(seen) => seen, Err(_) => continue },
                    };
                    let mut ids = seen;
                    let accepted = acquired
                        .into_iter()
                        .filter(|instrument| {
                            ids.insert(instrument.id())
                                && filters.iter().all(|filter| filter.accept(instrument))
                        })
                        .collect::<Vec<_>>();
                    cache_and_publish_instruments(
                        &subscriptions.closed_condition_ids,
                        &instrument_update_state,
                        &subscriptions.instruments,
                        &subscriptions.token_meta,
                        &data_sender,
                        clock.get_time_ns(),
                        &accepted,
                        InstrumentSource::Supplemental(&subscriptions),
                    );
                }
            };
            tokio::join!(biased; discovery, retained, supplemental);

            log::debug!("Polymarket instrument refresh task ended");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router,
        extract::{RawQuery, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::get,
    };
    use nautilus_common::{
        clients::DataClient, live::runner::replace_data_event_sender,
        messages::data::SubscribeInstrument,
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::Data,
        enums::AssetClass,
        identifiers::{ClientId, PositionId, Symbol},
        instruments::BinaryOption,
        types::{Currency, Price, Quantity},
    };
    use nautilus_network::{retry::RetryConfig, websocket::config::TransportBackend};
    use parking_lot::Mutex;
    use rstest::rstest;

    use super::*;
    use crate::{
        common::consts::WS_DEFAULT_SUBSCRIPTIONS,
        config::PolymarketDataClientConfig,
        filters::{PredicateFilter, TagFilter},
        http::{
            clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
            gamma::PolymarketGammaHttpClient,
        },
        websocket::pool::PolymarketMarketConnectionPool,
    };

    fn make_client(config: PolymarketDataClientConfig) -> PolymarketDataClient {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(tx);

        let gamma = PolymarketGammaHttpClient::new(
            Some("http://localhost".to_string()),
            1,
            RetryConfig::default(),
        )
        .expect("gamma client");
        let clob = PolymarketClobPublicClient::new(Some("http://localhost".to_string()), 1)
            .expect("clob client");
        let data_api = PolymarketDataApiHttpClient::new(Some("http://localhost".to_string()), 1)
            .expect("data api client");
        let ws = PolymarketMarketConnectionPool::new(
            Some("ws://localhost/ws/market".to_string()),
            false,
            TransportBackend::default(),
            WS_DEFAULT_SUBSCRIPTIONS,
        );

        PolymarketDataClient::new(
            ClientId::from("POLY-TEST"),
            config,
            gamma,
            clob,
            data_api,
            ws,
        )
        .unwrap()
    }

    #[rstest]
    fn refresh_task_spawns_for_registered_filter_without_instrument_config() {
        let mut client = make_client(PolymarketDataClientConfig {
            update_instruments_interval_mins: Some(1),
            instrument_config: None,
            ..PolymarketDataClientConfig::default()
        });
        client.add_instrument_filter(Arc::new(TagFilter::from_tag_id(84)));

        client.register_instrument_refresh_task().unwrap();

        // Without this the client would bootstrap from the filter and then freeze at its
        // connect-time universe, because the guard used to require an instrument_config.
        assert_eq!(client.tasks.len(), 1);
    }

    #[rstest]
    fn refresh_task_spawns_for_retained_instruments_without_discovery_scope() {
        let client = make_client(PolymarketDataClientConfig {
            update_instruments_interval_mins: Some(1),
            instrument_config: None,
            ..PolymarketDataClientConfig::default()
        });

        client.register_instrument_refresh_task().unwrap();

        assert_eq!(client.tasks.len(), 1);
    }

    #[rstest]
    fn refresh_task_spawns_for_accept_only_filter() {
        let mut client = make_client(PolymarketDataClientConfig {
            update_instruments_interval_mins: Some(1),
            instrument_config: None,
            ..PolymarketDataClientConfig::default()
        });
        client.add_instrument_filter(Arc::new(PredicateFilter::new("accept-all", |_| true)));

        client.register_instrument_refresh_task().unwrap();

        // Accepted cost of not probing filter sources here: an accept-only filter spawns a timer
        // whose refresh issues no HTTP. A wakeup every N minutes is cheaper than consuming a
        // dynamic filter's batch at connect.
        assert_eq!(client.tasks.len(), 1);
    }

    fn stub_instrument(
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
    ) -> InstrumentAny {
        let price_precision = price_increment.precision;
        let size_precision = size_increment.precision;
        InstrumentAny::BinaryOption(
            BinaryOption::builder()
                .instrument_id(InstrumentId::from(
                    format!("{raw_symbol}.POLYMARKET").as_str(),
                ))
                .raw_symbol(Symbol::new(raw_symbol))
                .asset_class(AssetClass::Alternative)
                .currency(Currency::pUSD())
                .activation_ns(UnixNanos::default())
                .expiration_ns(UnixNanos::from(u64::MAX))
                .price_precision(price_precision)
                .size_precision(size_precision)
                .price_increment(price_increment)
                .size_increment(size_increment)
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    #[rstest]
    #[case::p3_s2("token-a", Price::from("0.001"), Quantity::from("0.01"))]
    #[case::p5_s4("token-b", Price::from("0.00001"), Quantity::from("0.0001"))]
    fn cache_instrument_writes_both_maps(
        #[case] raw_symbol: &str,
        #[case] price_increment: Price,
        #[case] size_increment: Quantity,
    ) {
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());
        let inst = stub_instrument(raw_symbol, price_increment, size_increment);
        let expected_id = inst.id();
        let expected_token = Ustr::from(raw_symbol);
        let expected_price_precision = price_increment.precision;
        let expected_size_precision = size_increment.precision;

        cache_instrument_unchecked(&instruments, &token_meta, &inst);

        let loaded = instruments.load();
        let cached = loaded
            .get(&expected_id)
            .expect("instrument inserted into live cache");
        assert_eq!(cached.id(), expected_id);
        assert_eq!(cached.raw_symbol().as_str(), raw_symbol);

        let meta = token_meta
            .get(&expected_token)
            .expect("token_meta inserted for raw_symbol");
        assert_eq!(meta.instrument_id, expected_id);
        assert_eq!(meta.price_precision, expected_price_precision);
        assert_eq!(meta.size_precision, expected_size_precision);
    }

    #[rstest]
    fn cache_instrument_overwrites_precisions_on_second_call() {
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());
        let raw_symbol = "token-overwrite";

        let first = stub_instrument(raw_symbol, Price::from("0.01"), Quantity::from("0.1"));
        cache_instrument_unchecked(&instruments, &token_meta, &first);

        let second = stub_instrument(raw_symbol, Price::from("0.0001"), Quantity::from("0.001"));
        cache_instrument_unchecked(&instruments, &token_meta, &second);

        let meta = token_meta
            .get(&Ustr::from(raw_symbol))
            .expect("token_meta present after overwrite");
        assert_eq!(meta.price_precision, 4);
        assert_eq!(meta.size_precision, 3);
        assert_eq!(token_meta.len(), 1);
        assert_eq!(instruments.load().len(), 1);
    }

    #[rstest]
    fn cache_instrument_maintains_dual_cache_invariant() {
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());

        let samples = [
            stub_instrument("token-1", Price::from("0.001"), Quantity::from("0.01")),
            stub_instrument("token-2", Price::from("0.0001"), Quantity::from("0.01")),
            stub_instrument("token-3", Price::from("0.00001"), Quantity::from("0.001")),
        ];

        for inst in &samples {
            cache_instrument_unchecked(&instruments, &token_meta, inst);
        }

        let loaded = instruments.load();
        assert_eq!(loaded.len(), samples.len());
        for inst in loaded.values() {
            let token_id = Ustr::from(inst.raw_symbol().as_str());
            let meta = token_meta
                .get(&token_id)
                .unwrap_or_else(|| panic!("missing token_meta for {token_id}"));
            assert_eq!(meta.instrument_id, inst.id());
        }
    }

    #[rstest]
    fn cache_and_publish_merges_batch_and_preserves_live_ticks() {
        let instruments = Arc::new(AtomicMap::new());
        let instrument_update_state = Arc::new(Mutex::new(InstrumentUpdateState::default()));
        let token_meta = Arc::new(DashMap::new());
        let existing =
            stub_instrument("existing-token", Price::from("0.01"), Quantity::from("0.1"));
        let stale = stub_instrument("updated-token", Price::from("0.01"), Quantity::from("0.1"));
        cache_instrument_unchecked(&instruments, &token_meta, &existing);
        cache_instrument_unchecked(&instruments, &token_meta, &stale);
        let refreshed = stub_instrument(
            "updated-token",
            Price::from("0.01"),
            Quantity::from("0.001"),
        );
        let added = stub_instrument("added-token", Price::from("0.01"), Quantity::from("0.1"));
        let invalid = stub_instrument("invalid-token", Price::from("0.01"), Quantity::from("0.1"));
        instrument_update_state.lock().record_live_tick(
            Ustr::from("updated-token"),
            "0.001".parse().unwrap(),
            UnixNanos::from(42),
        );
        instrument_update_state.lock().record_live_tick(
            Ustr::from("invalid-token"),
            "-0.01".parse().unwrap(),
            UnixNanos::from(43),
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let total = cache_and_publish_instruments(
            &Arc::new(Mutex::new(AHashSet::new())),
            &instrument_update_state,
            &instruments,
            &token_meta,
            &tx.into(),
            UnixNanos::default(),
            &[refreshed.clone(), added.clone(), invalid.clone()],
            InstrumentSource::Discovery,
        );
        let events = std::iter::from_fn(|| rx.try_recv().ok())
            .map(|event| match event {
                DataEvent::Data(Data::Instrument(instrument)) => *instrument,
                _ => panic!("Expected instrument event"),
            })
            .collect::<Vec<_>>();
        let updated = instruments.get_cloned(&refreshed.id()).unwrap();
        let meta = token_meta.get(&Ustr::from("updated-token")).unwrap();

        assert_eq!(total, 2);
        assert_eq!(instruments.len(), 3);
        assert_eq!(token_meta.len(), 3);
        assert_eq!(instruments.get_cloned(&existing.id()), Some(existing));
        assert_eq!(instruments.get_cloned(&added.id()), Some(added.clone()));
        assert!(!instruments.contains_key(&invalid.id()));
        assert!(!token_meta.contains_key(&Ustr::from("invalid-token")));
        assert_eq!(updated.price_increment(), Price::from("0.001"));
        assert_eq!(updated.size_increment(), Quantity::from("0.001"));
        assert_eq!(updated.ts_event(), UnixNanos::from(42));
        assert_eq!(meta.instrument_id, refreshed.id());
        assert_eq!(meta.price_precision, 4);
        assert_eq!(meta.size_precision, 3);
        assert_eq!(events, vec![updated, added]);
    }

    #[rstest]
    #[case::terminal_condition(false)]
    #[case::retired_generation(true)]
    fn cache_and_publish_skips_terminal_or_retired(#[case] retired: bool) {
        let instruments = Arc::new(AtomicMap::new());
        let instrument_update_state = Arc::new(Mutex::new(InstrumentUpdateState::default()));
        let token_meta = Arc::new(DashMap::new());
        let closed_condition_ids = Arc::new(Mutex::new(AHashSet::new()));

        if retired {
            instrument_update_state.lock().retire_generation();
        } else {
            closed_condition_ids.lock().insert("terminal".to_string());
        }

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let instrument =
            stub_instrument("terminal-token", Price::from("0.01"), Quantity::from("0.1"));

        let total = cache_and_publish_instruments(
            &closed_condition_ids,
            &instrument_update_state,
            &instruments,
            &token_meta,
            &tx.into(),
            UnixNanos::default(),
            std::slice::from_ref(&instrument),
            InstrumentSource::Discovery,
        );

        assert_eq!(total, 0);
        assert!(instruments.load().is_empty());
        assert!(token_meta.is_empty());
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case::open(false, 1)]
    #[case::closed(true, 0)]
    fn cache_and_publish_past_end_market_honors_gamma_closure(
        #[case] closed: bool,
        #[case] expected: usize,
    ) {
        let mut binary = past_end_open_instrument();
        set_market_closed(&mut binary, closed);
        let instrument = InstrumentAny::BinaryOption(binary);
        let now_ns = Instrument::expiration_ns(&instrument).unwrap();
        let instruments = Arc::new(AtomicMap::new());
        let token_meta = Arc::new(DashMap::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let total = cache_and_publish_instruments(
            &Arc::new(Mutex::new(AHashSet::new())),
            &Arc::new(Mutex::new(InstrumentUpdateState::default())),
            &instruments,
            &token_meta,
            &tx.into(),
            now_ns,
            std::slice::from_ref(&instrument),
            InstrumentSource::Discovery,
        );
        let events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();

        assert_eq!(total, expected);
        assert_eq!(instruments.load().len(), expected);
        assert_eq!(token_meta.len(), expected);
        assert_eq!(events.len(), expected);

        if !closed {
            assert_eq!(
                instruments.get_cloned(&instrument.id()),
                Some(instrument.clone())
            );
            let token = token_meta
                .get(&Ustr::from(instrument.raw_symbol().as_str()))
                .unwrap();
            assert_eq!(token.instrument_id, instrument.id());
            assert_eq!(token.neg_risk, Some(true));
            assert!(
                matches!(&events[0], DataEvent::Data(Data::Instrument(published)) if published.as_ref() == &instrument)
            );
        }
    }

    #[rstest]
    #[tokio::test]
    async fn bootstrap_and_refresh_publish_past_end_open_market(
        #[values(false, true)] refresh: bool,
    ) {
        let addr = market_closure_server(false, false).await;
        let http = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();
        let instrument_id = past_end_open_instrument().id;

        let config = crate::config::PolymarketInstrumentProviderConfig {
            load_ids: Some(vec![instrument_id]),
            ..Default::default()
        };

        let mut client = make_client(PolymarketDataClientConfig::default());
        client.provider =
            crate::providers::PolymarketInstrumentProvider::new(http.clone(), Some(config.clone()))
                .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();

        if refresh {
            let (supplemental, _) = tokio::sync::mpsc::channel(1);
            let total = refresh_scoped_instruments(
                http,
                Some(config),
                vec![],
                &client.subscriptions,
                &client.instrument_update_state,
                &client.data_sender,
                client.clock,
                &supplemental,
            )
            .await
            .unwrap();
            assert_eq!(total, 2);
        } else {
            client.bootstrap_instruments().await.unwrap();
        }

        let events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();

        assert_eq!(client.instruments.load().len(), 2);
        assert_eq!(client.subscriptions.token_meta.len(), 2);
        assert_eq!(events.len(), 2);

        for event in events {
            let DataEvent::Data(Data::Instrument(instrument)) = event else {
                panic!("expected instrument");
            };

            assert!(is_expired(&instrument, client.clock.get_time_ns()));
            assert_eq!(market_closed(&instrument), Some(false));
            assert_eq!(
                client.instruments.get_cloned(&instrument.id()),
                Some(*instrument)
            );
        }
    }

    #[rstest]
    fn apply_live_instrument_orders_cache_and_publication() {
        let instruments = Arc::new(AtomicMap::new());
        let instrument_update_state = Arc::new(Mutex::new(InstrumentUpdateState::default()));
        let token_meta = Arc::new(DashMap::new());
        let closed_condition_ids = Arc::new(Mutex::new(AHashSet::new()));
        let instrument = stub_instrument(
            "ordered-token",
            Price::from("0.0100"),
            Quantity::from("0.1"),
        );
        let (callback_tx, callback_rx) = std::sync::mpsc::sync_channel(0);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
        let thread_state = instrument_update_state.clone();
        let terminal_conditions = closed_condition_ids.clone();

        let apply_thread = std::thread::spawn(move || {
            apply_live_instrument(
                &closed_condition_ids,
                &thread_state,
                &instruments,
                &token_meta,
                &instrument,
                |_| {
                    callback_tx.send(()).expect("callback started");
                    release_rx.recv().expect("callback released");
                },
            )
        });

        callback_rx.recv().expect("callback start signal");
        let update_locked = instrument_update_state.try_lock().is_none();
        let terminal_locked = terminal_conditions.try_lock().is_none();
        release_tx.send(()).expect("release callback");
        assert!(apply_thread.join().expect("apply thread"));
        assert!(update_locked);
        assert!(terminal_locked);
    }

    fn past_end_open_market() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../test_data/gamma_market_past_end_date_open.json"
        ))
        .unwrap()
    }

    // Mirrors the live Gamma funnel, which records closure state the historical loader must not
    // carry. See `parse_markets_with_transient`.
    fn past_end_open_instrument() -> BinaryOption {
        instrument_from_market(past_end_open_market())
    }

    fn instrument_from_market(market: serde_json::Value) -> BinaryOption {
        let market = serde_json::from_value(market).unwrap();
        let definitions = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&definitions[0], UnixNanos::default())
                .unwrap();
        let InstrumentAny::BinaryOption(mut binary) = instrument else {
            panic!("Expected BinaryOption, was {instrument:?}");
        };
        set_market_closed(&mut binary, false);
        binary
    }

    fn refresh_market(index: u64) -> serde_json::Value {
        let mut market = past_end_open_market();
        market["conditionId"] = format!("0x{index:064x}").into();
        market["clobTokenIds"] = format!("[\"{}\",\"{}\"]", index * 2 + 1, index * 2 + 2).into();
        market["slug"] = serde_json::Value::Null;
        market["endDate"] = "2099-01-01T00:00:00Z".into();
        market
    }

    #[derive(Clone)]
    struct RefreshServer {
        requests: tokio::sync::mpsc::UnboundedSender<Vec<String>>,
        release: Arc<tokio::sync::Semaphore>,
        discovery: Arc<AtomicUsize>,
        discovery_release: Arc<tokio::sync::Semaphore>,
        discovery_fail: Arc<AtomicBool>,
        closed: Arc<AtomicBool>,
        fail: bool,
    }

    async fn refresh_response(
        State(state): State<RefreshServer>,
        RawQuery(query): RawQuery,
    ) -> Response {
        let ids = requested_condition_ids(query.as_deref());
        let _ = state.requests.send(ids.clone());
        let markets = if query
            .as_deref()
            .is_some_and(|query| query.contains("slug="))
        {
            state.release.acquire().await.unwrap().forget();
            if state.fail {
                return StatusCode::BAD_REQUEST.into_response();
            }
            let mut market = refresh_market(1000);
            market["endDate"] = "2098-01-01T00:00:00Z".into();
            vec![market]
        } else if ids.is_empty() {
            state.discovery_release.acquire().await.unwrap().forget();
            if state.discovery_fail.load(Ordering::SeqCst) {
                return StatusCode::BAD_REQUEST.into_response();
            }
            vec![refresh_market(
                state.discovery.fetch_add(1, Ordering::SeqCst) as u64,
            )]
        } else {
            state.release.acquire().await.unwrap().forget();
            if state.fail {
                return StatusCode::BAD_REQUEST.into_response();
            }
            ids.iter()
                .map(|id| {
                    let mut market = refresh_market(u64::from_str_radix(&id[2..], 16).unwrap());
                    if state.closed.load(Ordering::SeqCst) {
                        market["closed"] = true.into();
                        market["endDate"] = "2020-01-01T00:00:00Z".into();
                    }
                    market
                })
                .collect()
        };
        Json(serde_json::json!({"markets": markets, "next_cursor": null})).into_response()
    }

    async fn refresh_server(
        blocked: bool,
        fail: bool,
    ) -> (
        PolymarketGammaHttpClient,
        RefreshServer,
        tokio::sync::mpsc::UnboundedReceiver<Vec<String>>,
    ) {
        let (requests, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = RefreshServer {
            requests,
            release: Arc::new(tokio::sync::Semaphore::new(if blocked { 0 } else { 1000 })),
            discovery: Arc::new(AtomicUsize::new(1000)),
            discovery_release: Arc::new(tokio::sync::Semaphore::new(10000)),
            discovery_fail: Arc::new(AtomicBool::new(false)),
            closed: Arc::new(AtomicBool::new(false)),
            fail,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/markets/keyset", get(refresh_response))
            .with_state(state.clone());

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();
        (http, state, rx)
    }

    #[rstest]
    #[case::retained(false)]
    #[case::released_during_lookup(true)]
    #[tokio::test]
    async fn refresh_batches_slugless_markets_and_releases_owners(
        #[case] release_during_lookup: bool,
    ) {
        let (http, server, mut queries) = refresh_server(release_during_lookup, false).await;
        let mut client = make_client(PolymarketDataClientConfig::default());

        for index in 0..101 {
            let mut binary = instrument_from_market(refresh_market(index));
            binary.expiration_ns = UnixNanos::from(1);
            let instrument = InstrumentAny::BinaryOption(binary);
            let id = instrument.id();
            cache_instrument_unchecked(
                &client.instruments,
                &client.subscriptions.token_meta,
                &instrument,
            );

            match index % 4 {
                0 => {
                    client
                        .subscribe_instrument(SubscribeInstrument::new(
                            id,
                            Some(client.client_id),
                            None,
                            UUID4::new(),
                            UnixNanos::default(),
                            None,
                            None,
                        ))
                        .unwrap();
                }
                1 => {
                    client.subscriptions.active_quote_subs.insert(id);
                }
                2 => crate::resolve::upsert_resolve_watch_entry_from_instrument(
                    &client.resolve_poll_watchlist,
                    &instrument,
                    PositionId::from(format!("P-{index}").as_str()),
                ),
                _ => {
                    client
                        .subscriptions
                        .exposure_instruments
                        .insert(id, Vec::new());
                }
            }
        }

        client
            .subscriptions
            .instrument_subs
            .insert(InstrumentId::from("legacy.POLYMARKET"));
        // Explicit definition ownership must survive a missing client-local definition.
        client
            .instruments
            .remove(&instrument_from_market(refresh_market(0)).id);
        client.instrument_update_state.lock().record_live_tick(
            Ustr::from("1"),
            Decimal::new(1, 3),
            UnixNanos::from(42),
        );

        for entry in client.resolve_poll_watchlist.write().values_mut() {
            entry.paused = true;
        }
        let subscriptions = &client.subscriptions;
        let state = &client.instrument_update_state;
        let sender = &client.data_sender;
        let release = || {
            subscriptions.instrument_subs.clear();
            subscriptions.active_quote_subs.clear();
            client.resolve_poll_watchlist.write().clear();
            subscriptions.exposure_instruments.clear();
        };

        if release_during_lookup {
            let refresh =
                refresh_retained_instruments(&http, subscriptions, state, sender, client.clock);
            tokio::pin!(refresh);
            tokio::select! {
                result = &mut refresh => panic!("lookup completed before release: {result:?}"),
                request = queries.recv() => assert_eq!(request.unwrap().len(), 100),
            }
            release();
            server.release.add_permits(1);
            assert_eq!(refresh.await.unwrap(), 0);
            assert!(queries.try_recv().is_err());
            return;
        }
        let total =
            refresh_retained_instruments(&http, subscriptions, state, sender, client.clock).await;
        let expected = 101;
        assert_eq!(total.unwrap(), expected);
        assert_eq!(queries.try_recv().unwrap().len(), 100);
        assert_eq!(queries.try_recv().unwrap().len(), 1);
        assert!(queries.try_recv().is_err());
        assert!(
            client
                .instruments
                .load()
                .values()
                .all(|instrument| instrument.expiration_ns().unwrap().as_u64() > 1)
        );
        assert!(
            client
                .resolve_poll_watchlist
                .read()
                .values()
                .all(|entry| entry.expiration_ns.as_u64() > 1 && entry.paused)
        );
        assert_eq!(
            client
                .instruments
                .get_cloned(&instrument_from_market(refresh_market(0)).id)
                .unwrap()
                .price_increment(),
            Price::from("0.001")
        );
        release();
        assert_eq!(
            refresh_retained_instruments(&http, subscriptions, state, sender, client.clock)
                .await
                .unwrap(),
            0
        );
        assert!(queries.try_recv().is_err());
        assert_eq!(client.instruments.load().len(), expected);
        assert!(client.subscriptions.ws_open_tokens.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn discovery_continues_during_retained_lookup(#[values(false, true)] fail: bool) {
        let (http, server, mut queries) = refresh_server(true, fail).await;
        let config = crate::config::PolymarketInstrumentProviderConfig {
            filters: Some(std::collections::HashMap::from([(
                "tag_id".into(),
                "84".into(),
            )])),
            ..Default::default()
        };
        let mut client = make_client(PolymarketDataClientConfig {
            instrument_config: Some(config.clone()),
            ..Default::default()
        });
        client.provider =
            crate::providers::PolymarketInstrumentProvider::new(http, Some(config)).unwrap();
        let mut binary = instrument_from_market(refresh_market(0));
        binary.expiration_ns = UnixNanos::from(1);
        let instrument = InstrumentAny::BinaryOption(binary);
        cache_instrument_unchecked(
            &client.instruments,
            &client.subscriptions.token_meta,
            &instrument,
        );
        client
            .subscriptions
            .active_quote_subs
            .insert(instrument.id());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let task = tokio::spawn(client.instrument_refresh_future(Duration::from_millis(50)));
        tokio::time::timeout(Duration::from_secs(2), async {
            while queries.recv().await.unwrap().is_empty() {}
            for _ in 0..4 {
                let DataEvent::Data(Data::Instrument(published)) = rx.recv().await.unwrap() else {
                    panic!("expected instrument")
                };
                assert_ne!(published.id(), instrument.id());
            }
            server.release.add_permits(2);

            if fail {
                while queries.recv().await.unwrap().is_empty() {}
                assert_eq!(
                    client.instruments.get_cloned(&instrument.id()),
                    Some(instrument)
                );
            } else {
                loop {
                    if let DataEvent::Data(Data::Instrument(published)) = rx.recv().await.unwrap()
                        && published.id() == instrument.id()
                    {
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();
        client.cancellation_token.cancel();
        task.await.unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn retained_response_cannot_overwrite_newer_http(#[values(false, true)] discovery: bool) {
        let (http, server, mut queries) = refresh_server(true, false).await;
        let client = make_client(PolymarketDataClientConfig::default());
        let mut binary = instrument_from_market(refresh_market(0));
        let id = binary.id;
        client.subscriptions.instrument_subs.insert(id);
        let refresh = refresh_retained_instruments(
            &http,
            &client.subscriptions,
            &client.instrument_update_state,
            &client.data_sender,
            client.clock,
        );
        tokio::pin!(refresh);
        tokio::select! {
            result = &mut refresh => panic!("unexpected completion: {result:?}"),
            _ = queries.recv() => {},
        }
        binary.expiration_ns = UnixNanos::from(42);
        let newer = InstrumentAny::BinaryOption(binary);

        if discovery {
            cache_and_publish_instruments(
                &client.subscriptions.closed_condition_ids,
                &client.instrument_update_state,
                &client.instruments,
                &client.subscriptions.token_meta,
                &client.data_sender,
                client.clock.get_time_ns(),
                std::slice::from_ref(&newer),
                InstrumentSource::Discovery,
            );
        } else {
            assert!(apply_live_instrument(
                &client.subscriptions.closed_condition_ids,
                &client.instrument_update_state,
                &client.instruments,
                &client.subscriptions.token_meta,
                &newer,
                |_| {}
            ));
        }
        server.release.add_permits(1);
        assert_eq!(refresh.await.unwrap(), 0);
        assert_eq!(client.instruments.get_cloned(&id), Some(newer));
    }

    #[rstest]
    fn older_gamma_revision_cannot_overwrite_cache() {
        let client = make_client(PolymarketDataClientConfig::default());
        let apply = |market| {
            apply_live_instrument(
                &client.subscriptions.closed_condition_ids,
                &client.instrument_update_state,
                &client.instruments,
                &client.subscriptions.token_meta,
                &InstrumentAny::BinaryOption(instrument_from_market(market)),
                |_| {},
            )
        };
        let mut market = refresh_market(0);
        market["updatedAt"] = "2030-01-01T00:00:00Z".into();
        assert!(apply(market.clone()));
        market["updatedAt"] = "2029-01-01T00:00:00Z".into();
        assert!(!apply(market));
    }

    #[rstest]
    #[tokio::test]
    async fn retained_closure_revises_future_deadline_without_settlement() {
        let (http, server, _) = refresh_server(false, false).await;
        server.closed.store(true, Ordering::SeqCst);
        let client = make_client(PolymarketDataClientConfig::default());
        let instrument = InstrumentAny::BinaryOption(instrument_from_market(refresh_market(0)));
        let id = instrument.id();
        let position = PositionId::from("RESTORED");
        client
            .subscriptions
            .exposure_instruments
            .insert(id, vec![position]);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert_eq!(
            refresh_retained_instruments(
                &http,
                &client.subscriptions,
                &client.instrument_update_state,
                &tx.into(),
                client.clock
            )
            .await
            .unwrap(),
            1
        );
        let condition = extract_condition_id(&id).unwrap();
        assert!(client.subscriptions.is_condition_closed(&condition));
        let entry = client.resolve_poll_watchlist.read()[&condition].clone();
        assert!(entry.expiration_ns < client.clock.get_time_ns());
        assert!(
            entry
                .tracked
                .values()
                .any(|tracked| tracked.open_position_ids.contains(&position))
        );
        let events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], DataEvent::Data(Data::Instrument(instrument)) if market_closed(instrument) == Some(true))
        );
        assert!(!apply_live_instrument(
            &client.subscriptions.closed_condition_ids,
            &client.instrument_update_state,
            &client.instruments,
            &client.subscriptions.token_meta,
            &instrument,
            |_| panic!("terminal reopened")
        ));
    }

    #[derive(Debug)]
    struct MixedFilter;

    impl InstrumentFilter for MixedFilter {
        fn query_params(&self) -> Option<GetGammaMarketsParams> {
            Some(GetGammaMarketsParams::default())
        }
        fn market_slugs(&self) -> Option<Vec<String>> {
            Some(vec!["retained".into()])
        }
        fn accept(&self, instrument: &InstrumentAny) -> bool {
            instrument.expiration_ns()
                == Some(instrument_from_market(refresh_market(1000)).expiration_ns)
        }
    }

    #[rstest]
    #[tokio::test]
    async fn mixed_optional_sources_do_not_block_or_mask_discovery(
        #[values(false, true)] fail: bool,
    ) {
        let (http, server, _) = refresh_server(true, fail).await;
        let mut client = make_client(PolymarketDataClientConfig::default());
        client.provider =
            crate::providers::PolymarketInstrumentProvider::new(http.clone(), None).unwrap();
        client.add_instrument_filter(Arc::new(MixedFilter));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let task = tokio::spawn(client.instrument_refresh_future(Duration::from_millis(10)));
        let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let DataEvent::Data(Data::Instrument(instrument)) = event else {
            panic!("expected discovery")
        };
        assert_eq!(
            instrument.expiration_ns(),
            Some(instrument_from_market(refresh_market(1000)).expiration_ns)
        );
        server.release.add_permits(100);
        client.cancellation_token.cancel();
        task.await.unwrap();
        // The aggregate provider path uses the same precedence before accept().
        server.discovery.store(1000, Ordering::SeqCst);
        let result = crate::providers::fetch_instruments(&http, &[Arc::new(MixedFilter)])
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert!(
            result
                .iter()
                .all(|instrument| MixedFilter.accept(instrument))
        );
    }

    #[rstest]
    #[tokio::test]
    async fn retained_refresh_continues_when_discovery_stalls_or_fails(
        #[values(false, true)] fail: bool,
    ) {
        let (http, server, _) = refresh_server(false, false).await;
        server.discovery_release.forget_permits(10000);
        server.discovery_fail.store(fail, Ordering::SeqCst);
        let mut client = make_client(PolymarketDataClientConfig {
            instrument_config: Some(crate::config::PolymarketInstrumentProviderConfig {
                filters: Some(std::collections::HashMap::from([(
                    "tag_id".into(),
                    "84".into(),
                )])),
                ..Default::default()
            }),
            ..Default::default()
        });
        client.provider = crate::providers::PolymarketInstrumentProvider::new(http, None).unwrap();
        let id = instrument_from_market(refresh_market(0)).id;
        client.subscriptions.instrument_subs.insert(id);
        client
            .instrument_update_state
            .lock()
            .discovered_ids
            .insert(id);

        if fail {
            server.discovery_release.add_permits(100);
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let task = tokio::spawn(client.instrument_refresh_future(Duration::from_millis(10)));
        let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(event, DataEvent::Data(Data::Instrument(instrument)) if instrument.id() == id)
        );
        client.cancellation_token.cancel();
        task.await.unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn gamma_quota_reserves_admission_for_discovery() {
        let (http, _, mut queries) = refresh_server(false, false).await;
        let quota_started = tokio::time::Instant::now();

        for _ in 0..100 {
            http.request_markets_by_params(GetGammaMarketsParams::default())
                .await
                .unwrap();
        }

        while queries.try_recv().is_ok() {}
        let retained = http.for_retained_refresh();
        let workload = async {
            for index in 0..3 {
                retained
                    .request_markets_by_params(GetGammaMarketsParams {
                        condition_ids: Some(vec![format!("0x{:064x}", 1)]),
                        ..Default::default()
                    })
                    .await
                    .unwrap();

                if index == 0 {
                    assert!(quota_started.elapsed() >= Duration::from_millis(590));
                }
            }
        };
        tokio::pin!(workload);
        let started = tokio::time::Instant::now();
        let discovery = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            http.request_markets_by_params(GetGammaMarketsParams::default())
                .await
                .unwrap();
        };
        tokio::join!(&mut workload, discovery);
        let requests = std::iter::from_fn(|| queries.try_recv().ok()).collect::<Vec<_>>();
        assert_eq!(requests.len(), 4);
        assert!(requests.iter().position(Vec::is_empty).unwrap() < 3);
        assert!(started.elapsed() >= Duration::from_millis(2400));
    }

    #[rstest]
    fn cached_closure_updates_observation_timestamps_once() {
        let instrument = InstrumentAny::BinaryOption(past_end_open_instrument());
        let condition_id = extract_condition_id(&instrument.id()).unwrap();
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(instrument.id(), instrument.clone());
        let update_state = Arc::new(Mutex::new(InstrumentUpdateState::default()));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = tx.into();
        let observed_at = UnixNanos::from(100);
        let clock = AtomicTime::new(false, observed_at);
        let publish = || {
            publish_cached_condition_closed(
                &condition_id,
                &update_state,
                &instruments,
                &sender,
                &clock,
            )
        };

        assert_eq!(publish(), 1);

        let DataEvent::Data(Data::Instrument(published)) = rx.try_recv().unwrap() else {
            panic!("Expected closure instrument event");
        };
        let cached = instruments.get_cloned(&instrument.id()).unwrap();

        for updated in [&cached, &published] {
            assert_eq!(updated.ts_init(), observed_at);
            assert_eq!(updated.ts_event(), observed_at);
            assert_eq!(market_closed(updated), Some(true));
            assert_eq!(updated.price_increment(), instrument.price_increment());
        }

        clock.set_time(UnixNanos::from(101));
        assert_eq!(publish(), 0);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case::missing(false)]
    #[case::cached(true)]
    #[tokio::test]
    async fn closure_publication_reports_closed_receiver(#[case] cached: bool) {
        if super::super::test_logging::run_in_isolated_process() {
            return;
        }

        let _capture = super::super::test_logging::capture_logs().await;
        let instrument = InstrumentAny::BinaryOption(past_end_open_instrument());
        let instrument_id = instrument.id();
        let instruments = Arc::new(AtomicMap::new());
        if cached {
            instruments.insert(instrument_id, instrument.clone());
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        let sender = tx.into();

        publish_market_closure_updates(&instruments, &sender, &[instrument_id]);

        let warnings = super::super::test_logging::messages()
            .into_iter()
            .filter_map(|(level, message)| (level == log::Level::Warn).then_some(message))
            .collect::<Vec<_>>();

        let expected = if cached {
            vec![format!(
                "Failed to publish market closure update for {instrument_id}: channel closed"
            )]
        } else {
            Vec::new()
        };

        assert_eq!(warnings, expected);
        assert_eq!(
            instruments.get_cloned(&instrument_id),
            cached.then_some(instrument),
        );
    }

    fn requested_condition_ids(query: Option<&str>) -> Vec<String> {
        query
            .unwrap_or_default()
            .split('&')
            .filter_map(|pair| pair.strip_prefix("condition_ids="))
            .map(ToString::to_string)
            .collect()
    }

    async fn market_closure_response(
        State((closed, fail)): State<(bool, bool)>,
        RawQuery(query): RawQuery,
    ) -> Response {
        if fail {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        let wants_closed = query.unwrap_or_default().contains("closed=true");
        let mut market = past_end_open_market();
        market["closed"] = wants_closed.into();
        market["orderPriceMinTickSize"] = serde_json::json!(0.01);
        let markets = (wants_closed == closed)
            .then_some(market)
            .into_iter()
            .collect::<Vec<_>>();
        Json(serde_json::json!({"markets": markets, "next_cursor": null})).into_response()
    }

    async fn market_closure_server(closed: bool, fail: bool) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/markets/keyset", get(market_closure_response))
            .with_state((closed, fail));

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    #[rstest]
    #[case(false, false)]
    #[case(true, false)]
    #[case(false, true)]
    #[tokio::test]
    async fn market_closure_refresh_uses_positive_signal_without_replacing_definition(
        #[case] closed: bool,
        #[case] fail: bool,
    ) {
        let binary = past_end_open_instrument();
        let probe_time = UnixNanos::from(binary.expiration_ns.as_u64() + 1);
        let observed_at = UnixNanos::from(probe_time.as_u64() + 1);
        let clock = AtomicTime::new(false, probe_time);
        let instrument = InstrumentAny::BinaryOption(binary);
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(instrument.id(), instrument.clone());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let addr = market_closure_server(closed, fail).await;
        let client = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();

        let (ws_tx, _ws_rx) = tokio::sync::mpsc::unbounded_channel();
        let ws_client =
            crate::websocket::pool::PolymarketMarketPoolHandle::test_single_shard(ws_tx, &[]);

        let subscriptions = crate::data::DataSubscriptions::new(
            instruments.clone(),
            Arc::default(),
            Arc::new(AtomicMap::new()),
            Arc::new(Mutex::new(())),
            ws_client,
            false,
        );
        let sender = tx.into();
        let mut refresh = Box::pin(refresh_expired_market_closure(
            &client,
            &instruments,
            &sender,
            probe_time,
            &subscriptions,
            None,
            &clock,
        ));
        assert!(futures_util::poll!(&mut refresh).is_pending());
        clock.set_time(observed_at);
        let updated = crate::http::parse::rebuild_instrument_with_tick_size(
            &instrument,
            "0.005",
            probe_time,
            probe_time,
        )
        .unwrap();
        instruments.insert(instrument.id(), updated);
        let result = refresh.await;

        if fail {
            assert!(result.is_err());
            assert!(rx.try_recv().is_err());
            return;
        }

        assert_eq!(result.unwrap(), usize::from(closed));
        let cached = instruments.get_cloned(&instrument.id()).unwrap();
        assert_eq!(market_closed(&cached), Some(closed));
        // Gamma reports 0.01; preserve the tick size updated while the probe was pending
        assert_eq!(cached.price_increment(), Price::from("0.005"));

        if closed {
            assert_eq!(cached.ts_init(), observed_at);
            assert_eq!(cached.ts_event(), observed_at);
            let event = rx.try_recv().expect("closure instrument event");

            let DataEvent::Data(Data::Instrument(published)) = event else {
                panic!("Expected instrument event, was {event:?}");
            };
            assert_eq!(published.id(), instrument.id());
            assert_eq!(market_closed(&published), Some(true));
            assert_eq!(published.ts_init(), observed_at);
            assert_eq!(published.ts_event(), observed_at);
            assert_eq!(published.price_increment(), Price::from("0.005"));
            assert_eq!(
                refresh_expired_market_closure(
                    &client,
                    &instruments,
                    &sender,
                    observed_at,
                    &subscriptions,
                    None,
                    &clock,
                )
                .await
                .unwrap(),
                0
            );
            assert!(rx.try_recv().is_err());
        } else {
            assert!(rx.try_recv().is_err());
        }
    }

    // Serves the first request only, echoing every requested condition ID back as closed.
    async fn first_chunk_only_response(
        State(requests): State<Arc<AtomicUsize>>,
        RawQuery(query): RawQuery,
    ) -> Response {
        if requests.fetch_add(1, Ordering::SeqCst) > 0 {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }

        let markets = requested_condition_ids(query.as_deref())
            .into_iter()
            .map(|condition_id| {
                let mut market = past_end_open_market();
                market["conditionId"] = condition_id.into();
                market["closed"] = true.into();
                market
            })
            .collect::<Vec<_>>();

        Json(serde_json::json!({"markets": markets, "next_cursor": null})).into_response()
    }

    #[rstest]
    #[tokio::test]
    async fn market_closure_refresh_keeps_earlier_chunk_results_when_a_later_chunk_fails() {
        let base = past_end_open_instrument();
        let instruments = Arc::new(AtomicMap::new());

        // One more candidate than a single Gamma request accepts, so the probe spans two chunks.
        for i in 0..=GAMMA_CONDITION_IDS_BATCH_SIZE {
            let mut binary = base.clone();
            binary.raw_symbol = Symbol::new(format!("0xCOND{i:04}-Yes"));
            binary.id = InstrumentId::new(binary.raw_symbol, base.id.venue);
            instruments.insert(binary.id, InstrumentAny::BinaryOption(binary));
        }

        let requests = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/markets/keyset", get(first_chunk_only_response))
            .with_state(requests);

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let client = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();

        let (ws_tx, _ws_rx) = tokio::sync::mpsc::unbounded_channel();
        let ws_client =
            crate::websocket::pool::PolymarketMarketPoolHandle::test_single_shard(ws_tx, &[]);

        let subscriptions = crate::data::DataSubscriptions::new(
            instruments.clone(),
            Arc::default(),
            Arc::new(AtomicMap::new()),
            Arc::new(Mutex::new(())),
            ws_client,
            false,
        );
        let closed_condition_ids = subscriptions.closed_condition_ids.clone();
        let result = refresh_expired_market_closure(
            &client,
            &instruments,
            &tx.clone().into(),
            UnixNanos::from(u64::MAX),
            &subscriptions,
            None,
            &AtomicTime::default(),
        )
        .await;

        assert!(result.is_err());

        let closed = instruments
            .load()
            .values()
            .filter(|instrument| market_closed(instrument) == Some(true))
            .count();

        assert_eq!(closed, GAMMA_CONDITION_IDS_BATCH_SIZE);
        assert_eq!(
            closed_condition_ids.lock().len(),
            GAMMA_CONDITION_IDS_BATCH_SIZE,
        );
    }
}
