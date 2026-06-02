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

//! Polymarket condition resolution tracking and reconciliation.

use std::sync::{Arc, Mutex as StdMutex};

use ahash::AHashSet;
use nautilus_common::messages::DataEvent;
use nautilus_core::{AtomicMap, Params, UnixNanos, time::AtomicTime};
use nautilus_model::{
    data::{
        Data as NautilusData, HasTsInit, InstrumentClose, InstrumentStatus, custom::CustomDataTrait,
    },
    enums::{InstrumentCloseType, MarketStatusAction},
    events::PositionEvent,
    identifiers::{InstrumentId, PositionId},
    instruments::{Instrument, InstrumentAny},
    types::Price,
};
#[cfg(feature = "python")]
use pyo3::types::PyDictMethods;
use serde::{Deserialize, Serialize};

use crate::{
    common::consts::{GAMMA_CONDITION_IDS_BATCH_SIZE, POLYMARKET_VENUE},
    http::{
        clob::PolymarketClobPublicClient,
        gamma::PolymarketGammaHttpClient,
        models::{ClobMarketResponse, GammaMarket},
        query::GetGammaMarketsParams,
    },
    providers::extract_condition_id,
};

pub(crate) const RESOLVE_REQUEST_TYPE_NAME: &str = "PolymarketResolveRequest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrackedInstrument {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) token_id: String,
    pub(crate) price_precision: u8,
    pub(crate) open_position_ids: AHashSet<PositionId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolveWatchEntry {
    pub(crate) condition_id: String,
    pub(crate) expiration_ns: UnixNanos,
    pub(crate) tracked: ahash::AHashMap<String, TrackedInstrument>,
    pub(crate) paused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolveWatchSelectionMode {
    AutoPoll,
    ManualFallback,
    ManualAllEligible,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ResolveWatchSelection {
    pub(crate) condition_ids: Vec<String>,
    pub(crate) skipped_not_expired: usize,
    pub(crate) timed_out_watchlist: usize,
    pub(crate) paused_watchlist: usize,
    pub(crate) min_ready_in_secs: Option<u64>,
    pub(crate) pause_condition_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResolveRequestSummary {
    pub(crate) requested_condition_ids: Vec<String>,
    pub(crate) fetched_markets: usize,
    pub(crate) resolved_markets: usize,
    pub(crate) skipped_non_binary_markets: usize,
    pub(crate) clob_fallback_successes: usize,
    pub(crate) emitted_condition_ids: Vec<String>,
    pub(crate) failed_condition_ids: Vec<String>,
    pub(crate) used_watchlist_fallback: bool,
    pub(crate) timed_out_watchlist: usize,
    pub(crate) error: Option<String>,
}

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

#[derive(Clone)]
pub(crate) struct ResolveContext {
    pub(crate) clock: &'static AtomicTime,
    pub(crate) data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    pub(crate) watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    pub(crate) apply_mutex: Arc<StdMutex<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PolymarketResolveRequestSummaryData {
    pub(crate) requested_condition_ids: Vec<String>,
    pub(crate) fetched_markets: usize,
    pub(crate) resolved_markets: usize,
    pub(crate) skipped_non_binary_markets: usize,
    pub(crate) clob_fallback_successes: usize,
    pub(crate) emitted_condition_ids: Vec<String>,
    pub(crate) failed_condition_ids: Vec<String>,
    pub(crate) used_watchlist_fallback: bool,
    pub(crate) timed_out_watchlist: usize,
    pub(crate) error: Option<String>,
    pub(crate) ts_event: UnixNanos,
    pub(crate) ts_init: UnixNanos,
}

impl PolymarketResolveRequestSummaryData {
    pub(crate) fn from_summary(summary: ResolveRequestSummary, ts_now: UnixNanos) -> Self {
        Self {
            requested_condition_ids: summary.requested_condition_ids,
            fetched_markets: summary.fetched_markets,
            resolved_markets: summary.resolved_markets,
            skipped_non_binary_markets: summary.skipped_non_binary_markets,
            clob_fallback_successes: summary.clob_fallback_successes,
            emitted_condition_ids: summary.emitted_condition_ids,
            failed_condition_ids: summary.failed_condition_ids,
            used_watchlist_fallback: summary.used_watchlist_fallback,
            timed_out_watchlist: summary.timed_out_watchlist,
            error: summary.error,
            ts_event: ts_now,
            ts_init: ts_now,
        }
    }
}

impl HasTsInit for PolymarketResolveRequestSummaryData {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for PolymarketResolveRequestSummaryData {
    fn type_name(&self) -> &'static str {
        RESOLVE_REQUEST_TYPE_NAME
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(other) = other.as_any().downcast_ref::<Self>() {
            self == other
        } else {
            false
        }
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        let dict = pyo3::types::PyDict::new(py);
        dict.set_item(
            "requested_condition_ids",
            self.requested_condition_ids.clone(),
        )?;
        dict.set_item("fetched_markets", self.fetched_markets)?;
        dict.set_item("resolved_markets", self.resolved_markets)?;
        dict.set_item(
            "skipped_non_binary_markets",
            self.skipped_non_binary_markets,
        )?;
        dict.set_item("clob_fallback_successes", self.clob_fallback_successes)?;
        dict.set_item("emitted_condition_ids", self.emitted_condition_ids.clone())?;
        dict.set_item("failed_condition_ids", self.failed_condition_ids.clone())?;
        dict.set_item("used_watchlist_fallback", self.used_watchlist_fallback)?;
        dict.set_item("timed_out_watchlist", self.timed_out_watchlist)?;
        dict.set_item("error", self.error.clone())?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        Ok(dict.unbind().into())
    }

    fn type_name_static() -> &'static str {
        RESOLVE_REQUEST_TYPE_NAME
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let parsed: Self = serde_json::from_value(value)?;
        Ok(Arc::new(parsed))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StrictResolvedMarket {
    pub(crate) condition_id: String,
    pub(crate) winning_asset_id: String,
    pub(crate) winning_outcome: String,
}

pub(crate) fn instrument_market_context(
    instrument: &InstrumentAny,
) -> (Option<String>, Option<String>, Option<String>) {
    match instrument {
        InstrumentAny::BinaryOption(binary) => {
            let slug = binary
                .info
                .as_ref()
                .and_then(|info| info.get_str("market_slug"))
                .map(ToString::to_string);
            let market_id = binary
                .info
                .as_ref()
                .and_then(|info| info.get_str("market_id"))
                .map(ToString::to_string);
            let condition_id = binary
                .info
                .as_ref()
                .and_then(|info| info.get_str("condition_id"))
                .map(ToString::to_string);
            (slug, market_id, condition_id)
        }
        _ => (None, None, None),
    }
}

fn binary_option_context(
    instrument: &InstrumentAny,
) -> Option<(String, String, UnixNanos, TrackedInstrument)> {
    if !matches!(instrument, InstrumentAny::BinaryOption(_)) {
        return None;
    }

    let expiration_ns = instrument.expiration_ns()?;
    let (_, _, condition_id) = instrument_market_context(instrument);
    let condition_id = condition_id.or_else(|| extract_condition_id(&instrument.id()).ok())?;
    let token_id = instrument.raw_symbol().as_str().to_string();
    let tracked = TrackedInstrument {
        instrument_id: instrument.id(),
        token_id: token_id.clone(),
        price_precision: instrument.price_precision(),
        open_position_ids: AHashSet::new(),
    };

    Some((condition_id, token_id, expiration_ns, tracked))
}

pub(crate) fn upsert_resolve_watch_entry_from_instrument(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    instrument: &InstrumentAny,
    position_id: PositionId,
) {
    let Some((condition_id, token_id, expiration_ns, tracked)) = binary_option_context(instrument)
    else {
        return;
    };

    watchlist.rcu(|entries| {
        let entry = entries
            .entry(condition_id.clone())
            .or_insert_with(|| ResolveWatchEntry {
                condition_id: condition_id.clone(),
                expiration_ns,
                tracked: ahash::AHashMap::new(),
                paused: false,
            });
        entry.expiration_ns = expiration_ns;
        entry
            .tracked
            .entry(token_id.clone())
            .and_modify(|existing| {
                existing.open_position_ids.insert(position_id);
            })
            .or_insert_with(|| {
                let mut seeded = tracked.clone();
                seeded.open_position_ids.insert(position_id);
                seeded
            });
    });
}

fn remove_resolve_watch_instrument(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    instrument: &InstrumentAny,
    position_id: PositionId,
) {
    let Some((condition_id, token_id, _expiration_ns, _tracked)) =
        binary_option_context(instrument)
    else {
        return;
    };

    watchlist.rcu(|entries| {
        let remove_entry = match entries.get_mut(&condition_id) {
            Some(entry) => {
                let remove_token = match entry.tracked.get_mut(&token_id) {
                    Some(tracked) => {
                        tracked.open_position_ids.remove(&position_id);
                        tracked.open_position_ids.is_empty()
                    }
                    None => false,
                };

                if remove_token {
                    entry.tracked.remove(&token_id);
                }
                entry.tracked.is_empty()
            }
            None => false,
        };

        if remove_entry {
            entries.remove(&condition_id);
        }
    });
}

pub(crate) fn update_resolve_watchlist_from_position_event(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    event: &PositionEvent,
) {
    let instrument_id = event.instrument_id();
    if instrument_id.venue != *POLYMARKET_VENUE {
        return;
    }

    let loaded = instruments.load();
    let Some(instrument) = loaded.get(&instrument_id) else {
        return;
    };

    let position_id = match event {
        PositionEvent::PositionOpened(position) => position.position_id,
        PositionEvent::PositionChanged(position) => position.position_id,
        PositionEvent::PositionClosed(position) => position.position_id,
        PositionEvent::PositionAdjusted(position) => position.position_id,
    };

    match event {
        PositionEvent::PositionClosed(_) => {
            remove_resolve_watch_instrument(watchlist, instrument, position_id);
        }
        PositionEvent::PositionOpened(_)
        | PositionEvent::PositionChanged(_)
        | PositionEvent::PositionAdjusted(_) => {
            upsert_resolve_watch_entry_from_instrument(watchlist, instrument, position_id);
        }
    }
}

pub(crate) fn collect_resolve_watch_selection(
    watchlist: &ahash::AHashMap<String, ResolveWatchEntry>,
    now_ns: UnixNanos,
    grace_secs: u64,
    max_wait_secs: u64,
    mode: ResolveWatchSelectionMode,
) -> ResolveWatchSelection {
    let mut selection = ResolveWatchSelection::default();
    let grace_ns = grace_secs.saturating_mul(1_000_000_000);
    let max_wait_ns = max_wait_secs.saturating_mul(1_000_000_000);

    for (condition_id, entry) in watchlist {
        if entry.tracked.is_empty() {
            continue;
        }

        let ready_at_ns = entry.expiration_ns.as_u64().saturating_add(grace_ns);
        if now_ns.as_u64() < ready_at_ns {
            selection.skipped_not_expired += 1;
            let ready_in_secs = (ready_at_ns - now_ns.as_u64()) / 1_000_000_000;
            selection.min_ready_in_secs = Some(
                selection
                    .min_ready_in_secs
                    .map_or(ready_in_secs, |current| current.min(ready_in_secs)),
            );
            continue;
        }

        let timed_out = now_ns.as_u64() >= entry.expiration_ns.as_u64().saturating_add(max_wait_ns);

        if timed_out {
            selection.timed_out_watchlist += 1;
            if entry.paused {
                selection.paused_watchlist += 1;
            } else {
                selection.pause_condition_ids.push(condition_id.clone());
            }

            if mode == ResolveWatchSelectionMode::AutoPoll {
                continue;
            }
        } else if entry.paused {
            selection.paused_watchlist += 1;

            if mode == ResolveWatchSelectionMode::AutoPoll {
                continue;
            }
        } else if mode == ResolveWatchSelectionMode::ManualFallback {
            continue;
        }

        selection.condition_ids.push(condition_id.clone());
    }

    selection
}

pub(crate) fn pause_resolve_watch_entries(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    condition_ids: &[String],
) {
    if condition_ids.is_empty() {
        return;
    }

    watchlist.rcu(|entries| {
        for condition_id in condition_ids {
            if let Some(entry) = entries.get_mut(condition_id) {
                entry.paused = true;
            }
        }
    });
}

fn parse_json_string_array(raw: &str) -> Option<Vec<String>> {
    serde_json::from_str::<Vec<String>>(raw)
        .ok()
        .filter(|values| !values.is_empty())
}

fn parse_string_array_param(value: &serde_json::Value) -> Option<Vec<String>> {
    match value {
        serde_json::Value::String(single) => {
            if single.is_empty() {
                return None;
            }
            Some(vec![single.clone()])
        }
        serde_json::Value::Array(items) => {
            let mut parsed = Vec::with_capacity(items.len());
            for item in items {
                let value = item.as_str()?;
                if value.is_empty() {
                    return None;
                }
                parsed.push(value.to_string());
            }
            (!parsed.is_empty()).then_some(parsed)
        }
        _ => None,
    }
}

fn parse_outcome_prices(raw: &Option<String>) -> Option<Vec<f64>> {
    let raw = raw.as_ref()?;

    if let Ok(values) = serde_json::from_str::<Vec<f64>>(raw)
        && !values.is_empty()
    {
        return Some(values);
    }

    let as_strings = serde_json::from_str::<Vec<String>>(raw).ok()?;
    let mut values = Vec::with_capacity(as_strings.len());
    for value in as_strings {
        values.push(value.parse::<f64>().ok()?);
    }
    (!values.is_empty()).then_some(values)
}

fn strict_winner_index(prices: &[f64]) -> Option<usize> {
    if prices.is_empty() {
        return None;
    }

    let mut winner_idx: Option<usize> = None;

    for (idx, value) in prices.iter().copied().enumerate() {
        if value >= 0.999 {
            if winner_idx.is_some() {
                return None;
            }
            winner_idx = Some(idx);
        } else if value > 0.001 {
            return None;
        }
    }

    winner_idx
}

pub(crate) fn build_strict_resolved_market(market: &GammaMarket) -> Option<StrictResolvedMarket> {
    if market.closed != Some(true) {
        return None;
    }

    let asset_ids = parse_json_string_array(&market.clob_token_ids)?;
    if asset_ids.len() != 2 {
        return None;
    }

    let outcomes = parse_json_string_array(&market.outcomes)?;
    if outcomes.len() != 2 {
        return None;
    }

    let prices = parse_outcome_prices(&market.outcome_prices)?;
    if prices.len() != 2 {
        return None;
    }
    let winner_idx = strict_winner_index(&prices)?;
    let winning_asset_id = asset_ids.get(winner_idx)?.clone();
    let winning_outcome = outcomes.get(winner_idx)?.clone();

    Some(StrictResolvedMarket {
        condition_id: market.condition_id.clone(),
        winning_asset_id,
        winning_outcome,
    })
}

pub(crate) fn build_resolved_market_from_clob_market(
    market: &ClobMarketResponse,
) -> Option<StrictResolvedMarket> {
    if !market.closed {
        return None;
    }

    if market.tokens.len() != 2 {
        return None;
    }

    let mut winner_idx: Option<usize> = None;

    for (idx, token) in market.tokens.iter().enumerate() {
        if token.winner {
            if winner_idx.is_some() {
                return None;
            }
            winner_idx = Some(idx);
        }
    }

    let winner_idx = winner_idx?;
    let winner = market.tokens.get(winner_idx)?;
    if winner.token_id.is_empty() || winner.outcome.is_empty() {
        return None;
    }

    Some(StrictResolvedMarket {
        condition_id: market.condition_id.clone(),
        winning_asset_id: winner.token_id.clone(),
        winning_outcome: winner.outcome.clone(),
    })
}

pub(crate) fn parse_condition_ids_from_request_params(params: &Option<Params>) -> Vec<String> {
    let Some(params) = params.as_ref() else {
        return Vec::new();
    };

    let mut condition_ids = Vec::new();

    if let Some(condition_id_value) = params.get("condition_id") {
        if let Some(condition_id) = condition_id_value.as_str() {
            condition_ids.push(condition_id.to_string());
        } else {
            log::warn!(
                "Ignoring invalid `condition_id` param: expected string, received {condition_id_value}"
            );
        }
    }

    if let Some(condition_ids_value) = params.get("condition_ids") {
        if let Some(values) = parse_string_array_param(condition_ids_value) {
            condition_ids.extend(values);
        } else {
            log::warn!(
                "Ignoring invalid `condition_ids` param: expected string or array[string], received {condition_ids_value}"
            );
        }
    }

    if let Some(instrument_ids_value) = params.get("instrument_ids") {
        if let Some(instrument_ids) = parse_string_array_param(instrument_ids_value) {
            for value in instrument_ids {
                if let Ok(instrument_id) = value.parse::<InstrumentId>() {
                    if instrument_id.venue != *POLYMARKET_VENUE {
                        log::warn!(
                            "Ignoring `instrument_ids` entry with non-Polymarket venue: {instrument_id}"
                        );
                        continue;
                    }

                    if let Ok(condition_id) = extract_condition_id(&instrument_id) {
                        condition_ids.push(condition_id);
                    } else {
                        log::warn!(
                            "Ignoring `instrument_ids` entry that cannot extract condition_id: {value}"
                        );
                    }
                } else {
                    log::warn!("Ignoring invalid `instrument_ids` entry: {value}");
                }
            }
        } else {
            log::warn!(
                "Ignoring invalid `instrument_ids` param: expected string or array[string], received {instrument_ids_value}"
            );
        }
    }

    condition_ids.sort();
    condition_ids.dedup();
    condition_ids
}

pub(crate) fn request_params_has_explicit_condition_selector(params: &Option<Params>) -> bool {
    let Some(params) = params.as_ref() else {
        return false;
    };

    params.contains_key("condition_id")
        || params.contains_key("condition_ids")
        || params.contains_key("instrument_ids")
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
            condition_ids: Some(chunk.join(",")),
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
                    let emitted = apply_condition_resolution(
                        ctx,
                        &resolved.condition_id,
                        &resolved.winning_asset_id,
                        &resolved.winning_outcome,
                    );

                    if emitted > 0 {
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
                    let emitted = apply_condition_resolution(
                        ctx,
                        &resolved.condition_id,
                        &resolved.winning_asset_id,
                        &resolved.winning_outcome,
                    );

                    if emitted > 0 {
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
        log::info!(
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

pub(crate) fn merge_resolve_watch_entry(ctx: &ResolveContext, entry: ResolveWatchEntry) {
    let _guard = ctx
        .apply_mutex
        .lock()
        .expect("resolve_apply_mutex poisoned");
    let condition_id = entry.condition_id.clone();
    let incoming_expiration_ns = entry.expiration_ns;
    let incoming_paused = entry.paused;
    let incoming_tracked = entry.tracked;

    ctx.watchlist.rcu(|entries| {
        if let Some(existing) = entries.get_mut(&condition_id) {
            existing.expiration_ns = existing.expiration_ns.max(incoming_expiration_ns);
            existing.paused |= incoming_paused;

            for (token_id, incoming) in &incoming_tracked {
                if let Some(current) = existing.tracked.get_mut(token_id.as_str()) {
                    current
                        .open_position_ids
                        .extend(incoming.open_position_ids.iter().copied());
                } else {
                    existing.tracked.insert(token_id.clone(), incoming.clone());
                }
            }
        } else {
            entries.insert(
                condition_id.clone(),
                ResolveWatchEntry {
                    condition_id: condition_id.clone(),
                    expiration_ns: incoming_expiration_ns,
                    tracked: incoming_tracked.clone(),
                    paused: incoming_paused,
                },
            );
        }
    });
}

pub(crate) fn apply_condition_resolution(
    ctx: &ResolveContext,
    condition_id: &str,
    winning_asset_id: &str,
    winning_outcome: &str,
) -> usize {
    let entry = {
        let _guard = ctx
            .apply_mutex
            .lock()
            .expect("resolve_apply_mutex poisoned");
        let Some(entry) = ctx.watchlist.get_cloned(&condition_id.to_string()) else {
            log::debug!(
                "Ignoring resolution for condition_id={condition_id}: no local watch entry"
            );
            return 0;
        };

        ctx.watchlist.remove(&condition_id.to_string());
        entry
    };

    if entry.tracked.is_empty() {
        return 0;
    }

    let ts_init = ctx.clock.get_time_ns();
    let reason = ustr::Ustr::from(&format!("Winner: {winning_asset_id} ({winning_outcome})"));
    let tracked_instruments: Vec<TrackedInstrument> = entry.tracked.values().cloned().collect();

    for tracked in &tracked_instruments {
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
            merge_resolve_watch_entry(ctx, entry);
            return 0;
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
            merge_resolve_watch_entry(ctx, entry);
            return 0;
        }
    }

    tracked_instruments.len()
}
