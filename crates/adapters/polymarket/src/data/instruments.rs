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

use std::{future::Future, sync::Arc, time::Duration};

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use jiff::Timestamp;
use nautilus_common::{
    live::sender::EventSender, messages::DataEvent, providers::InstrumentProvider,
};
use nautilus_core::{AtomicMap, Params, UnixNanos, time::AtomicTime};
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
        gamma::PolymarketGammaHttpClient,
        models::{GammaEvent, GammaMarket},
        parse::is_market_closed,
        query::GetGammaMarketsParams,
    },
    providers::{extract_condition_id, validated_condition_id},
    resolve::ResolveWatchlist,
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
    metadata_epoch: u64,
    metadata_epochs: AHashMap<InstrumentId, u64>,
    pub(super) retired: bool,
}

impl InstrumentUpdateState {
    pub(super) fn metadata_epoch(&self) -> u64 {
        self.metadata_epoch
    }

    pub(super) fn record_metadata(
        &mut self,
        pending: &[InstrumentAny],
        instruments: &AtomicMap<InstrumentId, InstrumentAny>,
    ) {
        let cached = instruments.load();
        self.metadata_epochs.retain(|id, _| cached.contains_key(id));
        for instrument in pending {
            if cached.get(&instrument.id()).is_some_and(|current| {
                current.info() == instrument.info()
                    && current.expiration_ns() == instrument.expiration_ns()
            }) {
                continue;
            }
            self.metadata_epoch = self
                .metadata_epoch
                .checked_add(1)
                .expect("Metadata epoch exhausted");
            self.metadata_epochs
                .insert(instrument.id(), self.metadata_epoch);
        }
    }

    pub(super) fn newer_cached(
        &self,
        incoming: &InstrumentAny,
        instruments: &AtomicMap<InstrumentId, InstrumentAny>,
        request_epoch: Option<u64>,
    ) -> Option<InstrumentAny> {
        fn revision(instrument: &InstrumentAny) -> Option<Timestamp> {
            serde_json::from_str::<serde_json::Value>(instrument.info()?.get_str("gamma_market")?)
                .ok()?
                .get("updatedAt")?
                .as_str()?
                .parse()
                .ok()
        }

        let cached = instruments.load();
        let cached = cached.get(&incoming.id())?;
        let newer = match revision(incoming).zip(revision(cached)) {
            Some((incoming, cached)) if incoming != cached => incoming < cached,
            // Equal or absent venue revisions cannot order overlapping HTTP responses
            _ => request_epoch.is_some_and(|epoch| {
                self.metadata_epochs
                    .get(&incoming.id())
                    .is_some_and(|current| *current > epoch)
            }),
        };
        newer.then(|| cached.clone())
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
        instruments: &AtomicMap<InstrumentId, InstrumentAny>,
        event_metadata: &mut AHashMap<usize, (Arc<serde_json::Value>, Arc<GammaEvent>)>,
    ) -> anyhow::Result<InstrumentAny> {
        let token_id = Ustr::from(instrument.raw_symbol().as_str());
        let mut instrument = match self.live_ticks.get(&token_id) {
            Some(live_tick) => crate::http::parse::rebuild_instrument_with_tick_size(
                instrument,
                &live_tick.tick_size.to_string(),
                live_tick.ts_event,
                instrument.ts_init(),
            )?,
            None => instrument.clone(),
        };
        let cached = instruments.load();

        // Market responses omit enclosing event snapshots, retain compatible discovery context
        if let InstrumentAny::BinaryOption(binary) = &mut instrument
            && let Some(InstrumentAny::BinaryOption(previous)) = cached.get(&binary.id)
            && (binary.event_id.is_none() || binary.event_id == previous.event_id)
            && let (Some(info), Some(previous_info)) =
                (binary.info.as_ref(), previous.info.as_ref())
            && info.get_str("gamma_event").is_none()
            && previous_info.get_str("gamma_event").is_some()
            && let Some(raw_market) = info.get_str("gamma_market")
        {
            let mut market: GammaMarket = serde_json::from_str(raw_market)?;
            if market.events.as_ref().is_some_and(|events| {
                events
                    .iter()
                    .any(|event| Some(Ustr::from(event.id.as_str())) != previous.event_id)
            }) {
                return Ok(instrument);
            }

            let mut info = info.to_shared_map();
            let previous_info = previous_info.to_shared_map();
            let raw_event = Arc::clone(previous_info.get("gamma_event").unwrap());
            let event_key = Arc::as_ptr(&raw_event) as usize;
            let event = if let Some((_, event)) = event_metadata.get(&event_key) {
                Arc::clone(event)
            } else {
                let mut event: GammaEvent = serde_json::from_str(raw_event.as_str().unwrap())?;
                event.markets = Vec::new();
                event.raw = String::new();
                let event = Arc::new(event);
                // Retain the source so its address cannot be reused within this batch
                event_metadata.insert(event_key, (raw_event, Arc::clone(&event)));
                event
            };

            for key in ["gamma_event", "event_id"] {
                if let Some(value) = previous_info.get(key) {
                    info.entry(key.to_string())
                        .or_insert_with(|| Arc::clone(value));
                }
            }

            market.game_id = market.game_id.or_else(|| event.game_id.clone());
            if let Some(game_id) = &market.game_id {
                info.entry("game_id".into())
                    .or_insert_with(|| Arc::new(serde_json::json!(game_id)));
            }
            market.parent_event = Some(event);
            crate::http::parse::enrich_market_fee_schedule(&mut market);
            if let Some(schedule) = market.fee_schedule {
                info.insert("fee_schedule".into(), Arc::new(schedule.to_info()));
            }
            binary.event_id = binary.event_id.or(previous.event_id);
            binary.info = Some(Params::from_shared(info));
        }

        Ok(instrument)
    }
}

pub(super) fn cache_instrument_unchecked(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    watchlist: &Arc<ResolveWatchlist>,
    instrument: &InstrumentAny,
) {
    cache_instruments_unchecked(
        instruments,
        token_meta,
        watchlist,
        std::slice::from_ref(instrument),
    );
}

pub(super) fn cache_instruments_unchecked(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    watchlist: &Arc<ResolveWatchlist>,
    pending: &[InstrumentAny],
) {
    if pending.is_empty() {
        return;
    }

    // Timeout decisions must observe the deadline belonging to the cached definitions
    let mut watchlist = watchlist.write();

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

    let instruments = instruments.load();
    let conditions = pending
        .iter()
        .filter_map(|instrument| extract_condition_id(&instrument.id()).ok())
        .collect::<AHashSet<_>>();

    for condition_id in conditions {
        let Some(entry) = watchlist.get_mut(&condition_id) else {
            continue;
        };

        if let Some(expiration) = entry
            .tracked
            .values()
            .filter_map(|tracked| instruments.get(&tracked.instrument_id)?.expiration_ns())
            .filter(|expiration| expiration.as_u64() != 0)
            .max()
        {
            entry.expiration_ns = expiration;
        }
    }
}

