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

use std::{sync::Arc, time::Duration};

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use nautilus_common::{
    live::sender::EventSender, messages::DataEvent, providers::InstrumentProvider,
};
use nautilus_core::{AtomicMap, DurationNanos, Params, UnixNanos, time::AtomicTime};
use nautilus_live::task::TaskSpawner;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{BinaryOption, Instrument, InstrumentAny},
};
use parking_lot::{Mutex, MutexGuard};
use rust_decimal::Decimal;
use serde_json::{Value, from_str, to_string, value::RawValue};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    PolymarketDataClient, runtime::is_instrument_expired_and_not_reported_open,
    subscriptions::DataSubscriptions,
};
use crate::{
    common::consts::GAMMA_CONDITION_IDS_BATCH_SIZE,
    config::PolymarketInstrumentProviderConfig,
    filters::{
        InstrumentFilter, binary_market_closed, is_expired, market_closed, set_market_closed,
    },
    http::{
        gamma::{
            PolymarketGammaHttpClient, parse_markets_with_event_metadata,
            parse_markets_with_transient,
        },
        models::{GammaEvent, GammaMarket},
        parse::{enrich_market_fee_schedule, is_market_closed},
        query::GetGammaMarketsParams,
    },
    providers::{extract_condition_id, fetch_configured_instruments, validated_condition_id},
    resolve::{ResolveContext, ResolveWatchEntry},
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
    pub(super) retired: bool,
}

impl InstrumentUpdateState {
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
    cache_instruments(instruments, token_meta, pending, |_| true);
}

fn cache_instruments(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    pending: &[InstrumentAny],
    update_token_meta: impl Fn(&InstrumentAny) -> bool,
) {
    if pending.is_empty() {
        return;
    }

    for instrument in pending
        .iter()
        .filter(|instrument| update_token_meta(instrument))
    {
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

pub(super) struct SelectedInstrument<'a> {
    pub(super) update_state: MutexGuard<'a, InstrumentUpdateState>,
    pub(super) instrument: InstrumentAny,
    pub(super) replace: bool,
}

pub(super) enum InstrumentSelectionError {
    Filtered,
    Unavailable,
}

pub(super) fn select_current_instrument<'a>(
    update_state: &'a Mutex<InstrumentUpdateState>,
    instruments: &AtomicMap<InstrumentId, InstrumentAny>,
    snapshot: &AHashMap<InstrumentId, InstrumentAny>,
    fetched: &InstrumentAny,
    accept: impl Fn(&InstrumentAny) -> bool,
    cancellation: &CancellationToken,
) -> Result<SelectedInstrument<'a>, InstrumentSelectionError> {
    let id = fetched.id();

    loop {
        if cancellation.is_cancelled() {
            return Err(InstrumentSelectionError::Unavailable);
        }

        let cached = Arc::clone(&instruments.load());
        let replace = !definition_superseded(snapshot, &cached, id);
        let instrument = if replace {
            Some(fetched)
        } else {
            cached.get(&id)
        };

        let Some(instrument) = instrument else {
            return Err(InstrumentSelectionError::Unavailable);
        };

        // Predicates can inspect the cache, so evaluate outside the publication guard
        let accepted = accept(instrument);
        let update_state = update_state.lock();
        if update_state.retired || cancellation.is_cancelled() {
            return Err(InstrumentSelectionError::Unavailable);
        }

        if !Arc::ptr_eq(&cached, &instruments.load()) {
            continue;
        }

        return if accepted {
            Ok(SelectedInstrument {
                update_state,
                instrument: instrument.clone(),
                replace,
            })
        } else {
            Err(InstrumentSelectionError::Filtered)
        };
    }
}

// The callback receives None when a newer open definition wins the race
pub(super) fn apply_live_instrument(
    closed_condition_ids: &Arc<Mutex<AHashSet<String>>>,
    update_state: &MutexGuard<'_, InstrumentUpdateState>,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    replace: bool,
    instrument: &InstrumentAny,
    apply: impl FnOnce(Option<&InstrumentAny>),
) -> bool {
    if !replace {
        let terminal = closed_condition_ids.lock();

        if !update_state.retired
            && !extract_condition_id(&instrument.id())
                .is_ok_and(|condition| terminal.contains(&condition))
            && market_closed(instrument) == Some(false)
        {
            apply(None);
        }

        return false;
    }

    // Keep the guard through the callback so cache writes and queued publication stay ordered
    apply_live_instrument_locked(
        closed_condition_ids,
        update_state,
        instruments,
        token_meta,
        instrument,
        |instrument| apply(Some(instrument)),
    )
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

    publish_cached_conditions_closed(instruments, data_sender, clock, |candidate| {
        candidate == condition_id
    })
}

fn publish_cached_conditions_closed(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    data_sender: &EventSender<DataEvent>,
    clock: &AtomicTime,
    includes: impl Fn(&str) -> bool,
) -> usize {
    let ts_init = clock.get_time_ns();
    let mut updated_ids = Vec::new();
    instruments.rcu(|map| {
        updated_ids.clear();

        for (instrument_id, instrument) in map.iter_mut() {
            if !extract_condition_id(instrument_id).is_ok_and(|candidate| includes(&candidate)) {
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

#[expect(
    clippy::too_many_arguments,
    reason = "Bootstrap serializes cache and publication"
)]
fn cache_and_publish_instruments(
    closed_condition_ids: &Arc<Mutex<AHashSet<String>>>,
    instrument_update_state: &Arc<Mutex<InstrumentUpdateState>>,
    instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    data_sender: &EventSender<DataEvent>,
    now_ns: UnixNanos,
    instruments: Vec<&InstrumentAny>,
    snapshot: Option<&AHashMap<InstrumentId, InstrumentAny>>,
) -> usize {
    let update_state = instrument_update_state.lock();
    if update_state.retired {
        return 0;
    }

    let mut pending = Vec::with_capacity(instruments.len());
    let cached = instruments_cache.load();

    for definition in instruments {
        let id = definition.id();
        let instrument =
            if snapshot.is_none_or(|snapshot| definition_superseded(snapshot, &cached, id)) {
                cached.get(&id).unwrap_or(definition)
            } else {
                definition
            };

        if is_instrument_expired_and_not_reported_open(instrument, now_ns) {
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

    let terminal_conditions = closed_condition_ids.lock();
    pending.retain(|instrument| {
        !extract_condition_id(&instrument.id())
            .is_ok_and(|condition_id| terminal_conditions.contains(&condition_id))
    });

    if pending.is_empty() {
        return 0;
    }

    cache_instruments_unchecked(instruments_cache, token_meta, &pending);
    let total = pending.len();

    // Keep the update guard through publication so live tick updates cannot overtake this batch
    publish_instrument_definitions(pending, data_sender);

    total
}

async fn refresh_instruments(
    http: &PolymarketGammaHttpClient,
    config: Option<&PolymarketInstrumentProviderConfig>,
    filters: &[Arc<dyn InstrumentFilter>],
    ctx: &ResolveContext,
    update_state: &Mutex<InstrumentUpdateState>,
    max_wait: DurationNanos,
) -> usize {
    let mut discovery_events = AHashMap::new();

    let discovery = if config.is_some() || !filters.is_empty() {
        let snapshot = Arc::clone(&ctx.instruments.load());
        let config = config.cloned().unwrap_or_default();
        match fetch_configured_instruments(http, &config, filters).await {
            Ok(instruments) => {
                apply_instrument_refresh(
                    ctx,
                    update_state,
                    &snapshot,
                    instruments,
                    false,
                    max_wait,
                    &mut discovery_events,
                )
                .await
            }
            Err(e) => {
                log::warn!("Failed to refresh Polymarket discovery instruments: {e}");
                AHashSet::new()
            }
        }
    } else {
        AHashSet::new()
    };

    let tracked = tracked_conditions(ctx);
    if tracked.is_empty() {
        return discovery.len();
    }

    let (mut missing, cached_closed) = tracked_refresh_candidates(ctx, &tracked, &discovery);
    missing.sort_unstable();

    let mut total = discovery.len();
    let http = http.for_tracked_refresh();

    for chunk in missing.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
        let snapshot = Arc::clone(&ctx.instruments.load());

        let known_closed = {
            let terminal = ctx.subscriptions.lock_closed_conditions();
            chunk
                .iter()
                .filter(|id| terminal.contains(*id) || cached_closed.contains(*id))
                .cloned()
                .collect()
        };

        let instruments =
            match fetch_tracked_instruments(&http, chunk, &known_closed, ctx.clock.get_time_ns())
                .await
            {
                Ok(instruments) => instruments,
                Err(e) => {
                    log::warn!("Failed to refresh tracked Polymarket instruments: {e}");
                    continue;
                }
            };

        total += apply_instrument_refresh(
            ctx,
            update_state,
            &snapshot,
            instruments,
            true,
            max_wait,
            &mut discovery_events,
        )
        .await
        .len();
    }

    total
}

fn reconcile_book_availability(
    subscriptions: &DataSubscriptions,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let mut ids = subscriptions
        .active_delta_subs
        .iter()
        .map(|id| *id)
        .filter(|id| !subscriptions.book_sync.is_observed(*id))
        .collect::<Vec<_>>();
    ids.sort_unstable();

    let terminal = subscriptions.lock_closed_conditions();

    for id in ids {
        subscriptions.reconcile_book_availability(id, snapshot_timeout, tasks, &terminal);
    }
}

fn tracked_refresh_candidates(
    ctx: &ResolveContext,
    tracked: &AHashSet<String>,
    discovery: &AHashSet<InstrumentId>,
) -> (Vec<String>, AHashSet<String>) {
    let mut missing = AHashSet::new();
    let mut closed = AHashSet::new();

    for (id, instrument) in ctx.instruments.load().iter() {
        let Ok(condition) = extract_condition_id(id) else {
            continue;
        };

        if !tracked.contains(&condition) {
            continue;
        }

        if market_closed(instrument) == Some(true) {
            closed.insert(condition);
        }

        if !discovery.contains(id)
            && let Ok(condition) = validated_condition_id(id)
        {
            missing.insert(condition);
        }
    }

    (missing.into_iter().collect(), closed)
}

fn tracked_conditions(ctx: &ResolveContext) -> AHashSet<String> {
    let mut conditions = ctx
        .watchlist
        .read()
        .keys()
        .cloned()
        .collect::<AHashSet<_>>();
    let cached = ctx.instruments.load();
    for intents in ctx.subscriptions.intents() {
        conditions.extend(
            intents
                .iter()
                .filter(|id| cached.contains_key(id.key()))
                .filter_map(|id| extract_condition_id(id.key()).ok()),
        );
    }

    conditions
}

async fn fetch_tracked_instruments(
    http: &PolymarketGammaHttpClient,
    condition_ids: &[String],
    known_closed: &AHashSet<String>,
    now: UnixNanos,
) -> anyhow::Result<Vec<InstrumentAny>> {
    let mut markets = http
        .request_markets_by_params(GetGammaMarketsParams {
            condition_ids: Some(condition_ids.to_vec()),
            ..Default::default()
        })
        .await?;

    markets.retain(|market| {
        condition_ids.contains(&market.condition_id)
            && (!known_closed.contains(&market.condition_id) || is_market_closed(market))
    });

    let returned = markets
        .iter()
        .map(|market| &market.condition_id)
        .collect::<AHashSet<_>>();
    let missing = condition_ids
        .iter()
        .filter(|id| !returned.contains(id))
        .cloned()
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        match query_positive_closed_markets(http, &missing).await {
            Ok(closed) => markets.extend(closed),
            Err(e) => log::warn!("Failed to refresh closed Polymarket instruments: {e}"),
        }
    }

    for market in &mut markets {
        enrich_market_fee_schedule(market);
    }

    Ok(parse_markets_with_transient(&markets, now).0)
}

async fn apply_instrument_refresh(
    ctx: &ResolveContext,
    update_state: &Mutex<InstrumentUpdateState>,
    snapshot: &AHashMap<InstrumentId, InstrumentAny>,
    instruments: Vec<InstrumentAny>,
    tracked_only: bool,
    max_wait: DurationNanos,
    discovery_events: &mut AHashMap<InstrumentId, DeferredDiscoveryEvent>,
) -> AHashSet<InstrumentId> {
    let conditions = prepare_refresh_batches(
        snapshot,
        instruments,
        discovery_events,
        ctx.clock.get_time_ns(),
    );
    let (published, resumed) = commit_instrument_refresh(
        ctx,
        update_state,
        snapshot,
        conditions,
        tracked_only,
        max_wait,
        discovery_events,
    );

    for (id, token) in resumed {
        ctx.sync_ws_subscription(id, token).await;
    }

    published
}

struct DeferredDiscoveryEvent {
    previous: InstrumentAny,
    info: Params,
}

fn prepare_refresh_batches(
    snapshot: &AHashMap<InstrumentId, InstrumentAny>,
    instruments: Vec<InstrumentAny>,
    discovery_events: &AHashMap<InstrumentId, DeferredDiscoveryEvent>,
    now: UnixNanos,
) -> AHashMap<String, Vec<InstrumentAny>> {
    let mut conditions = AHashMap::<String, Vec<InstrumentAny>>::new();
    let mut events = AHashMap::new();

    for mut instrument in instruments {
        let id = instrument.id();

        let Ok(condition) = extract_condition_id(&id) else {
            continue;
        };

        if let Some(current) = snapshot.get(&id)
            && let Some(info) = discovery_events
                .get(&id)
                .filter(|event| !definition_changed(&event.previous, current))
                .map(|event| &event.info)
                .or_else(|| current.info())
        {
            restore_refresh_event(&mut instrument, info, &mut events, now);
        }

        conditions.entry(condition).or_default().push(instrument);
    }

    conditions
}

fn commit_instrument_refresh(
    ctx: &ResolveContext,
    update_state: &Mutex<InstrumentUpdateState>,
    snapshot: &AHashMap<InstrumentId, InstrumentAny>,
    conditions: AHashMap<String, Vec<InstrumentAny>>,
    tracked_only: bool,
    max_wait: DurationNanos,
    discovery_events: &mut AHashMap<InstrumentId, DeferredDiscoveryEvent>,
) -> (AHashSet<InstrumentId>, Vec<(InstrumentId, String)>) {
    let update_state = update_state.lock();
    let _owners = ctx.apply_mutex.lock();
    if update_state.retired || ctx.cancellation_token.is_cancelled() {
        return (AHashSet::new(), Vec::new());
    }

    let terminal = ctx.subscriptions.lock_closed_conditions();
    let mut watches = ctx.watchlist.write();
    let cached = ctx.instruments.load();

    let omitted = if tracked_only {
        AHashMap::new()
    } else {
        omitted_discovery_outcomes(&conditions, &cached)
    };

    let now = ctx.clock.get_time_ns();
    let mut pending = Vec::new();
    let mut resumed = Vec::new();

    for (condition, instruments) in conditions {
        let watch = watches.get_mut(&condition);
        let siblings = omitted.get(&condition);

        let tracked = watch.is_some()
            || instruments
                .iter()
                .map(Instrument::id)
                .chain(siblings.into_iter().flatten().copied())
                .any(|id| cached.contains_key(&id) && ctx.subscriptions.has_intent(id));

        let defer = tracked && siblings.is_some();
        // Admit new discovery rows now and refresh cached outcomes together in the fallback
        if defer && defer_discovery_update(&instruments, snapshot, &cached, discovery_events) {
            continue;
        }

        let updates = prepare_condition_refresh(
            &update_state,
            snapshot,
            &cached,
            instruments,
            tracked,
            tracked_only,
            terminal.contains(&condition),
            siblings.map_or(&[], Vec::as_slice),
            now,
        );

        if let Some(watch) = watch.filter(|_| !defer) {
            refresh_watch_deadline(watch, &updates, now, max_wait, &mut resumed);
        }

        pending.extend(updates);
    }

    // A closed watch keeps its definition after retirement has removed its live token routing
    cache_instruments(
        &ctx.instruments,
        &ctx.subscriptions.token_meta,
        &pending,
        |instrument| {
            !extract_condition_id(&instrument.id())
                .is_ok_and(|condition| terminal.contains(&condition))
        },
    );

    let published = pending.iter().map(Instrument::id).collect();
    publish_instrument_definitions(pending, &ctx.data_sender);
    (published, resumed)
}

fn defer_discovery_update(
    instruments: &[InstrumentAny],
    snapshot: &AHashMap<InstrumentId, InstrumentAny>,
    cached: &AHashMap<InstrumentId, InstrumentAny>,
    discovery_events: &mut AHashMap<InstrumentId, DeferredDiscoveryEvent>,
) -> bool {
    let [instrument] = instruments else {
        return false;
    };

    let id = instrument.id();

    let Some(previous) = cached.get(&id) else {
        return false;
    };

    if let Some(info) = instrument
        .info()
        .filter(|info| info.contains_key("gamma_event"))
        && !definition_superseded(snapshot, cached, id)
    {
        discovery_events.insert(
            id,
            DeferredDiscoveryEvent {
                previous: previous.clone(),
                info: info.clone(),
            },
        );
    }

    true
}

fn omitted_discovery_outcomes(
    conditions: &AHashMap<String, Vec<InstrumentAny>>,
    cached: &AHashMap<InstrumentId, InstrumentAny>,
) -> AHashMap<String, Vec<InstrumentId>> {
    let mut omitted = AHashMap::<String, Vec<InstrumentId>>::new();
    if !conditions.values().any(|outcomes| outcomes.len() == 1) {
        return omitted;
    }

    for id in cached.keys() {
        if let Ok(condition) = extract_condition_id(id)
            && let Some([outcome]) = conditions.get(&condition).map(Vec::as_slice)
            && outcome.id() != *id
        {
            omitted.entry(condition).or_default().push(*id);
        }
    }

    omitted
}

fn publish_instrument_definitions(
    instruments: Vec<InstrumentAny>,
    sender: &EventSender<DataEvent>,
) {
    for instrument in instruments {
        let id = instrument.id();
        if let Err(e) = sender.send(DataEvent::Data(instrument.into())) {
            log::warn!("Failed to publish instrument {id}: {e}");
        }
    }
}

fn refresh_watch_deadline(
    watch: &mut ResolveWatchEntry,
    updates: &[InstrumentAny],
    now: UnixNanos,
    max_wait: DurationNanos,
    resumed: &mut Vec<(InstrumentId, String)>,
) {
    let Some(expiry) = updates
        .iter()
        .filter_map(Instrument::expiration_ns)
        .filter(|expiry| expiry.as_u64() != 0)
        .max()
    else {
        return;
    };

    if watch.paused && now < expiry.saturating_add(max_wait) {
        watch.paused = false;
        watch.clob_retry = None;
        resumed.extend(
            watch
                .tracked
                .values()
                .filter(|tracked| tracked.has_data_subscription)
                .map(|tracked| (tracked.instrument_id, tracked.token_id.clone())),
        );
    }

    watch.expiration_ns = expiry;
}

#[expect(
    clippy::too_many_arguments,
    reason = "Condition refresh uses the current cache and request snapshot"
)]
fn prepare_condition_refresh(
    state: &InstrumentUpdateState,
    snapshot: &AHashMap<InstrumentId, InstrumentAny>,
    cached: &AHashMap<InstrumentId, InstrumentAny>,
    instruments: Vec<InstrumentAny>,
    tracked: bool,
    tracked_only: bool,
    terminal: bool,
    omitted: &[InstrumentId],
    now: UnixNanos,
) -> Vec<InstrumentAny> {
    if (tracked_only || terminal) && !tracked {
        return Vec::new();
    }

    if instruments
        .iter()
        .any(|instrument| definition_superseded(snapshot, cached, instrument.id()))
    {
        return Vec::new();
    }

    let known_closed = terminal
        || instruments
            .iter()
            .map(Instrument::id)
            .chain(omitted.iter().copied())
            .any(|id| cached.get(&id).and_then(market_closed) == Some(true));

    if known_closed
        && instruments
            .iter()
            .any(|instrument| market_closed(instrument) != Some(true))
    {
        return Vec::new();
    }

    instruments
        .into_iter()
        .filter_map(|instrument| {
            let current = cached.get(&instrument.id());
            if (tracked_only || terminal) && current.is_none() {
                return None;
            }

            let close_recorded = terminal || current.and_then(market_closed) == Some(true);
            prepare_instrument_refresh(state, instrument, current, tracked, close_recorded, now)
        })
        .collect()
}

fn prepare_instrument_refresh(
    state: &InstrumentUpdateState,
    mut instrument: InstrumentAny,
    current: Option<&InstrumentAny>,
    tracked: bool,
    was_closed: bool,
    now: UnixNanos,
) -> Option<InstrumentAny> {
    if tracked && let InstrumentAny::BinaryOption(binary) = &mut instrument {
        update_tracked_lifecycle(binary, current, was_closed, now);
    }

    if (!was_closed || !tracked) && is_instrument_expired_and_not_reported_open(&instrument, now) {
        return None;
    }

    state
        .compose_instrument(&instrument)
        .inspect_err(|e| {
            log::warn!(
                "Failed to apply live tick to instrument {}: {e}",
                instrument.id()
            );
        })
        .ok()
}

fn update_tracked_lifecycle(
    binary: &mut BinaryOption,
    current: Option<&InstrumentAny>,
    was_closed: bool,
    now: UnixNanos,
) {
    let defer_closure = !was_closed && binary_market_closed(binary) == Some(true);
    if defer_closure {
        // The expiry probe or resolution handler owns the trading state transition
        set_market_closed(binary, false);
    }

    if let Some(current) = current
        && (binary.expiration_ns.as_u64() == 0
            || (defer_closure && binary.expiration_ns > now && is_expired(current, now)))
    {
        preserve_expiry(binary, current);
    }
}

pub(super) fn preserve_expiry(binary: &mut BinaryOption, current: &InstrumentAny) {
    if let Some(expiry) = current.expiration_ns() {
        binary.expiration_ns = expiry;
    }

    let mut info = binary.info.as_ref().map_or_default(Params::to_shared_map);
    if let Some(date) = current.info().and_then(|info| info.get("end_date")) {
        info.insert("end_date".into(), Arc::new(date.clone()));
    } else {
        info.shift_remove("end_date");
    }

    binary.info = Some(Params::from_shared(info));
}

struct RefreshEvent {
    parsed: Arc<GammaEvent>,
    raw: Arc<Value>,
}

fn restore_refresh_event<'a>(
    instrument: &mut InstrumentAny,
    source: &'a Params,
    events: &mut AHashMap<&'a str, Option<RefreshEvent>>,
    now: UnixNanos,
) {
    let Some(info) = instrument.info() else {
        return;
    };

    if info.get("gamma_event").is_some() {
        return;
    }

    let Some(event) = source.get_str("gamma_event") else {
        return;
    };

    let Some(raw_market) = info.get_str("gamma_market") else {
        return;
    };

    let event = events.entry(event).or_insert_with(|| {
        let mut fields: AHashMap<String, &RawValue> = from_str(event).ok()?;
        // The complete JSON stays shared while only the parent metadata is decoded
        fields.remove("markets");
        let parsed = from_str::<GammaEvent>(&to_string(&fields).ok()?).ok()?;
        let raw = source.to_shared_map().swap_remove("gamma_event")?;
        Some(RefreshEvent {
            parsed: Arc::new(parsed),
            raw,
        })
    });

    let (
        Ok(mut market),
        Some(RefreshEvent {
            parsed: event,
            raw: raw_event,
        }),
    ) = (from_str::<GammaMarket>(raw_market), event)
    else {
        return;
    };

    if market
        .events
        .as_ref()
        .is_some_and(|events| events.iter().any(|item| item.id != event.id))
    {
        return;
    }

    market.game_id = market.game_id.or_else(|| event.game_id.clone());
    market.parent_event = Some(event.clone());
    enrich_market_fee_schedule(&mut market);
    let mut metadata = AHashMap::from([(Arc::as_ptr(event), Arc::clone(raw_event))]);
    if let Some(updated) = parse_markets_with_event_metadata(&[market], now, &mut metadata)
        .0
        .into_iter()
        .find(|candidate| candidate.id() == instrument.id())
    {
        *instrument = updated;
    }
}