// Applies one instrument while serializing live tick composition, cache mutation,
// and queued publication with terminal resolution.
pub(super) fn apply_live_instrument(
    closed_condition_ids: &Arc<Mutex<AHashSet<String>>>,
    update_state: &mut MutexGuard<'_, InstrumentUpdateState>,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    watchlist: &Arc<ResolveWatchlist>,
    instrument: &InstrumentAny,
    apply: impl FnOnce(&InstrumentAny),
) -> bool {
    if update_state.retired
        || update_state
            .newer_cached(instrument, instruments, None)
            .is_some()
    {
        return false;
    }

    let mut events = Default::default();
    let instrument = match update_state.compose_instrument(instrument, instruments, &mut events) {
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

    update_state.record_metadata(std::slice::from_ref(&instrument), instruments);
    cache_instrument_unchecked(instruments, token_meta, watchlist, &instrument);
    apply(&instrument);
    true
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

pub(super) fn cache_and_publish_instruments(
    subscriptions: &DataSubscriptions,
    instrument_update_state: &Arc<Mutex<InstrumentUpdateState>>,
    data_sender: &EventSender<DataEvent>,
    now_ns: UnixNanos,
    request_epoch: Option<u64>,
    instruments: Vec<InstrumentAny>,
) -> usize {
    let instruments_cache = &subscriptions.instruments;
    let mut update_state = instrument_update_state.lock();
    if update_state.retired {
        return 0;
    }

    let mut pending = Vec::with_capacity(instruments.len());
    let mut event_metadata = AHashMap::new();

    for instrument in instruments {
        if update_state
            .newer_cached(&instrument, instruments_cache, request_epoch)
            .is_some()
        {
            continue;
        }

        // Explicit definition owners can receive closure before their first cached definition
        if is_instrument_expired_and_not_reported_open(&instrument, now_ns)
            && !(market_closed(&instrument) == Some(true)
                && (instruments_cache.contains_key(&instrument.id())
                    || subscriptions.instrument_subs.contains(&instrument.id())))
        {
            log::debug!(
                "Skipping expired instrument {} during live cache publish",
                instrument.id()
            );
            continue;
        }

        match update_state.compose_instrument(&instrument, instruments_cache, &mut event_metadata) {
            Ok(instrument) => pending.push(instrument),
            Err(e) => log::error!(
                "Failed to apply live tick to instrument {}: {e}",
                instrument.id()
            ),
        }
    }

    let mut terminal_conditions = subscriptions.closed_condition_ids.lock();
    pending.retain(|instrument| {
        !extract_condition_id(&instrument.id())
            .is_ok_and(|condition_id| terminal_conditions.contains(&condition_id))
    });

    if pending.is_empty() {
        return 0;
    }

    // Bootstrap leaves resolution ownership available for later admission
    let retained = request_epoch.map(|_| subscriptions.retained_instrument_ids());
    update_state.record_metadata(&pending, instruments_cache);
    cache_instruments_unchecked(
        instruments_cache,
        &subscriptions.token_meta,
        &subscriptions.watchlist,
        &pending,
    );

    for instrument in &pending {
        if market_closed(instrument) == Some(true)
            && retained
                .as_ref()
                .is_some_and(|ids| ids.contains(&instrument.id()))
            && let Ok(condition_id) = extract_condition_id(&instrument.id())
        {
            terminal_conditions.insert(condition_id);
        }
    }
    let total = pending.len();

    // Keep the update guard through publication so live tick updates cannot overtake this batch
    for instrument in pending {
        let instrument_id = instrument.id();
        if let Err(e) = data_sender.send(DataEvent::Data(instrument.into())) {
            log::warn!("Failed to publish instrument {instrument_id}: {e}");
        }
    }

    total
}

#[derive(Clone, Copy)]
enum InstrumentRefreshScope<'a> {
    Discovery(
        &'a crate::config::PolymarketInstrumentProviderConfig,
        &'a [Arc<dyn InstrumentFilter>],
    ),
    Retained,
}

async fn refresh_instruments(
    http_client: PolymarketGammaHttpClient,
    scope: InstrumentRefreshScope<'_>,
    subscriptions: &DataSubscriptions,
    instrument_update_state: &Arc<Mutex<InstrumentUpdateState>>,
    data_sender: &EventSender<DataEvent>,
    clock: &'static AtomicTime,
) -> usize {
    let request_epoch = instrument_update_state.lock().metadata_epoch();
    let publish = |instruments| {
        cache_and_publish_instruments(
            subscriptions,
            instrument_update_state,
            data_sender,
            clock.get_time_ns(),
            Some(request_epoch),
            instruments,
        )
    };

    if let InstrumentRefreshScope::Discovery(instrument_config, filters) = scope {
        let (mut refreshed, closing, _) = match crate::providers::fetch_configured_with_closed(
            &http_client,
            instrument_config,
            filters,
        )
        .await
        {
            Ok(instruments) => instruments,
            Err(e) => {
                log::warn!("Failed to refresh Polymarket discovery: {e}");
                return 0;
            }
        };
        let _owner_guard = subscriptions.owner_lock.lock();
        let mut retained = subscriptions.retained_instrument_ids();
        for instrument in &refreshed {
            retained.remove(&instrument.id());
        }
        // Preserve closing definitions for tracked siblings before the condition becomes terminal
        refreshed.extend(
            closing
                .into_iter()
                .filter(|instrument| retained.remove(&instrument.id())),
        );
        return publish(refreshed);
    }

    let condition_ids = subscriptions
        .retained_instrument_ids()
        .iter()
        .filter_map(|id| validated_condition_id(id).ok())
        .filter(|id| !subscriptions.is_condition_closed(id))
        .collect::<AHashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut refreshed = Vec::new();
    let mut total = 0;

    for chunk in condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
        let mut missing = chunk.to_vec();
        for closed in [None, Some(true)] {
            if missing.is_empty() {
                break;
            }

            match http_client
                .request_instruments_by_params(GetGammaMarketsParams {
                    condition_ids: Some(missing.clone()),
                    closed,
                    ..Default::default()
                })
                .await
            {
                Ok(mut batch) => {
                    let _owner_guard = subscriptions.owner_lock.lock();
                    let owned = subscriptions.retained_instrument_ids();
                    batch.retain(|instrument| {
                        owned.contains(&instrument.id())
                            && (closed.is_none() || market_closed(instrument) == Some(true))
                    });
                    let returned = batch
                        .iter()
                        .filter_map(|instrument| extract_condition_id(&instrument.id()).ok())
                        .collect::<AHashSet<_>>();
                    missing.retain(|id| !returned.contains(id));
                    let publish_now = {
                        let watchlist = subscriptions.watchlist.read();
                        let cached = subscriptions.instruments.load();
                        batch.iter().any(|instrument| {
                            market_closed(instrument) == Some(true)
                                || instrument.expiration_ns().is_some_and(|expiration| {
                                    expiration.as_u64() != 0
                                        && (cached
                                            .get(&instrument.id())
                                            .and_then(Instrument::expiration_ns)
                                            != Some(expiration)
                                            || extract_condition_id(&instrument.id())
                                                .ok()
                                                .and_then(|id| watchlist.get(&id))
                                                .is_some_and(|entry| {
                                                    expiration != entry.expiration_ns
                                                }))
                                })
                        })
                    };
                    refreshed.extend(batch);

                    // Commit changed deadlines before another request can let polling time out
                    if publish_now {
                        total += publish(std::mem::take(&mut refreshed));
                    }
                }
                Err(e) => {
                    log::warn!("Failed to refresh retained Polymarket instruments: {e}");
                    break;
                }
            }
        }
    }

    total + publish(refreshed)
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

        let total = cache_and_publish_instruments(
            &self.subscriptions,
            &self.instrument_update_state,
            &self.data_sender,
            self.clock.get_time_ns(),
            None,
            self.provider
                .store()
                .list_all()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
        );

        log::debug!("Published {total} Polymarket instruments to data engine");
        Ok(())
    }

    pub(super) fn register_instrument_refresh_task(&self) -> anyhow::Result<()> {
        let Some(interval_mins) = self.config.update_instruments_interval_mins else {
            return Ok(());
        };

        // Subscriptions and position watches can acquire refresh ownership after connect
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
        let instrument_config = self.config.instrument_config.clone().unwrap_or_default();
        let subscriptions = self.subscriptions.clone();
        let instrument_update_state = self.instrument_update_state.clone();
        let data_sender = self.data_sender.clone();
        let clock = self.clock;

        async move {
            log::debug!("Polymarket instrument refresh task started");

            let refresh = async |scope: InstrumentRefreshScope<'_>| {
                loop {
                    tokio::select! {
                        () = tokio::time::sleep(interval) => {}
                        () = cancellation.cancelled() => {
                            log::debug!("Polymarket instrument refresh task cancelled");
                            break;
                        }
                    }

                    let total = tokio::select! {
                        biased;
                        () = cancellation.cancelled() => break,
                        total = refresh_instruments(
                            http_client.clone(),
                            scope,
                            &subscriptions,
                            &instrument_update_state,
                            &data_sender,
                            clock,
                        ) => total,
                    };

                    if total > 0 {
                        log::debug!("Refreshed {total} Polymarket instruments into the live cache");
                    }
                }
            };

            // Discovery pagination must not hold up subsequent tracked-market refresh cycles
            tokio::join!(
                refresh(InstrumentRefreshScope::Discovery(
                    &instrument_config,
                    &filters
                )),
                refresh(InstrumentRefreshScope::Retained),
            );

            log::debug!("Polymarket instrument refresh task ended");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router,
        extract::{RawQuery, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::get,
    };
    use nautilus_common::{
        clients::DataClient,
        live::runner::replace_data_event_sender,
        messages::{
            DataResponse,
            data::{RequestInstrument, RequestInstruments},
        },
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

    async fn refresh_scoped_instruments(
        http: PolymarketGammaHttpClient,
        config: Option<crate::config::PolymarketInstrumentProviderConfig>,
        filters: Vec<Arc<dyn InstrumentFilter>>,
        subscriptions: &DataSubscriptions,
        updates: &Arc<Mutex<InstrumentUpdateState>>,
        sender: &EventSender<DataEvent>,
        clock: &'static AtomicTime,
    ) -> usize {
        let config = config.unwrap_or_default();
        let mut total = 0;
        for scope in [
            InstrumentRefreshScope::Discovery(&config, &filters),
            InstrumentRefreshScope::Retained,
        ] {
            total +=
                refresh_instruments(http.clone(), scope, subscriptions, updates, sender, clock)
                    .await;
        }
        total
    }

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

    async fn gamma_server(app: Router, timeout_secs: u64) -> PolymarketGammaHttpClient {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            timeout_secs,
            RetryConfig::default(),
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
    fn refresh_task_spawns_without_discovery_scope_for_future_owners() {
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

        cache_instrument_unchecked(&instruments, &token_meta, &Arc::default(), &inst);

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
        cache_instrument_unchecked(&instruments, &token_meta, &Arc::default(), &first);

        let second = stub_instrument(raw_symbol, Price::from("0.0001"), Quantity::from("0.001"));
        cache_instrument_unchecked(&instruments, &token_meta, &Arc::default(), &second);

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
            cache_instrument_unchecked(&instruments, &token_meta, &Arc::default(), inst);
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
        let client = make_client(PolymarketDataClientConfig::default());
        let instruments = client.instruments.clone();
        let instrument_update_state = client.instrument_update_state.clone();
        let token_meta = client.subscriptions.token_meta.clone();
        let existing =
            stub_instrument("existing-token", Price::from("0.01"), Quantity::from("0.1"));
        let stale = stub_instrument("updated-token", Price::from("0.01"), Quantity::from("0.1"));
        cache_instrument_unchecked(
            &instruments,
            &token_meta,
            &client.subscriptions.watchlist,
            &existing,
        );
        cache_instrument_unchecked(
            &instruments,
            &token_meta,
            &client.subscriptions.watchlist,
            &stale,
        );
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
            &client.subscriptions,
            &instrument_update_state,
            &tx.into(),
            UnixNanos::default(),
            Some(0),
            vec![refreshed.clone(), added.clone(), invalid.clone()],
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
    fn cache_and_publish_skips_terminal_or_retired(
        #[case] retired: bool,
        #[values(false, true)] is_refresh: bool,
    ) {
        let client = make_client(PolymarketDataClientConfig::default());
        let instruments = client.instruments.clone();
        let instrument_update_state = client.instrument_update_state.clone();
        let token_meta = client.subscriptions.token_meta.clone();
        let closed_condition_ids = client.subscriptions.closed_condition_ids.clone();

        if retired {
            instrument_update_state.lock().retire_generation();
        } else {
            closed_condition_ids.lock().insert("terminal".to_string());
        }

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let instrument =
            stub_instrument("terminal-token", Price::from("0.01"), Quantity::from("0.1"));

        let total = cache_and_publish_instruments(
            &client.subscriptions,
            &instrument_update_state,
            &tx.into(),
            UnixNanos::default(),
            is_refresh.then_some(0),
            vec![instrument],
        );

        assert_eq!(total, 0);
        assert!(instruments.load().is_empty());
        assert!(token_meta.is_empty());
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case::open(false, false, 1)]
    #[case::closed(true, false, 0)]
    #[case::subscribed_closed(true, true, 1)]
    fn cache_and_publish_past_end_market_honors_gamma_closure(
        #[case] closed: bool,
        #[case] subscribed: bool,
        #[case] expected: usize,
    ) {
        let mut binary = past_end_open_instrument();
        set_market_closed(&mut binary, closed);
        let instrument = InstrumentAny::BinaryOption(binary);
        let now_ns = Instrument::expiration_ns(&instrument).unwrap();
        let client = make_client(PolymarketDataClientConfig::default());
        let instruments = client.instruments.clone();
        let token_meta = client.subscriptions.token_meta.clone();

        if subscribed {
            client.subscriptions.instrument_subs.insert(instrument.id());
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let total = cache_and_publish_instruments(
            &client.subscriptions,
            &client.instrument_update_state,
            &tx.into(),
            now_ns,
            Some(0),
            vec![instrument.clone()],
        );
        let events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();

        assert_eq!(total, expected);
        assert_eq!(instruments.load().len(), expected);
        assert_eq!(token_meta.len(), expected);
        assert_eq!(events.len(), expected);

        if expected != 0 {
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
    #[case::past_end_open(false, false)]
    #[case::closed_without_owner(true, false)]
    #[case::closed_with_owner(true, true)]
    #[tokio::test]
    async fn bootstrap_and_refresh_publish_open_and_closed_markets(
        #[values(false, true)] refresh: bool,
        #[case] closed: bool,
        #[case] tracked: bool,
    ) {
        let app = Router::new().route(
            "/markets/keyset",
            get(move || async move {
                let mut market = past_end_open_market();
                market["closed"] = closed.into();
                if closed {
                    market["endDate"] = "2099-01-01T00:00:00Z".into();
                }
                Json(serde_json::json!({"markets": [market], "next_cursor": null}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();
        let instrument_id = past_end_open_instrument().id;
        let condition_id = extract_condition_id(&instrument_id).unwrap();

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

        if tracked {
            client.subscriptions.instrument_subs.insert(instrument_id);
        }

        if refresh {
            let total = refresh_scoped_instruments(
                http,
                Some(config),
                vec![],
                &client.subscriptions,
                &client.instrument_update_state,
                &client.data_sender,
                client.clock,
            )
            .await;
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

            assert_eq!(is_expired(&instrument, client.clock.get_time_ns()), !closed);
            assert_eq!(market_closed(&instrument), Some(closed));
            assert_eq!(
                client.instruments.get_cloned(&instrument.id()),
                Some(*instrument)
            );
        }

        if closed {
            let terminal = refresh && tracked;
            assert_eq!(
                client.subscriptions.is_condition_closed(&condition_id),
                terminal
            );

            for kind in [
                super::super::subscriptions::SubscriptionKind::Status,
                super::super::subscriptions::SubscriptionKind::Close,
            ] {
                assert_eq!(
                    client
                        .subscriptions
                        .add_resolution_subscription_intent(instrument_id, kind),
                    !terminal
                );
            }
        }
        server.abort();
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
                &mut thread_state.lock(),
                &instruments,
                &token_meta,
                &Arc::default(),
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
        let market = serde_json::from_value(past_end_open_market()).unwrap();
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
    #[case::extended(1, None, true)]
    #[case::shortened(u64::MAX, None, true)]
    #[case::closed(u64::MAX, Some("2020-01-01T00:00:00Z"), true)]
    #[case::closed_before_end(u64::MAX, Some("2099-12-01T00:00:00Z"), true)]
    #[case::closed_during_refresh(u64::MAX, Some("2020-01-01T00:00:00Z"), false)]
    #[tokio::test]
    async fn refresh_batches_tracked_markets_and_stops_after_release(
        #[case] initial_expiration: u64,
        #[case] closed_end_date: Option<&'static str>,
        #[case] discovery_closed: bool,
        #[values(false, true)] discovery_fails: bool,
    ) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let closing_date = Arc::new(Mutex::new(closed_end_date));
        let closed = closed_end_date.is_some();
        let app = Router::new()
            .route("/markets/keyset", get(retained_response))
            .with_state((
                requests.clone(),
                closing_date.clone(),
                discovery_fails,
                discovery_closed,
            ));
        let http = gamma_server(app, 1).await;
        let mut client = make_client(PolymarketDataClientConfig::default());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let subscriptions = &client.subscriptions;
        let watchlist = &subscriptions.watchlist;
        let expiration = UnixNanos::from(initial_expiration);

        // One discovered, one terminal, 101 retained, and one cache-only market
        let event = serde_json::from_value(serde_json::json!({
            "id": "event-123", "gameId": "parent-game",
            "markets": (0..104).map(retained_market).collect::<Vec<_>>(),
        }))
        .unwrap();
        let markets = crate::http::gamma::flatten_event_markets(vec![event]);
        let (instruments, _) =
            crate::http::gamma::parse_markets_with_transient(&markets, UnixNanos::default());
        let event_snapshot = instruments[0].info().unwrap().to_shared_map()["gamma_event"].clone();

        for (index, instruments) in instruments.chunks(2).enumerate() {
            for mut instrument in instruments.iter().cloned() {
                if let InstrumentAny::BinaryOption(binary) = &mut instrument {
                    binary.expiration_ns = expiration;
                }
                let id = instrument.id();
                client.instruments.insert(id, instrument.clone());

                if index == 103 {
                    continue;
                }

                match index % 7 {
                    0 if index != 0
                        || (discovery_closed && instrument.raw_symbol().as_str() != "2") =>
                    {
                        subscriptions.instrument_subs.insert(id);
                    }
                    1..=5 => {
                        subscriptions.intents()[index % 7 - 1].insert(id);
                    }
                    _ => crate::resolve::upsert_resolve_watch_entry_from_instrument(
                        watchlist,
                        &instrument,
                        PositionId::from("P-1"),
                    ),
                }
            }
        }

        for (offset, intents) in subscriptions.intents().into_iter().take(3).enumerate() {
            let index = offset + 104;
            let id =
                InstrumentId::from(format!("0x{index:064x}-{}.POLYMARKET", index * 2 + 1).as_str());
            intents.insert(id);
            subscriptions.pending_auto_loads.lock().insert(id);
        }
        let terminal = format!("0x{:064x}", 1);
        subscriptions.closed_condition_ids.lock().insert(terminal);

        for entry in watchlist.write().values_mut() {
            entry.paused = true;
        }
        let tick = Price::from("0.005");
        client.instrument_update_state.lock().record_live_tick(
            Ustr::from("5"),
            tick.as_decimal(),
            UnixNanos::from(42),
        );
        let refresh = || {
            refresh_scoped_instruments(
                http.clone(),
                None,
                vec![
                    Arc::new(TagFilter::from_tag_id(84)),
                    Arc::new(PredicateFilter::new("yes-only", |instrument| {
                        instrument.raw_symbol().as_str() == "1"
                    })),
                ],
                subscriptions,
                &client.instrument_update_state,
                &client.data_sender,
                client.clock,
            )
        };
        let closes_later = closed && !discovery_closed && !discovery_fails;
        let expected_total = 204 + usize::from(!discovery_fails && (!closed || !discovery_closed));
        assert_eq!(refresh().await, expected_total);
        let queries = std::mem::take(&mut *requests.lock());
        let batch_sizes = if closed {
            let last_batch = 1 + usize::from(discovery_fails || !discovery_closed);
            vec![0, 100, 100, last_batch, last_batch]
        } else {
            vec![0, 100, 2]
        };
        assert_eq!(
            queries.iter().map(Vec::len).collect::<Vec<_>>(),
            batch_sizes
        );
        assert_eq!(
            queries.into_iter().flatten().collect::<AHashSet<_>>(),
            (0..103)
                .filter(|index| *index != 1
                    && (!closed || !discovery_closed || discovery_fails || *index != 0))
                .map(|index| format!("0x{index:064x}"))
                .collect(),
        );
        let published = std::iter::from_fn(|| rx.try_recv().ok())
            .map(|event| match event {
                DataEvent::Data(Data::Instrument(instrument)) => instrument,
                _ => panic!("Expected instrument event"),
            })
            .collect::<Vec<_>>();
        assert_eq!(published.len(), expected_total);
        for instrument in &published {
            let InstrumentAny::BinaryOption(binary) = instrument.as_ref() else {
                unreachable!()
            };
            let info = binary.info.as_ref().unwrap();
            assert_eq!(binary.event_id, Some(Ustr::from("event-123")));
            assert_eq!(info.get_str("event_id"), Some("event-123"));
            assert_eq!(info.get_str("game_id"), Some("parent-game"));
            assert!(std::ptr::eq(
                info.get("gamma_event").unwrap(),
                event_snapshot.as_ref()
            ));
        }

        assert!(published.iter().any(|instrument| {
            instrument.raw_symbol().as_str() == "5" && instrument.price_increment() == tick
        }));
        let expected = closed_end_date
            .unwrap_or("2099-12-01T00:00:00Z")
            .parse::<UnixNanos>()
            .unwrap();
        assert!(
            watchlist
                .read()
                .values()
                .all(|entry| entry.expiration_ns == expected && entry.paused)
        );
        assert!(
            published
                .iter()
                .skip(usize::from(closes_later))
                .all(|instrument| {
                    let fee_schedule = instrument.info().unwrap().get("fee_schedule").unwrap();
                    instrument.expiration_ns() == Some(expected)
                        && (!closed || market_closed(instrument) == Some(true))
                        && fee_schedule["rebateRate"] == "0.20"
                })
        );

        if closed && discovery_closed && !discovery_fails {
            let condition_id = retained_market(0).condition_id;
            subscriptions
                .retire_condition(&condition_id, [], None)
                .await;
            assert!(!client.instruments.contains_key(&published[0].id()));
            let sibling_id = watchlist.read()[&condition_id].tracked["2"].instrument_id;
            let sibling = client.instruments.get_cloned(&sibling_id).unwrap();
            crate::resolve::upsert_resolve_watch_entry_from_instrument(
                watchlist,
                &sibling,
                PositionId::from("P-1"),
            );
            assert_eq!(refresh().await, 0);
            assert_eq!(watchlist.read()[&condition_id].expiration_ns, expected);
            requests.lock().clear();
        }

        // A stale discovery response must not reopen a market learned to be closed
        *closing_date.lock() = None;
        let cached = client.instruments.load();
        subscriptions.instrument_subs.clear();
        for intents in subscriptions.intents() {
            intents.clear();
        }
        watchlist.write().clear();
        assert_eq!(refresh().await, usize::from(!closed && !discovery_fails));
        assert_eq!(*requests.lock(), vec![Vec::<String>::new()]);
        if closed || discovery_fails {
            assert_eq!(client.instruments.load().as_ref(), cached.as_ref());
            assert!(rx.try_recv().is_err());
        }
        assert!(subscriptions.ws_open_tokens.is_empty());
    }

    #[rstest]
    #[case::discovery(0, 0, true, 0, 0)]
    #[case::slow_discovery(0, 2, true, 8, 2)]
    #[case::retained(1, 2, true, 2, 0)]
    #[case::later_batch(2, 102, true, 202, 100)]
    #[case::unchanged_deadlines(2, 102, false, 2, 102)]
    #[tokio::test]
    async fn refresh_publishes_before_cancelling_pending_requests(
        #[case] gate_at: usize,
        #[case] market_count: usize,
        #[case] deadline_changes: bool,
        #[case] expected_publications: usize,
        #[case] expected_not_expired: usize,
        #[values(false, true)] initially_watched: bool,
    ) {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let app = Router::new()
            .route(
                "/markets/keyset",
                get(
                    move |State((started, release, requests, discoveries)): State<RefreshGate>,
                          RawQuery(query): RawQuery| async move {
                        let ids = requested_condition_ids(query.as_deref());
                        let discovery_repeat =
                            ids.is_empty() && discoveries.fetch_add(1, Ordering::SeqCst) > 0;
                        let blocked = if ids.is_empty() {
                            gate_at == 0
                        } else {
                            let request = requests.fetch_add(1, Ordering::SeqCst);
                            request == if gate_at == 0 { 2 } else { gate_at - 1 }
                        };

                        if blocked {
                            started.notify_one();
                        }

                        if blocked || discovery_repeat {
                            release.acquire().await.unwrap().forget();
                        }
                        let markets = if ids.is_empty() {
                            vec![retained_market(0)]
                        } else {
                            ids.iter()
                                .map(|id| {
                                    retained_market(usize::from_str_radix(&id[2..], 16).unwrap())
                                })
                                .collect()
                        };
                        Json(serde_json::json!({"markets": markets, "next_cursor": null}))
                    },
                ),
            )
            .with_state((
                started.clone(),
                release.clone(),
                Arc::new(AtomicUsize::new(0)),
                Arc::new(AtomicUsize::new(0)),
            ));
        let http = gamma_server(app, 60).await;
        let mut client = make_client(PolymarketDataClientConfig::default());
        client.provider = crate::providers::PolymarketInstrumentProvider::new(http, None).unwrap();
        client.add_instrument_filter(Arc::new(TagFilter::from_tag_id(84)));

        if market_count > 0 {
            let markets = (1..=market_count).map(retained_market).collect::<Vec<_>>();
            let (instruments, _) =
                crate::http::gamma::parse_markets_with_transient(&markets, UnixNanos::default());

            for mut instrument in instruments {
                if deadline_changes && let InstrumentAny::BinaryOption(binary) = &mut instrument {
                    binary.expiration_ns = UnixNanos::from(1);
                }
                client
                    .instruments
                    .insert(instrument.id(), instrument.clone());

                if initially_watched {
                    crate::resolve::upsert_resolve_watch_entry_from_instrument(
                        &client.subscriptions.watchlist,
                        &instrument,
                        PositionId::from("P-1"),
                    );
                } else {
                    client.subscriptions.instrument_subs.insert(instrument.id());
                }
            }
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let refresh = client.instrument_refresh_future(Duration::ZERO);
        tokio::pin!(refresh);
        tokio::select! {
            () = &mut refresh => panic!("Refresh completed before response"),
            () = started.notified() => {}
        }

        if !initially_watched {
            let instruments = client.instruments.load();

            for id in client.subscriptions.instrument_subs.iter() {
                crate::resolve::upsert_resolve_watch_entry_from_instrument(
                    &client.subscriptions.watchlist,
                    instruments.get(&*id).unwrap(),
                    PositionId::from("P-1"),
                );
            }
        }

        tokio::select! {
            () = &mut refresh => panic!("Refresh completed before response"),
            result = tokio::time::timeout(Duration::from_secs(5), async {
                for _ in 0..expected_publications {
                    rx.recv().await.unwrap();
                }
            }) => result.expect("Tracked definitions did not publish while discovery was blocked"),
        }

        if market_count > 0 {
            let selection = crate::resolve::collect_resolve_watch_selection(
                &client.subscriptions.watchlist.read(),
                client.clock.get_time_ns(),
                0,
                0,
                crate::resolve::ResolveWatchSelectionMode::AutoPoll,
            );
            assert_eq!(selection.skipped_not_expired, expected_not_expired);
            assert_eq!(
                selection.pause_condition_ids.len(),
                market_count - expected_not_expired,
            );
        }
        assert!(rx.is_empty());
        client.stop_client();
        let cancelled = futures_util::poll!(&mut refresh).is_ready();
        release.add_permits(2);
        assert!(cancelled);
        assert_eq!(
            client.instruments.load().len(),
            (market_count + usize::from(gate_at > 0)) * 2
        );
        assert!(rx.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn overlapping_metadata_requests_preserve_current_definitions(
        #[values("discovery", "single", "bulk")] origin: &str,
        #[values("retained", "aba", "tick", "unchanged", "versioned", "unrelated")] change: &str,
    ) {
        let old_date = "2020-01-01T00:00:00Z";
        let new_date = "2099-12-01T00:00:00Z";
        let definition = |extended: bool, revision: u8| {
            let mut market = serde_json::to_value(retained_market(0)).unwrap();
            market["endDate"] = serde_json::json!(if extended { new_date } else { old_date });

            if change == "versioned" {
                market["updatedAt"] = serde_json::json!(format!("2026-01-0{revision}T00:00:00Z"));
            }
            market
        };
        let initial = definition(change == "aba", 1);
        let delayed = definition(!matches!(change, "retained" | "aba"), 3);
        let intervening = definition(change == "retained", 2);
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/markets/keyset",
            get({
                let started = started.clone();
                let release = release.clone();
                let requests = requests.clone();
                move || {
                    let started = started.clone();
                    let release = release.clone();
                    let index = requests.fetch_add(1, Ordering::SeqCst);
                    let mut market = if index == 0 {
                        delayed.clone()
                    } else {
                        intervening.clone()
                    };

                    if index == 2 {
                        market["endDate"] = serde_json::json!(new_date);
                    }
                    async move {
                        if index == 0 {
                            started.notify_one();
                            release.acquire().await.unwrap().forget();
                        }
                        Json(serde_json::json!({"markets": [market], "next_cursor": null}))
                    }
                }
            }),
        );
        let http = gamma_server(app, 1).await;
        let mut client = make_client(PolymarketDataClientConfig::default());
        client.provider =
            crate::providers::PolymarketInstrumentProvider::new(http.clone(), None).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let mut market: GammaMarket = serde_json::from_value(initial).unwrap();
        crate::http::parse::enrich_market_fee_schedule(&mut market);
        let (cached, _) =
            crate::http::gamma::parse_markets_with_transient(&[market], UnixNanos::default());

        for instrument in &cached {
            client
                .instruments
                .insert(instrument.id(), instrument.clone());
            crate::resolve::upsert_resolve_watch_entry_from_instrument(
                &client.subscriptions.watchlist,
                instrument,
                PositionId::from("P-1"),
            );
        }
        let condition_id = extract_condition_id(&cached[0].id()).unwrap();
        let config = crate::config::PolymarketInstrumentProviderConfig {
            load_all: true,
            ..Default::default()
        };
        let mut pending: futures_util::future::LocalBoxFuture<'_, Option<DataResponse>> =
            if origin == "discovery" {
                Box::pin(async {
                    let count = refresh_instruments(
                        http.clone(),
                        InstrumentRefreshScope::Discovery(&config, &[]),
                        &client.subscriptions,
                        &client.instrument_update_state,
                        &client.data_sender,
                        client.clock,
                    )
                    .await;
                    assert_eq!(
                        count,
                        if matches!(change, "retained" | "aba") {
                            0
                        } else {
                            2
                        }
                    );
                    None
                })
            } else {
                let request_id = UUID4::new();

                if origin == "single" {
                    client
                        .request_instrument(RequestInstrument::new(
                            cached[0].id(),
                            None,
                            None,
                            Some(client.client_id),
                            request_id,
                            UnixNanos::default(),
                            None,
                        ))
                        .unwrap();
                } else {
                    client
                        .request_instruments(RequestInstruments::new(
                            None,
                            None,
                            Some(client.client_id),
                            None,
                            request_id,
                            UnixNanos::default(),
                            None,
                        ))
                        .unwrap();
                }
                Box::pin(async {
                    loop {
                        if let DataEvent::Response(response) = rx.recv().await.unwrap() {
                            break Some(response);
                        }
                    }
                })
            };
        tokio::select! {
            _ = &mut pending => panic!("Request completed before release"),
            () = started.notified() => {}
        }
        client.instrument_update_state.lock().record_live_tick(
            cached[0].raw_symbol().inner(),
            "0.005".parse().unwrap(),
            UnixNanos::from(1),
        );
        assert!(apply_live_instrument(
            &client.subscriptions.closed_condition_ids,
            &mut client.instrument_update_state.lock(),
            &client.instruments,
            &client.subscriptions.token_meta,
            &client.subscriptions.watchlist,
            &cached[0],
            |_| {}
        ));

        if !matches!(change, "tick" | "unrelated") {
            for _ in 0..if change == "aba" { 2 } else { 1 } {
                assert_eq!(
                    refresh_instruments(
                        http.clone(),
                        InstrumentRefreshScope::Retained,
                        &client.subscriptions,
                        &client.instrument_update_state,
                        &client.data_sender,
                        client.clock
                    )
                    .await,
                    2
                );
            }
        } else if change == "unrelated" {
            let (other, _) = crate::http::gamma::parse_markets_with_transient(
                &[retained_market(1)],
                UnixNanos::default(),
            );
            assert_eq!(
                cache_and_publish_instruments(
                    &client.subscriptions,
                    &client.instrument_update_state,
                    &client.data_sender,
                    client.clock.get_time_ns(),
                    None,
                    other
                ),
                2
            );
        }

        if change == "aba" {
            assert_eq!(
                client
                    .instruments
                    .get_cloned(&cached[0].id())
                    .unwrap()
                    .info(),
                cached[0].info()
            );
        }
        release.add_permits(1);
        let response = tokio::time::timeout(Duration::from_secs(3), pending)
            .await
            .unwrap();
        let expected = new_date.parse::<UnixNanos>().unwrap();
        let mut observed = client
            .instruments
            .get_cloned(&cached[0].id())
            .into_iter()
            .collect::<Vec<_>>();

        match response {
            Some(DataResponse::Instrument(response)) => observed.push(response.data),
            Some(DataResponse::Instruments(response)) => observed.extend(
                response
                    .data
                    .into_iter()
                    .filter(|instrument| instrument.id() == cached[0].id()),
            ),
            None => {}
            _ => panic!("Unexpected response"),
        }

        for instrument in observed {
            assert_eq!(instrument.expiration_ns(), Some(expected));
            assert_eq!(instrument.price_increment(), Price::from("0.005"));
        }
        assert_eq!(
            client.subscriptions.watchlist.read()[&condition_id].expiration_ns,
            expected
        );
    }

    #[rstest]
    #[case::batch(false)]
    #[case::single(true)]
    #[tokio::test]
    async fn metadata_writes_preserve_newer_gamma_revision(#[case] single: bool) {
        let mut client = make_client(PolymarketDataClientConfig::default());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let definitions = |revision: &str, end_date: &str| {
            let mut market = retained_market(0);
            market.raw = serde_json::json!({"updatedAt": revision}).to_string();
            market.end_date = Some(end_date.to_string());
            crate::http::gamma::parse_markets_with_transient(&[market], UnixNanos::default()).0
        };
        let publish = |instruments| {
            cache_and_publish_instruments(
                &client.subscriptions,
                &client.instrument_update_state,
                &client.data_sender,
                client.clock.get_time_ns(),
                Some(0),
                instruments,
            )
        };
        assert_eq!(
            publish(definitions("2026-01-02T00:00:00Z", "2099-12-01T00:00:00Z")),
            2
        );
        let cached = client.instruments.load();
        let stale = definitions("2026-01-01T00:00:00Z", "2099-01-01T00:00:00Z");
        let applied = if single {
            usize::from(apply_live_instrument(
                &client.subscriptions.closed_condition_ids,
                &mut client.instrument_update_state.lock(),
                &client.instruments,
                &client.subscriptions.token_meta,
                &client.subscriptions.watchlist,
                &stale[0],
                |_| panic!("Stale definition was published"),
            ))
        } else {
            publish(stale.clone())
        };
        assert_eq!(applied, 0);
        assert_eq!(client.instruments.load().as_ref(), cached.as_ref());
        assert_eq!(rx.len(), 2);

        if single {
            let instrument = &stale[0];
            let condition_id = extract_condition_id(&instrument.id()).unwrap();
            let expiration = cached
                .get(&instrument.id())
                .unwrap()
                .expiration_ns()
                .unwrap();
            client
                .subscriptions
                .active_instrument_status_subs
                .insert(instrument.id());
            crate::resolve::admit_data_resolution_instrument(
                &client.resolution_context(),
                instrument,
            )
            .await;
            assert_eq!(
                client.subscriptions.watchlist.read()[&condition_id].expiration_ns,
                expiration
            );
        }
    }

    #[rstest]
    #[case::missing(None, false, false, Some("old"))]
    #[case::same_parent(Some("event-123"), false, false, Some("old"))]
    #[case::different_parent(Some("event-456"), false, false, None)]
    #[case::conflicting_parents(Some("event-456"), false, true, None)]
    #[case::mixed_parents(Some("event-123"), false, true, None)]
    #[case::fresh_snapshot(Some("event-123"), true, false, Some("new"))]
    fn metadata_updates_preserve_event_context(
        #[case] parent_id: Option<&str>,
        #[case] full_snapshot: bool,
        #[case] conflicting_links: bool,
        #[case] expected_snapshot: Option<&str>,
        #[values(Some("parent-game"), None)] parent_game: Option<&str>,
        #[values("parent", "crypto", "disabled", "free")] fee_source: &str,
    ) {
        let mut initial_market = retained_market(0);
        initial_market.fee_type = None;
        initial_market.game_id = parent_game.is_none().then(|| "market-game".into());
        let parent = serde_json::from_value(serde_json::json!({
            "id": "event-123", "snapshot": "old", "gameId": parent_game,
            "markets": [initial_market],
        }))
        .unwrap();
        let mut markets = crate::http::gamma::flatten_event_markets(vec![parent]);
        crate::http::parse::enrich_market_fee_schedule(&mut markets[0]);
        let (cached, _) =
            crate::http::gamma::parse_markets_with_transient(&markets, UnixNanos::default());
        let old_snapshot = cached[0].info().unwrap().to_shared_map()["gamma_event"].clone();
        let client = make_client(PolymarketDataClientConfig::default());
        for instrument in cached {
            client.instruments.insert(instrument.id(), instrument);
        }
        let mut market = retained_market(0);
        market.end_date = Some("2098-01-01T00:00:00Z".into());
        market.fee_type = match fee_source {
            "crypto" => Some("crypto_fees".into()),
            "free" => Some("geopolitics".into()),
            _ => None,
        };
        market.fees_enabled = Some(fee_source != "disabled");

        if let Some(id) = parent_id {
            let event = serde_json::from_value(
                serde_json::json!({"id": id, "snapshot": "new", "markets": [market]}),
            )
            .unwrap();

            if full_snapshot {
                market.parent_event = Some(Arc::new(event));
            } else {
                let mut events = vec![event];

                if conflicting_links {
                    events.push(serde_json::from_str(r#"{"id":"event-789"}"#).unwrap());
                }
                market.events = Some(events);
            }
        }
        market.raw = serde_json::to_string(&market).unwrap();
        crate::http::parse::enrich_market_fee_schedule(&mut market);
        let (incoming, _) = crate::http::gamma::parse_markets_with_transient(
            std::slice::from_ref(&market),
            UnixNanos::default(),
        );
        let mut published = None;
        assert!(apply_live_instrument(
            &client.subscriptions.closed_condition_ids,
            &mut client.instrument_update_state.lock(),
            &client.instruments,
            &client.subscriptions.token_meta,
            &client.subscriptions.watchlist,
            &incoming[0],
            |instrument| published = Some(instrument.clone()),
        ));
        let InstrumentAny::BinaryOption(binary) = published.unwrap() else {
            unreachable!()
        };
        let expected_event_id = (!conflicting_links).then_some(parent_id.unwrap_or("event-123"));
        assert_eq!(binary.event_id, expected_event_id.map(Ustr::from));
        assert_eq!(
            binary.expiration_ns,
            "2098-01-01T00:00:00Z".parse::<UnixNanos>().unwrap()
        );
        let info = binary.info.unwrap();
        assert_eq!(info.get_str("gamma_market"), Some(market.raw.as_str()));
        assert_eq!(info.get_str("event_id"), expected_event_id);
        let schedule = info.get("fee_schedule").unwrap();
        let expected_rebate = match fee_source {
            "crypto" => "0.20",
            "parent" if expected_snapshot == Some("old") && parent_game.is_some() => "0.15",
            _ => "0",
        };
        assert_eq!(schedule["rebateRate"], expected_rebate);
        assert_eq!(schedule["rate"], "0.07");
        let snapshot = info
            .get_str("gamma_event")
            .map(|raw| serde_json::from_str::<serde_json::Value>(raw).unwrap());
        assert_eq!(
            snapshot
                .as_ref()
                .and_then(|value| value["snapshot"].as_str()),
            expected_snapshot
        );
        assert_eq!(
            info.get_str("game_id"),
            (expected_snapshot == Some("old"))
                .then_some(parent_game)
                .flatten()
        );

        if expected_snapshot == Some("old") {
            assert!(std::ptr::eq(
                info.get("gamma_event").unwrap(),
                old_snapshot.as_ref()
            ));
        }
    }

    #[rstest]
    #[tokio::test]
    async fn refresh_includes_sibling_owners_added_during_request(
        #[values(false, true)] closed: bool,
        #[values(false, true)] position: bool,
    ) {
        let market: GammaMarket = serde_json::from_value(past_end_open_market()).unwrap();
        let addr = market_closure_server(closed, false).await;
        let http = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();
        let mut client = make_client(PolymarketDataClientConfig::default());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let (mut cached, _) =
            crate::http::gamma::parse_markets_with_transient(&[market], UnixNanos::default());
        let expected_expiration = cached[0].expiration_ns().unwrap();
        let condition_id = extract_condition_id(&cached[0].id()).unwrap();

        for instrument in &mut cached {
            let InstrumentAny::BinaryOption(binary) = instrument else {
                unreachable!()
            };
            binary.expiration_ns = UnixNanos::from(1);
            client.instruments.insert(binary.id, instrument.clone());
        }
        client.subscriptions.instrument_subs.insert(cached[0].id());
        let refresh = refresh_instruments(
            http,
            InstrumentRefreshScope::Retained,
            &client.subscriptions,
            &client.instrument_update_state,
            &client.data_sender,
            client.clock,
        );
        tokio::pin!(refresh);
        assert!(futures_util::poll!(&mut refresh).is_pending());

        if position {
            crate::resolve::upsert_resolve_watch_entry_from_instrument(
                &client.subscriptions.watchlist,
                &cached[1],
                PositionId::from("P-1"),
            );
        } else {
            assert!(client.subscriptions.add_resolution_subscription_intent(
                cached[1].id(),
                super::super::subscriptions::SubscriptionKind::Status,
            ));
        }
        assert_eq!(refresh.await, 2);
        assert_eq!(rx.len(), 2);
        assert_eq!(
            client.subscriptions.watchlist.read()[&condition_id].expiration_ns,
            expected_expiration
        );
        assert_eq!(
            client.subscriptions.is_condition_closed(&condition_id),
            closed
        );
    }

    type RefreshGate = (
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Semaphore>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    );

    type RefreshState = (
        Arc<Mutex<Vec<Vec<String>>>>,
        Arc<Mutex<Option<&'static str>>>,
        bool,
        bool,
    );

    async fn retained_response(
        State((requests, closing_date, discovery_fails, discovery_closed)): State<RefreshState>,
        RawQuery(query): RawQuery,
    ) -> Response {
        let ids = requested_condition_ids(query.as_deref());
        requests.lock().push(ids.clone());
        if ids.is_empty() && discovery_fails {
            return StatusCode::BAD_REQUEST.into_response();
        }

        let closed_end_date = *closing_date.lock();
        if closed_end_date.is_some()
            && !ids.is_empty()
            && !query
                .as_deref()
                .is_some_and(|query| query.contains("closed=true"))
        {
            return Json(serde_json::json!({"markets": [], "next_cursor": null})).into_response();
        }

        let mut markets = if ids.is_empty() {
            vec![retained_market(0)]
        } else {
            ids.iter()
                .map(|id| retained_market(usize::from_str_radix(&id[2..], 16).unwrap()))
                .collect()
        };

        if let Some(end_date) = closed_end_date
            && (discovery_closed || !ids.is_empty())
        {
            for market in &mut markets {
                market.closed = Some(true);
                market.end_date = Some(end_date.to_string());
            }
        }
        let cursor = (!ids.is_empty()).then_some("unused");
        Json(serde_json::json!({"markets": markets, "next_cursor": cursor})).into_response()
    }

    fn retained_market(index: usize) -> GammaMarket {
        let mut market: GammaMarket =
            serde_json::from_str(include_str!("../../test_data/gamma_market_fee_crypto.json"))
                .unwrap();
        market.condition_id = format!("0x{index:064x}");
        market.clob_token_ids = format!("[\"{}\",\"{}\"]", index * 2 + 1, index * 2 + 2);
        market.market_slug = None;
        market.end_date = Some("2099-12-01T00:00:00Z".into());
        market
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