pub(super) fn definition_superseded(
    snapshot: &AHashMap<InstrumentId, InstrumentAny>,
    cached: &AHashMap<InstrumentId, InstrumentAny>,
    id: InstrumentId,
) -> bool {
    let Some(current) = cached.get(&id) else {
        return snapshot.contains_key(&id);
    };

    let Some(previous) = snapshot.get(&id) else {
        return true;
    };

    definition_changed(previous, current)
}

fn definition_changed(previous: &InstrumentAny, current: &InstrumentAny) -> bool {
    current.expiration_ns() != previous.expiration_ns()
        || market_closed(current) != market_closed(previous)
        || ["gamma_market", "gamma_event", "fee_schedule"]
            .into_iter()
            .any(|key| {
                current.info().and_then(|info| info.get(key))
                    != previous.info().and_then(|info| info.get(key))
            })
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
    let mut condition_ids = AHashSet::new();

    for (id, instrument) in cache.load().iter() {
        if is_expired(instrument, now_ns)
            && market_closed(instrument) == Some(false)
            && let Ok(condition_id) = extract_condition_id(id)
        {
            condition_ids.insert(condition_id);
        }
    }

    let condition_ids = condition_ids.into_iter().collect::<Vec<_>>();
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

    // Serialize terminal application with reset. If reset wins the boundary, this old generation
    // must not mutate or publish from the cache it captured before the request.
    for condition_id in &closed_ids {
        if !subscriptions.mark_closed(condition_id, cancellation).await {
            return Ok(0);
        }
    }

    let _terminal = subscriptions.lock_closed_conditions();

    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        return Ok(0);
    }

    let updated = if closed_ids.is_empty() {
        0
    } else {
        publish_cached_conditions_closed(cache, sender, clock, |id| closed_ids.contains(id))
    };

    if failed_chunks > 0 {
        anyhow::bail!(
            "Failed to probe market closure for {failed_chunks} of {total_chunks} condition ID chunk(s)"
        );
    }

    Ok(updated)
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
        let snapshot =
            (!self.provider.store().is_initialized()).then(|| Arc::clone(&self.instruments.load()));
        self.provider.initialize(false).await?;

        let total = cache_and_publish_instruments(
            &self.subscriptions.closed_condition_ids,
            &self.instrument_update_state,
            &self.instruments,
            &self.subscriptions.token_meta,
            &self.data_sender,
            self.clock.get_time_ns(),
            self.provider.store().list_all(),
            snapshot.as_deref(),
        );

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
        let cancellation = self.cancellation_token.clone();
        let http = self.provider.http_client().clone();
        let filters = self.provider.filters();
        let config = self.config.instrument_config.clone();
        let ctx = self.resolution_context();
        let update_state = self.instrument_update_state.clone();
        let snapshot_timeout = Duration::from_secs(self.config.book_snapshot_timeout_secs);
        let tasks = self.tasks.spawner()?;
        let max_wait = self
            .config
            .resolve_poll_max_wait_secs
            .max(self.config.resolve_poll_grace_secs);
        let max_wait = DurationNanos::try_from_secs(max_wait).unwrap_or(DurationNanos::MAX);

        let future = async move {
            log::debug!("Polymarket instrument refresh task started");

            loop {
                tokio::select! {
                    () = tokio::time::sleep(interval) => {}
                    () = cancellation.cancelled() => break,
                }

                let total = tokio::select! {
                    biased;
                    () = cancellation.cancelled() => break,
                    total = refresh_instruments(&http, config.as_ref(), &filters, &ctx, &update_state, max_wait) => total,
                };

                if total > 0 {
                    reconcile_book_availability(&ctx.subscriptions, snapshot_timeout, &tasks);
                    log::debug!("Refreshed {total} Polymarket instruments into the live cache");
                }
            }

            log::debug!("Polymarket instrument refresh task ended");
        };

        self.tasks.spawn(future).map_err(|e| {
            anyhow::anyhow!("failed to register Polymarket instrument refresh: {e}")
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        iter::from_fn,
        net::SocketAddr,
        ptr::eq as ptr_eq,
        slice::from_ref,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router,
        extract::{RawQuery, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::get,
        serve,
    };
    use nautilus_common::live::runner::replace_data_event_sender;
    use nautilus_core::UnixNanos;
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
    use serde_json::{Number, from_value, to_value};
    use tokio::net::TcpListener;

    use super::*;
    use crate::{
        common::consts::WS_DEFAULT_SUBSCRIPTIONS,
        config::PolymarketDataClientConfig,
        data::subscriptions::SubscriptionKind,
        filters::{PredicateFilter, TagFilter},
        http::{
            clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
            gamma::PolymarketGammaHttpClient,
        },
        resolve::{
            pause_resolve_watch_entries, upsert_data_resolve_watch_entry_from_instrument,
            upsert_resolve_watch_entry_from_instrument,
        },
        websocket::pool::PolymarketMarketConnectionPool,
    };

    impl InstrumentUpdateState {
        pub(in crate::data) fn contains_live_tick(&self, token_id: &Ustr) -> bool {
            self.live_ticks.contains_key(token_id)
        }
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
    fn refresh_task_spawns_for_future_owners_without_discovery_scope() {
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
    #[case::replay(true, "0.001", 3)]
    #[case::fresh(false, "0.1", 1)]
    fn bootstrap_composes_definitions_and_preserves_live_ticks(
        #[case] replay: bool,
        #[case] size_increment: &str,
        #[case] size_precision: u8,
    ) {
        let instruments = Arc::new(AtomicMap::new());
        let instrument_update_state = Arc::new(Mutex::new(InstrumentUpdateState::default()));
        let token_meta = Arc::new(DashMap::new());
        let existing =
            stub_instrument("existing-token", Price::from("0.01"), Quantity::from("0.1"));
        let provider = stub_instrument("updated-token", Price::from("0.01"), Quantity::from("0.1"));
        let refreshed = stub_instrument(
            "updated-token",
            Price::from("0.01"),
            Quantity::from("0.001"),
        );
        cache_instrument_unchecked(&instruments, &token_meta, &existing);
        cache_instrument_unchecked(&instruments, &token_meta, &refreshed);
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
        let snapshot = Arc::clone(&instruments.load());

        let total = cache_and_publish_instruments(
            &Arc::new(Mutex::new(AHashSet::new())),
            &instrument_update_state,
            &instruments,
            &token_meta,
            &tx.into(),
            UnixNanos::default(),
            vec![&provider, &added, &invalid],
            (!replay).then_some(snapshot.as_ref()),
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
        assert_eq!(updated.size_increment(), Quantity::from(size_increment));
        assert_eq!(updated.ts_event(), UnixNanos::from(42));
        assert_eq!(meta.instrument_id, refreshed.id());
        assert_eq!(meta.price_precision, 4);
        assert_eq!(meta.size_precision, size_precision);
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
            vec![&instrument],
            None,
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
            vec![&instrument],
            None,
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
            let total = refresh_instruments(
                &http,
                Some(&config),
                &[],
                &client.resolution_context(),
                &client.instrument_update_state,
                DurationNanos::MAX,
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

            assert!(is_expired(&instrument, client.clock.get_time_ns()));
            assert_eq!(market_closed(&instrument), Some(false));
            assert_eq!(
                client.instruments.get_cloned(&instrument.id()),
                Some(*instrument)
            );
        }
    }

    #[tokio::test]
    async fn tracked_metadata_refresh_resumes_an_unavailable_book() {
        use crate::websocket::{
            handler::{CycleMarketOutcome, HandlerCommand},
            pool::PolymarketMarketPoolHandle,
        };

        let definition = |enabled: bool| {
            let mut market = past_end_open_market();
            market["enableOrderBook"] = enabled.into();
            let market = from_value::<GammaMarket>(market).unwrap();
            parse_markets_with_transient(&[market], UnixNanos::default())
                .0
                .remove(0)
        };
        let instrument = definition(false);
        let id = instrument.id();
        let token = instrument.raw_symbol().as_str().to_string();

        let mut client = make_client(PolymarketDataClientConfig::default());
        let (tx, mut commands) = tokio::sync::mpsc::unbounded_channel();
        client.subscriptions.ws_client =
            PolymarketMarketPoolHandle::test_single_shard(tx, &[&token]);
        let ctx = client.resolution_context();
        let subscriptions = &ctx.subscriptions;
        cache_instrument_unchecked(&ctx.instruments, &subscriptions.token_meta, &instrument);
        subscriptions
            .ws_open_tokens
            .insert(Ustr::from(token.as_str()));
        assert!(subscriptions.add_delta_subscription_intent(id, true));
        assert!(subscriptions.book_sync.is_unavailable(id));

        let snapshot = Arc::clone(&ctx.instruments.load());
        let refreshed = apply_instrument_refresh(
            &ctx,
            &client.instrument_update_state,
            &snapshot,
            vec![definition(true)],
            true,
            DurationNanos::MAX,
            &mut AHashMap::new(),
        )
        .await;
        assert!(refreshed.contains(&id));

        let tasks = client.tasks.spawner().unwrap();
        reconcile_book_availability(subscriptions, Duration::from_secs(2), &tasks);

        let command = tokio::time::timeout(Duration::from_secs(2), commands.recv())
            .await
            .unwrap()
            .unwrap();
        let HandlerCommand::CycleMarketSubscription {
            asset_ids,
            responder,
            gate,
            ..
        } = command
        else {
            panic!("expected initial snapshot request, was {command:?}");
        };
        assert_eq!(asset_ids, [token]);
        gate.open();
        responder.send(CycleMarketOutcome::Completed).unwrap();
        assert!(!subscriptions.book_sync.is_unavailable(id));
    }

    #[rstest]
    #[case::open(false, false, false, true)]
    #[case::closure_due(true, false, false, true)]
    #[case::awaiting_settlement(true, true, false, true)]
    #[case::failed_discovery(false, false, true, true)]
    #[case::missing_expiry(false, false, false, false)]
    #[tokio::test]
    async fn tracked_markets_refresh_metadata_outside_discovery(
        #[case] gamma_closed: bool,
        #[case] already_closed: bool,
        #[case] discovery_fails: bool,
        #[case] has_expiry: bool,
    ) {
        let mut client = make_client(PolymarketDataClientConfig::default());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let ctx = client.resolution_context();
        let original_date = "2025-01-01T00:00:00Z";
        let mut previous = tracked_market(original_date, "0.03", already_closed);
        let parent =
            from_str::<GammaEvent>(r#"{"id":"123","gameId":"456","category":"Sports"}"#).unwrap();
        previous.parent_event = Some(Arc::new(parent));
        let initial = parse_markets_with_transient(&[previous], UnixNanos::default()).0;
        cache_instruments_unchecked(&ctx.instruments, &ctx.subscriptions.token_meta, &initial);
        let snapshot = Arc::clone(&ctx.instruments.load());
        let condition = extract_condition_id(&initial[0].id()).unwrap();

        if already_closed {
            ctx.subscriptions
                .lock_closed_conditions()
                .insert(condition.clone());
            ctx.subscriptions.token_meta.clear();
        }

        let position_id = PositionId::from("P-1");
        upsert_resolve_watch_entry_from_instrument(&ctx.watchlist, &initial[0], position_id);
        let max_wait = DurationNanos::try_from_secs(3_600).unwrap();
        pause_resolve_watch_entries(
            &ctx.watchlist,
            from_ref(&condition),
            client.clock.get_time_ns(),
            max_wait,
        );
        assert!(ctx.watchlist.read()[&condition].paused);

        client.instrument_update_state.lock().record_live_tick(
            initial[0].raw_symbol().inner(),
            "0.005".parse().unwrap(),
            UnixNanos::from(25),
        );
        let changed_date = "2099-12-01T00:00:00Z";
        let market = tracked_market(changed_date, "0.07", gamma_closed);
        let mut market = from_str::<Value>(&market.raw).unwrap();
        if !has_expiry {
            market.as_object_mut().unwrap().remove("endDate");
        }

        let lookups = Arc::new(AtomicUsize::new(0));

        let app = Router::new().route(
            "/markets/keyset",
            get({
                let lookups = lookups.clone();
                move |RawQuery(query): RawQuery| {
                    let market = market.clone();
                    let lookups = lookups.clone();

                    async move {
                        let query = query.unwrap_or_default();
                        if !query.contains("condition_ids=") {
                            if discovery_fails {
                                return StatusCode::BAD_REQUEST.into_response();
                            }

                            assert!(query.contains("end_date_max="));
                            return Json(serde_json::json!({"markets": []})).into_response();
                        }

                        lookups.fetch_add(1, Ordering::SeqCst);

                        let markets =
                            match (query.contains("closed=true"), gamma_closed, already_closed) {
                                (false, true, true) => {
                                    let mut stale = market;
                                    stale["closed"] = serde_json::json!(false);
                                    stale["endDate"] = serde_json::json!(original_date);
                                    vec![stale]
                                }
                                (closed, expected, _) if closed == expected => vec![market],
                                _ => Vec::new(),
                            };

                        Json(serde_json::json!({"markets": markets})).into_response()
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { serve(listener, app).await.unwrap() });
        let http = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();

        let config = PolymarketInstrumentProviderConfig {
            filters: Some([("end_date_max".into(), "2099-06-01T00:00:00Z".into())].into()),
            ..Default::default()
        };

        let refresh = || {
            refresh_instruments(
                &http,
                Some(&config),
                &[],
                &ctx,
                &client.instrument_update_state,
                max_wait,
            )
        };

        assert_eq!(refresh().await, 2);
        assert_eq!(
            lookups.load(Ordering::SeqCst),
            1 + usize::from(gamma_closed)
        );
        assert_eq!(
            ctx.subscriptions.is_condition_closed(&condition),
            already_closed
        );
        assert_eq!(
            ctx.subscriptions.token_meta.len(),
            if already_closed { 0 } else { 2 }
        );
        let preserve_expiry = !has_expiry || (gamma_closed && !already_closed);
        let expected_date = if preserve_expiry {
            original_date
        } else {
            changed_date
        };

        let expiration = expected_date.parse::<UnixNanos>().unwrap();

        let published = from_fn(|| rx.try_recv().ok())
            .map(|event| {
                let DataEvent::Data(Data::Instrument(instrument)) = event else {
                    panic!("Expected instrument event, was {event:?}");
                };

                (instrument.id(), instrument)
            })
            .collect::<AHashMap<_, _>>();

        assert_eq!(published.len(), 2);

        for (index, previous) in initial.iter().enumerate() {
            let instrument = &published[&previous.id()];
            let info = instrument.info().unwrap();
            assert_eq!(instrument.expiration_ns(), Some(expiration));
            assert_eq!(info.get_str("end_date"), Some(expected_date));
            assert_eq!(info["fee_schedule"]["rate"], "0.07");
            assert_eq!(info.get_str("event_id"), Some("123"));
            assert_eq!(info.get_str("game_id"), Some("456"));
            assert_eq!(
                info.get_str("gamma_event"),
                previous.info().unwrap().get_str("gamma_event")
            );
            assert_eq!(market_closed(instrument), Some(already_closed));
            let tick = ["0.005", "0.01"][index];
            assert_eq!(instrument.price_increment(), Price::from(tick));
            let cached = ctx.instruments.get_cloned(&instrument.id()).unwrap();
            assert_eq!(
                to_value(cached).unwrap(),
                to_value(instrument.as_ref()).unwrap()
            );
        }

        {
            let watches = ctx.watchlist.read();
            let watch = &watches[&condition];
            assert_eq!(watch.expiration_ns, expiration);
            assert_eq!(watch.paused, preserve_expiry);
            assert_eq!(
                watch.tracked[initial[0].raw_symbol().as_str()].open_position_ids,
                AHashSet::from([position_id])
            );
        }

        assert!(
            apply_instrument_refresh(
                &ctx,
                &client.instrument_update_state,
                &snapshot,
                initial,
                true,
                max_wait,
                &mut AHashMap::new(),
            )
            .await
            .is_empty()
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(ctx.watchlist.read()[&condition].expiration_ns, expiration);

        ctx.watchlist.write().clear();
        assert_eq!(refresh().await, 0);
        assert_eq!(
            lookups.load(Ordering::SeqCst),
            1 + usize::from(gamma_closed)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn closed_refresh_applies_corrected_expiry_that_is_still_due(
        #[values(false, true)] deadline_already_admitted: bool,
    ) {
        let client = make_client(PolymarketDataClientConfig::default());
        let ctx = client.resolution_context();
        let now = ctx.clock.get_time_ns();
        let max_wait = DurationNanos::try_from_secs(3_600).unwrap();
        let due = now.saturating_sub(DurationNanos::try_from_secs(60).unwrap());
        let original = tracked_market("2025-01-01T00:00:00Z", "0.03", false);
        let correction = tracked_market(&due.to_rfc3339(), "0.07", true);
        let initial = parse_markets_with_transient(&[original], now).0;
        let updated = parse_markets_with_transient(&[correction], now).0;
        cache_instruments_unchecked(&ctx.instruments, &ctx.subscriptions.token_meta, &initial);
        upsert_resolve_watch_entry_from_instrument(
            &ctx.watchlist,
            &initial[0],
            PositionId::from("P-1"),
        );
        let condition = extract_condition_id(&initial[0].id()).unwrap();
        pause_resolve_watch_entries(&ctx.watchlist, from_ref(&condition), now, max_wait);

        if deadline_already_admitted {
            assert!(upsert_data_resolve_watch_entry_from_instrument(
                &ctx.watchlist,
                &updated[0],
            ));
            assert_eq!(ctx.watchlist.read()[&condition].expiration_ns, due);
        }

        assert!(ctx.watchlist.read()[&condition].paused);

        let snapshot = Arc::clone(&ctx.instruments.load());
        let published = apply_instrument_refresh(
            &ctx,
            &client.instrument_update_state,
            &snapshot,
            updated,
            true,
            max_wait,
            &mut AHashMap::new(),
        )
        .await;

        assert_eq!(published, initial.iter().map(Instrument::id).collect());

        for instrument in ctx.instruments.load().values() {
            assert_eq!(instrument.expiration_ns(), Some(due));
            assert_eq!(market_closed(instrument), Some(false));
        }

        let watches = ctx.watchlist.read();
        assert_eq!(watches[&condition].expiration_ns, due);
        assert!(!watches[&condition].paused);
    }

    #[rstest]
    #[tokio::test]
    async fn tracked_refresh_keeps_the_overdue_sibling_eligible_for_closure() {
        let client = make_client(PolymarketDataClientConfig::default());
        let ctx = client.resolution_context();
        let now = ctx.clock.get_time_ns();
        let original = tracked_market("2025-01-01T00:00:00Z", "0.03", false);
        let closed = tracked_market("2099-12-01T00:00:00Z", "0.07", true);
        let initial = parse_markets_with_transient(&[original], now).0;
        let refreshed = parse_markets_with_transient(&[closed], now).0;
        let condition = extract_condition_id(&initial[0].id()).unwrap();

        cache_instruments_unchecked(
            &ctx.instruments,
            &ctx.subscriptions.token_meta,
            &[refreshed[0].clone(), initial[1].clone()],
        );
        assert!(
            ctx.subscriptions
                .add_live_subscription_intent(initial[1].id(), SubscriptionKind::Quotes,)
        );
        let snapshot = Arc::clone(&ctx.instruments.load());
        let updates = apply_instrument_refresh(
            &ctx,
            &client.instrument_update_state,
            &snapshot,
            refreshed.clone(),
            true,
            DurationNanos::MAX,
            &mut AHashMap::new(),
        )
        .await;

        assert_eq!(updates, initial.iter().map(Instrument::id).collect());
        let cached = ctx.instruments.load();
        assert_eq!(market_closed(&cached[&initial[0].id()]), Some(true));
        assert_eq!(market_closed(&cached[&initial[1].id()]), Some(false));
        assert_eq!(
            cached[&initial[0].id()].expiration_ns(),
            refreshed[0].expiration_ns()
        );
        assert_eq!(
            cached[&initial[1].id()].expiration_ns(),
            initial[1].expiration_ns()
        );
        assert!(!ctx.subscriptions.is_condition_closed(&condition));

        let addr = market_closure_server(true, false).await;
        let http = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();
        assert_eq!(
            refresh_expired_market_closure(
                &http,
                &ctx.instruments,
                &ctx.data_sender,
                now,
                &ctx.subscriptions,
                None,
                ctx.clock,
            )
            .await
            .unwrap(),
            1
        );
        assert!(ctx.subscriptions.is_condition_closed(&condition));
        assert_eq!(
            market_closed(&ctx.instruments.load()[&initial[1].id()]),
            Some(true)
        );
    }

    #[rstest]
    #[case::already_cached(2)]
    #[case::new_outcome(1)]
    #[case::owner_arrives_during_discovery(0)]
    #[tokio::test]
    async fn discovery_defers_partial_condition_owned_by_omitted_outcome(#[case] cached: usize) {
        let mut client = make_client(PolymarketDataClientConfig::default());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.data_sender = tx.into();
        let ctx = client.resolution_context();
        let mut previous = tracked_market("2099-01-01T00:00:00Z", "0.03", false);
        previous.parent_event = Some(Arc::new(
            from_str(r#"{"id":"123","gameId":"456","category":"Sports"}"#).unwrap(),
        ));
        let initial = parse_markets_with_transient(&[previous], UnixNanos::default()).0;
        cache_instruments_unchecked(
            &ctx.instruments,
            &ctx.subscriptions.token_meta,
            &initial[..cached],
        );
        let snapshot = Arc::clone(&ctx.instruments.load());

        if cached == 0 {
            cache_instrument_unchecked(
                &ctx.instruments,
                &ctx.subscriptions.token_meta,
                &initial[0],
            );
        }

        assert!(
            ctx.subscriptions
                .add_live_subscription_intent(initial[0].id(), SubscriptionKind::Quotes,)
        );

        let updated = tracked_market("2099-12-01T00:00:00Z", "0.07", true);
        let mut discovered = updated.clone();
        discovered.game_id = Some("789".into());
        discovered.parent_event = Some(Arc::new(
            from_str(r#"{"id":"123","gameId":"789","category":"Sports"}"#).unwrap(),
        ));
        let discovered = parse_markets_with_transient(&[discovered], UnixNanos::from(1)).0;
        let mut discovery_events = AHashMap::new();
        let discovery = apply_instrument_refresh(
            &ctx,
            &client.instrument_update_state,
            &snapshot,
            vec![discovered[1].clone()],
            false,
            DurationNanos::MAX,
            &mut discovery_events,
        )
        .await;

        if cached == 2 {
            assert!(discovery.is_empty());
            assert!(rx.try_recv().is_err());
        } else {
            assert_eq!(discovery, AHashSet::from([initial[1].id()]));

            let DataEvent::Data(Data::Instrument(instrument)) = rx.try_recv().unwrap() else {
                panic!("expected new discovery outcome");
            };

            assert_eq!(instrument.id(), initial[1].id());
            assert_eq!(market_closed(&instrument), Some(false));
            assert_eq!(instrument.info().unwrap()["fee_schedule"]["rate"], "0.07");
        }

        let fallback_snapshot = Arc::clone(&ctx.instruments.load());
        let updates = parse_markets_with_transient(&[updated], UnixNanos::from(2)).0;
        let tracked = apply_instrument_refresh(
            &ctx,
            &client.instrument_update_state,
            &fallback_snapshot,
            updates,
            true,
            DurationNanos::MAX,
            &mut discovery_events,
        )
        .await;
        assert_eq!(
            tracked,
            initial.iter().map(Instrument::id).collect::<AHashSet<_>>()
        );

        let cached = ctx.instruments.load();
        for (source, game_id) in [(&initial[0], "456"), (&discovered[1], "789")] {
            let instrument = &cached[&source.id()];
            let info = instrument.info().unwrap();
            assert_eq!(market_closed(instrument), Some(false));
            assert_eq!(info["fee_schedule"]["rate"], "0.07");
            assert_eq!(info.get_str("game_id"), Some(game_id));
            assert!(ptr_eq(
                info.get("gamma_event").unwrap(),
                source.info().unwrap().get("gamma_event").unwrap()
            ));
        }
    }

    #[rstest]
    fn discovery_preserves_context_when_owner_arrives_before_commit() {
        let client = make_client(PolymarketDataClientConfig::default());
        let ctx = client.resolution_context();
        let now = ctx.clock.get_time_ns();
        let mut event_market = tracked_market("2099-01-01T00:00:00Z", "0.03", false);
        event_market.parent_event = Some(Arc::new(
            from_str(r#"{"id":"123","gameId":"456","category":"Sports"}"#).unwrap(),
        ));
        enrich_market_fee_schedule(&mut event_market);
        let initial = parse_markets_with_transient(&[event_market], now).0;
        cache_instruments_unchecked(&ctx.instruments, &ctx.subscriptions.token_meta, &initial);

        let snapshot = Arc::clone(&ctx.instruments.load());
        let refreshed = tracked_market("2099-12-01T00:00:00Z", "0.07", false);
        let refreshed = parse_markets_with_transient(&[refreshed], now).0;
        let expiry = refreshed[0].expiration_ns();
        let mut discovery_events = AHashMap::new();
        let conditions = prepare_refresh_batches(&snapshot, refreshed, &discovery_events, now);

        assert!(
            ctx.subscriptions
                .add_live_subscription_intent(initial[0].id(), SubscriptionKind::Quotes,)
        );
        let (published, _) = commit_instrument_refresh(
            &ctx,
            &client.instrument_update_state,
            &snapshot,
            conditions,
            false,
            DurationNanos::MAX,
            &mut discovery_events,
        );

        assert_eq!(published, initial.iter().map(Instrument::id).collect());
        let cached = ctx.instruments.load();
        for source in &initial {
            let instrument = &cached[&source.id()];
            let info = instrument.info().unwrap();
            assert_eq!(instrument.expiration_ns(), expiry);
            assert_eq!(info.get_str("game_id"), Some("456"));
            assert_eq!(info["fee_schedule"]["rate"], "0.07");
            assert_eq!(info["fee_schedule"]["rebateRate"], "0.15");
            assert!(ptr_eq(
                info.get("gamma_event").unwrap(),
                source.info().unwrap().get("gamma_event").unwrap(),
            ));
        }
    }

    #[rstest]
    fn discovery_retains_distinct_outcome_enrichment() {
        let client = make_client(PolymarketDataClientConfig::default());
        let ctx = client.resolution_context();
        let mut previous = tracked_market("2099-01-01T00:00:00Z", "0.03", false);
        previous.parent_event = Some(Arc::new(
            from_str(r#"{"id":"123","gameId":"456","category":"Sports"}"#).unwrap(),
        ));
        let initial = parse_markets_with_transient(&[previous], UnixNanos::default()).0;
        cache_instruments_unchecked(&ctx.instruments, &ctx.subscriptions.token_meta, &initial);
        assert!(
            ctx.subscriptions
                .add_live_subscription_intent(initial[0].id(), SubscriptionKind::Quotes,)
        );

        let first = tracked_market("2099-12-01T00:00:00Z", "0.07", false);
        let mut second = tracked_market("2099-12-01T00:00:00Z", "0.08", false);
        second.parent_event = Some(Arc::new(
            from_str(r#"{"id":"123","gameId":"789","category":"Sports"}"#).unwrap(),
        ));
        second.game_id = Some("789".into());
        let mut first = parse_markets_with_transient(&[first], UnixNanos::from(1)).0;
        let mut second = parse_markets_with_transient(&[second], UnixNanos::from(2)).0;
        let updates = vec![first.swap_remove(0), second.pop().unwrap()];

        let conditions = prepare_refresh_batches(
            &ctx.instruments.load(),
            updates,
            &AHashMap::new(),
            ctx.clock.get_time_ns(),
        );
        let refreshed = &conditions[&extract_condition_id(&initial[0].id()).unwrap()];
        let first = refreshed[0].info().unwrap();
        let second = refreshed[1].info().unwrap();
        assert_eq!(first.get_str("game_id"), Some("456"));
        assert_eq!(first["fee_schedule"]["rate"], "0.07");
        assert_eq!(second.get_str("game_id"), Some("789"));
        assert_eq!(second["fee_schedule"]["rate"], "0.08");
        assert_eq!(
            second.get_str("gamma_event"),
            Some(r#"{"id":"123","gameId":"789","category":"Sports"}"#)
        );
    }

    fn tracked_market(end_date: &str, rate: &str, closed: bool) -> GammaMarket {
        let mut raw = past_end_open_market();
        raw["endDate"] = serde_json::json!(end_date);
        raw["orderPriceMinTickSize"] = serde_json::json!(0.01);
        raw["closed"] = serde_json::json!(closed);
        raw["events"] = serde_json::json!([{"id":"123"}]);
        raw["feeSchedule"] = serde_json::json!({
            "rate": rate.parse::<Number>().unwrap(),
            "exponent": 1,
            "rebateRate": 0,
            "takerOnly": true,
        });
        from_value(raw).unwrap()
    }

    #[rstest]
    fn instrument_selection_uses_corrected_effective_fees() {
        let update_state = Mutex::new(InstrumentUpdateState::default());
        let instruments = AtomicMap::new();
        let mut market = tracked_market("2099-12-01T00:00:00Z", "0.07", false);
        market.parent_event = Some(Arc::new(
            from_str(r#"{"id":"123","category":"Sports"}"#).unwrap(),
        ));
        let original = parse_markets_with_transient(from_ref(&market), UnixNanos::from(1))
            .0
            .pop()
            .unwrap();
        instruments.insert(original.id(), original.clone());
        let snapshot = Arc::clone(&instruments.load());
        enrich_market_fee_schedule(&mut market);
        let corrected = parse_markets_with_transient(&[market], UnixNanos::from(2))
            .0
            .pop()
            .unwrap();
        instruments.insert(corrected.id(), corrected.clone());
        assert_eq!(
            original.info().unwrap()["gamma_market"],
            corrected.info().unwrap()["gamma_market"]
        );
        assert_eq!(original.info().unwrap()["fee_schedule"]["rebateRate"], "0");

        let Ok(selected) = select_current_instrument(
            &update_state,
            &instruments,
            &snapshot,
            &original,
            |instrument| instrument.info().unwrap()["fee_schedule"]["rebateRate"] == "0.15",
            &CancellationToken::new(),
        ) else {
            panic!("expected the corrected rebate to pass the filter");
        };

        assert!(!selected.replace);
        assert_eq!(
            selected.instrument.info().unwrap()["fee_schedule"]["rebateRate"],
            "0.15"
        );
    }

    #[rstest]
    fn instrument_selection_rechecks_a_definition_changed_during_filtering() {
        let update_state = Mutex::new(InstrumentUpdateState::default());
        let instruments = AtomicMap::new();
        let mut revised = past_end_open_instrument();
        let original_expiry = revised.expiration_ns;
        let id = revised.id;
        let original = InstrumentAny::BinaryOption(revised.clone());
        instruments.insert(id, original.clone());
        let snapshot = Arc::clone(&instruments.load());
        revised.expiration_ns = original_expiry.saturating_add(DurationNanos::new(1));
        let revised_expiry = revised.expiration_ns;
        let seen = RefCell::new(Vec::new());

        let selected = select_current_instrument(
            &update_state,
            &instruments,
            &snapshot,
            &original,
            |instrument| {
                let _update = update_state
                    .try_lock()
                    .expect("filter must run outside the guard");
                let mut seen = seen.borrow_mut();
                if seen.is_empty() {
                    instruments.insert(id, InstrumentAny::BinaryOption(revised.clone()));
                }

                seen.push(instrument.expiration_ns().unwrap());
                instrument.expiration_ns() == Some(original_expiry)
            },
            &CancellationToken::new(),
        );

        assert!(matches!(selected, Err(InstrumentSelectionError::Filtered)));
        assert_eq!(seen.into_inner(), [original_expiry, revised_expiry]);
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
                &thread_state.lock(),
                &instruments,
                &token_meta,
                true,
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
        let (mut siblings, _) = parse_markets_with_transient(
            &[from_value(past_end_open_market()).unwrap()],
            probe_time,
        );
        let mut sibling = siblings.pop().unwrap();

        let InstrumentAny::BinaryOption(binary) = &mut sibling else {
            panic!("Expected sibling binary option");
        };

        binary.expiration_ns = probe_time.saturating_add(DurationNanos::new(10));
        assert_ne!(instrument.id(), sibling.id());
        instruments.insert(sibling.id(), sibling.clone());
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

        assert_eq!(result.unwrap(), 2 * usize::from(closed));
        let cached = instruments.get_cloned(&instrument.id()).unwrap();
        let cached_sibling = instruments.get_cloned(&sibling.id()).unwrap();
        assert_eq!(market_closed(&cached), Some(closed));
        assert_eq!(market_closed(&cached_sibling), Some(closed));
        assert_eq!(cached_sibling.expiration_ns(), sibling.expiration_ns());

        // Gamma reports 0.01; preserve the tick size updated while the probe was pending
        assert_eq!(cached.price_increment(), Price::from("0.005"));

        if closed {
            let mut events = AHashMap::new();

            for _ in 0..2 {
                let event = rx.try_recv().expect("closure instrument event");

                let DataEvent::Data(Data::Instrument(published)) = event else {
                    panic!("Expected instrument event, was {event:?}");
                };

                assert_eq!(market_closed(&published), Some(true));
                assert_eq!(published.ts_init(), observed_at);
                assert_eq!(published.ts_event(), observed_at);
                events.insert(published.id(), *published);
            }

            assert_eq!(events.remove(&cached.id()), Some(cached));
            assert_eq!(events.remove(&cached_sibling.id()), Some(cached_sibling));
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
