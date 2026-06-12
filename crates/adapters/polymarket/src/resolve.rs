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

fn remove_resolve_watch_instrument_by_ids(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    instrument_id: InstrumentId,
    position_id: PositionId,
) {
    watchlist.rcu(|entries| {
        let mut remove_conditions = Vec::new();

        for (condition_id, entry) in entries.iter_mut() {
            let mut remove_tokens = Vec::new();

            for (token_id, tracked) in &mut entry.tracked {
                if tracked.instrument_id != instrument_id {
                    continue;
                }

                tracked.open_position_ids.remove(&position_id);
                if tracked.open_position_ids.is_empty() {
                    remove_tokens.push(token_id.clone());
                }
            }

            for token_id in remove_tokens {
                entry.tracked.remove(&token_id);
            }

            if entry.tracked.is_empty() {
                remove_conditions.push(condition_id.clone());
            }
        }

        for condition_id in remove_conditions {
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

    let position_id = match event {
        PositionEvent::PositionOpened(position) => position.position_id,
        PositionEvent::PositionChanged(position) => position.position_id,
        PositionEvent::PositionClosed(position) => position.position_id,
        PositionEvent::PositionAdjusted(position) => position.position_id,
    };

    match event {
        PositionEvent::PositionClosed(_) => {
            remove_resolve_watch_instrument_by_ids(watchlist, instrument_id, position_id);
        }
        PositionEvent::PositionOpened(_)
        | PositionEvent::PositionChanged(_)
        | PositionEvent::PositionAdjusted(_) => {
            let loaded = instruments.load();
            let Some(instrument) = loaded.get(&instrument_id) else {
                return;
            };
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nautilus_core::{UUID4, time::get_atomic_clock_realtime};
    use nautilus_model::{
        enums::{AssetClass, OrderSide, PositionSide},
        events::{PositionClosed, PositionEvent, PositionOpened},
        identifiers::{AccountId, ClientOrderId, PositionId, StrategyId, Symbol, TraderId},
        instruments::BinaryOption,
        types::{Currency, Money, Quantity},
    };
    #[cfg(feature = "python")]
    use pyo3::types::PyAnyMethods;
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    fn stub_instrument(
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
    ) -> InstrumentAny {
        let price_precision = price_increment.precision;
        let size_precision = size_increment.precision;
        InstrumentAny::BinaryOption(BinaryOption::new(
            InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str()),
            Symbol::new(raw_symbol),
            AssetClass::Alternative,
            Currency::pUSD(),
            UnixNanos::default(),
            UnixNanos::from(u64::MAX),
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[derive(Clone, Copy, Default)]
    struct SeedInstrumentContext<'a> {
        market_slug: Option<&'a str>,
        market_id: Option<&'a str>,
        condition_id: Option<&'a str>,
        expiration_ns: Option<UnixNanos>,
    }

    fn seed_instrument_with_context(
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
        seed_ctx: SeedInstrumentContext<'_>,
    ) -> InstrumentAny {
        let mut inst = stub_instrument(raw_symbol, price_increment, size_increment);
        if let InstrumentAny::BinaryOption(ref mut binary) = inst {
            if let Some(expiration_ns) = seed_ctx.expiration_ns {
                binary.expiration_ns = expiration_ns;
            }

            let mut info = Params::new();
            info.insert(
                "token_id".to_string(),
                serde_json::Value::String(raw_symbol.to_string()),
            );

            if let Some(market_slug) = seed_ctx.market_slug {
                info.insert(
                    "market_slug".to_string(),
                    serde_json::Value::String(market_slug.to_string()),
                );
            }

            if let Some(market_id) = seed_ctx.market_id {
                info.insert(
                    "market_id".to_string(),
                    serde_json::Value::String(market_id.to_string()),
                );
            }

            if let Some(condition_id) = seed_ctx.condition_id {
                info.insert(
                    "condition_id".to_string(),
                    serde_json::Value::String(condition_id.to_string()),
                );
            }

            binary.info = Some(info);
        }

        instruments.insert(inst.id(), inst.clone());
        inst
    }

    fn make_resolve_context() -> (
        ResolveContext,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let ctx = ResolveContext {
            clock: get_atomic_clock_realtime(),
            data_sender: data_tx,
            watchlist: Arc::new(AtomicMap::new()),
            apply_mutex: Arc::new(StdMutex::new(())),
        };

        (ctx, data_rx)
    }

    fn stub_position_opened_event_with_position_id(
        instrument_id: InstrumentId,
        position_id: &str,
    ) -> PositionEvent {
        PositionEvent::PositionOpened(PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            instrument_id,
            position_id: PositionId::new(position_id),
            account_id: AccountId::from("ACCOUNT-001"),
            opening_order_id: ClientOrderId::from("ENTRY-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 1.0,
            quantity: Quantity::from("1"),
            last_qty: Quantity::from("1"),
            last_px: Price::from("0.75"),
            currency: Currency::pUSD(),
            avg_px_open: 0.75,
            event_id: UUID4::new(),
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
        })
    }

    fn stub_position_opened_event(instrument_id: InstrumentId) -> PositionEvent {
        stub_position_opened_event_with_position_id(instrument_id, "P-1")
    }

    fn stub_position_closed_event_with_position_id(
        instrument_id: InstrumentId,
        position_id: &str,
    ) -> PositionEvent {
        PositionEvent::PositionClosed(PositionClosed {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            instrument_id,
            position_id: PositionId::new(position_id),
            account_id: AccountId::from("ACCOUNT-001"),
            opening_order_id: ClientOrderId::from("ENTRY-1"),
            closing_order_id: Some(ClientOrderId::from("EXIT-1")),
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::from("0"),
            peak_quantity: Quantity::from("1"),
            last_qty: Quantity::from("1"),
            last_px: Price::from("1.0"),
            currency: Currency::pUSD(),
            avg_px_open: 0.75,
            avg_px_close: Some(1.0),
            realized_return: 0.3333333333,
            realized_pnl: Some(Money::new(0.25, Currency::pUSD())),
            unrealized_pnl: Money::new(0.0, Currency::pUSD()),
            duration: 1u64,
            event_id: UUID4::new(),
            ts_opened: UnixNanos::from(1),
            ts_closed: Some(UnixNanos::from(2)),
            ts_event: UnixNanos::from(2),
            ts_init: UnixNanos::from(2),
        })
    }

    fn stub_position_closed_event(instrument_id: InstrumentId) -> PositionEvent {
        stub_position_closed_event_with_position_id(instrument_id, "P-1")
    }

    fn make_gamma_market_value_with_outcome_prices(
        condition_id: &str,
        clob_token_ids: &str,
        outcome_prices: Option<&str>,
        closed: Option<bool>,
        accepting_orders: Option<bool>,
    ) -> Value {
        let mut value = serde_json::json!({
            "id": "1557558",
            "conditionId": condition_id,
            "questionID": "0xquestion",
            "clobTokenIds": clob_token_ids,
            "outcomes": "[\"Yes\",\"No\"]",
            "question": "Will test pass?",
            "description": null,
            "startDate": null,
            "endDate": null,
            "active": false,
            "closed": closed,
            "acceptingOrders": accepting_orders,
            "enableOrderBook": false,
            "slug": "test-market",
            "events": []
        });

        if let Some(outcome_prices) = outcome_prices {
            value["outcomePrices"] = serde_json::Value::String(outcome_prices.to_string());
        }

        value
    }

    fn make_gamma_market_with_outcome_prices(
        condition_id: &str,
        clob_token_ids: &str,
        outcome_prices: Option<&str>,
        closed: Option<bool>,
        accepting_orders: Option<bool>,
    ) -> GammaMarket {
        serde_json::from_value(make_gamma_market_value_with_outcome_prices(
            condition_id,
            clob_token_ids,
            outcome_prices,
            closed,
            accepting_orders,
        ))
        .expect("valid gamma market")
    }

    fn load_gamma_market_fixture(filename: &str) -> GammaMarket {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename);
        let content = std::fs::read_to_string(path).expect("fixture missing");
        serde_json::from_str(&content).expect("invalid gamma fixture json")
    }

    fn load_clob_market_fixture(filename: &str) -> ClobMarketResponse {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename);
        let content = std::fs::read_to_string(path).expect("fixture missing");
        serde_json::from_str(&content).expect("invalid clob fixture json")
    }

    #[rstest]
    fn build_strict_resolved_market_requires_closed_and_binary_resolution_prices() {
        let good = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        );
        let resolved = build_strict_resolved_market(&good).expect("expected resolved market");
        assert_eq!(resolved.condition_id, "0xCOND");
        assert_eq!(resolved.winning_asset_id, "0xYES");
        assert_eq!(resolved.winning_outcome, "Yes");

        let ambiguous = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"0.7\",\"0.3\"]"),
            Some(true),
            Some(false),
        );
        assert!(build_strict_resolved_market(&ambiguous).is_none());

        let malformed_token_count = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\",\"0xMAYBE\"]",
            Some("[\"1\",\"0\",\"0\"]"),
            Some(true),
            Some(false),
        );
        assert!(build_strict_resolved_market(&malformed_token_count).is_none());

        let mut malformed_outcome_count = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        );
        malformed_outcome_count.outcomes = "[\"Yes\",\"No\",\"Other\"]".to_string();
        assert!(build_strict_resolved_market(&malformed_outcome_count).is_none());

        let accepting_true = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(true),
        );
        let resolved =
            build_strict_resolved_market(&accepting_true).expect("expected resolved market");
        assert_eq!(resolved.winning_asset_id, "0xYES");

        let not_final = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"1\",\"0\"]"),
            Some(false),
            Some(true),
        );
        assert!(build_strict_resolved_market(&not_final).is_none());
    }

    #[rstest]
    fn build_strict_resolved_market_matches_official_fixture_shapes() {
        let closed = load_gamma_market_fixture("gamma_market_sports_market_money_line.json");
        let resolved = build_strict_resolved_market(&closed).expect("expected resolved fixture");
        assert_eq!(
            resolved.condition_id,
            "0x202abb9a80673068ec5ce9294d60e31eeaf3ab5c82fb21fb0c9142e5d0cab385"
        );
        assert_eq!(
            resolved.winning_asset_id,
            "89972346417086440659189114668296975440208562769200022591480064439842896371398"
        );

        let active = load_gamma_market_fixture("gamma_market.json");
        assert!(build_strict_resolved_market(&active).is_none());
    }

    #[rstest]
    fn build_strict_resolved_market_real_gamma_samples_cover_resolution_buckets() {
        let closed_binary_accepting_false =
            load_gamma_market_fixture("gamma_market_closed_binary_accepting_false.json");
        let resolved = build_strict_resolved_market(&closed_binary_accepting_false)
            .expect("expected resolved market for binary accepting=false fixture");
        assert_eq!(
            resolved.condition_id,
            "0x8ccc3f4951ff02c1d34b87988752b4444ad17228732780a6cf22afefe8478bb6"
        );

        let closed_binary_accepting_true =
            load_gamma_market_fixture("gamma_market_closed_binary_accepting_true.json");
        let resolved = build_strict_resolved_market(&closed_binary_accepting_true)
            .expect("expected resolved market for binary accepting=true fixture");
        assert_eq!(
            resolved.condition_id,
            "0xd57eed0d44f5b8ca54925d8d6ff440b146b3e6e071da18136ee3ee572d34479e"
        );

        let closed_zero_zero =
            load_gamma_market_fixture("gamma_market_closed_zero_zero_legacy.json");
        assert!(build_strict_resolved_market(&closed_zero_zero).is_none());

        let closed_non_binary =
            load_gamma_market_fixture("gamma_market_closed_nonbinary_legacy.json");
        assert!(build_strict_resolved_market(&closed_non_binary).is_none());
    }

    #[rstest]
    fn build_resolved_market_from_clob_market_real_samples() {
        let accepting_false =
            load_clob_market_fixture("clob_market_closed_binary_accepting_false.json");
        let resolved_false = build_resolved_market_from_clob_market(&accepting_false)
            .expect("expected resolved market for accepting=false fixture");
        assert_eq!(
            resolved_false.condition_id,
            "0x8ccc3f4951ff02c1d34b87988752b4444ad17228732780a6cf22afefe8478bb6"
        );
        assert_eq!(resolved_false.winning_outcome, "No");
        assert_eq!(
            resolved_false.winning_asset_id,
            "89711174926330519158043401581181146613785179104141808554061413232025882707365"
        );

        let accepting_true =
            load_clob_market_fixture("clob_market_closed_binary_accepting_true.json");
        let resolved_true = build_resolved_market_from_clob_market(&accepting_true)
            .expect("expected resolved market for accepting=true fixture");
        assert_eq!(
            resolved_true.condition_id,
            "0xd57eed0d44f5b8ca54925d8d6ff440b146b3e6e071da18136ee3ee572d34479e"
        );
        assert_eq!(resolved_true.winning_outcome, "Yes");
        assert_eq!(
            resolved_true.winning_asset_id,
            "22978793223071892222859460592277435458011604214087068523744633723809814935807"
        );
    }

    #[rstest]
    fn parse_condition_ids_supports_single_multi_and_dedup() {
        let mut params = Params::new();
        params.insert("condition_id".to_string(), serde_json::json!("0xCOND-A"));
        params.insert(
            "condition_ids".to_string(),
            serde_json::json!(["0xCOND-B", "0xCOND-A", "0xCOND-B"]),
        );

        let parsed = parse_condition_ids_from_request_params(&Some(params));
        assert_eq!(parsed, vec!["0xCOND-A".to_string(), "0xCOND-B".to_string()]);
    }

    #[rstest]
    fn parse_condition_ids_accepts_single_condition_ids_string() {
        let mut params = Params::new();
        params.insert("condition_ids".to_string(), serde_json::json!("0xCOND-A"));

        let parsed = parse_condition_ids_from_request_params(&Some(params));
        assert_eq!(parsed, vec!["0xCOND-A".to_string()]);
    }

    #[rstest]
    fn parse_condition_ids_ignores_non_polymarket_instrument_ids() {
        let mut params = Params::new();
        params.insert(
            "instrument_ids".to_string(),
            serde_json::json!([
                "0xCOND-A-0xTOKENA.POLYMARKET",
                "BTCUSDT-PERP.BINANCE",
                "ETHUSDT-PERP.BINANCE"
            ]),
        );

        let parsed = parse_condition_ids_from_request_params(&Some(params));
        assert_eq!(parsed, vec!["0xCOND-A".to_string()]);
    }

    #[rstest]
    fn position_events_build_condition_level_watch_entries() {
        let watchlist: Arc<AtomicMap<String, ResolveWatchEntry>> = Arc::new(AtomicMap::new());
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &instruments,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                expiration_ns: Some(expiration_ns),
            },
        );
        let no = seed_instrument_with_context(
            &instruments,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                expiration_ns: Some(expiration_ns),
            },
        );

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_opened_event(yes.id()),
        );
        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_opened_event(no.id()),
        );

        let entries = watchlist.load();
        let entry = entries
            .get("0xCOND-BTC")
            .expect("expected watch entry for condition");
        assert_eq!(entry.tracked.len(), 2);
        assert_eq!(
            entry
                .tracked
                .get("0xTOKEN_YES")
                .expect("expected yes tracked")
                .open_position_ids
                .len(),
            1
        );
        assert_eq!(
            entry
                .tracked
                .get("0xTOKEN_NO")
                .expect("expected no tracked")
                .open_position_ids
                .len(),
            1
        );
        assert!(!entry.paused);
        drop(entries);

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_closed_event(yes.id()),
        );
        let entries = watchlist.load();
        let entry = entries
            .get("0xCOND-BTC")
            .expect("expected remaining condition entry");
        assert_eq!(entry.tracked.len(), 1);
        drop(entries);

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_closed_event(no.id()),
        );
        assert!(!watchlist.contains_key(&"0xCOND-BTC".to_string()));
    }

    #[rstest]
    fn position_events_keep_token_watched_until_last_position_id_closes() {
        let watchlist: Arc<AtomicMap<String, ResolveWatchEntry>> = Arc::new(AtomicMap::new());
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &instruments,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                expiration_ns: Some(expiration_ns),
            },
        );

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_opened_event_with_position_id(yes.id(), "P-1"),
        );
        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_opened_event_with_position_id(yes.id(), "P-2"),
        );

        let entries = watchlist.load();
        let entry = entries
            .get("0xCOND-BTC")
            .expect("expected watch entry for condition");
        let yes_tracked = entry
            .tracked
            .get("0xTOKEN_YES")
            .expect("expected tracked yes token");
        assert_eq!(yes_tracked.open_position_ids.len(), 2);
        drop(entries);

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_closed_event_with_position_id(yes.id(), "P-1"),
        );

        let entries = watchlist.load();
        let entry = entries
            .get("0xCOND-BTC")
            .expect("expected condition still watched");
        let yes_tracked = entry
            .tracked
            .get("0xTOKEN_YES")
            .expect("expected tracked yes token");
        assert_eq!(yes_tracked.open_position_ids.len(), 1);
        assert!(
            yes_tracked
                .open_position_ids
                .contains(&PositionId::new("P-2"))
        );
        drop(entries);

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_closed_event_with_position_id(yes.id(), "P-2"),
        );

        assert!(!watchlist.contains_key(&"0xCOND-BTC".to_string()));
    }

    #[rstest]
    fn position_closed_cleans_watchlist_without_local_instrument_metadata() {
        let watchlist: Arc<AtomicMap<String, ResolveWatchEntry>> = Arc::new(AtomicMap::new());
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &instruments,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                expiration_ns: Some(expiration_ns),
            },
        );

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_opened_event(yes.id()),
        );
        instruments.remove(&yes.id());

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_closed_event(yes.id()),
        );

        assert!(!watchlist.contains_key(&"0xCOND-BTC".to_string()));
    }

    #[rstest]
    fn resolve_watch_selection_deduplicates_shared_condition_ids_and_pauses_timed_out_entries() {
        let now_ns = UnixNanos::from(2_000_000_000_000);
        let mut watchlist = ahash::AHashMap::new();

        let mut tracked = ahash::AHashMap::new();
        tracked.insert(
            "0xYES".to_string(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-A-0xYES.POLYMARKET"),
                token_id: "0xYES".to_string(),
                price_precision: 3,
                open_position_ids: AHashSet::new(),
            },
        );
        tracked.insert(
            "0xNO".to_string(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-A-0xNO.POLYMARKET"),
                token_id: "0xNO".to_string(),
                price_precision: 3,
                open_position_ids: AHashSet::new(),
            },
        );
        watchlist.insert(
            "0xCOND-A".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND-A".to_string(),
                expiration_ns: UnixNanos::from(1_000_000_000_000),
                tracked,
                paused: false,
            },
        );

        let selection = collect_resolve_watch_selection(
            &watchlist,
            now_ns,
            10,
            1800,
            ResolveWatchSelectionMode::AutoPoll,
        );
        assert_eq!(selection.condition_ids, vec!["0xCOND-A".to_string()]);

        let timed_out_now = UnixNanos::from(1_000_000_000_000 + (1900_u64 * 1_000_000_000));
        let selection = collect_resolve_watch_selection(
            &watchlist,
            timed_out_now,
            10,
            1800,
            ResolveWatchSelectionMode::AutoPoll,
        );
        assert!(selection.condition_ids.is_empty());
        assert_eq!(selection.pause_condition_ids, vec!["0xCOND-A".to_string()]);
    }

    #[rstest]
    fn resolve_watch_selection_manual_fallback_only_includes_paused_or_timed_out_entries() {
        let mut watchlist = ahash::AHashMap::new();
        watchlist.insert(
            "0xCOND-PAUSED".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND-PAUSED".to_string(),
                expiration_ns: UnixNanos::from(1_000_000_000_000),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        instrument_id: InstrumentId::from("0xCOND-PAUSED-0xYES.POLYMARKET"),
                        token_id: "0xYES".to_string(),
                        price_precision: 3,
                        open_position_ids: AHashSet::new(),
                    },
                )]),
                paused: true,
            },
        );
        watchlist.insert(
            "0xCOND-ACTIVE".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND-ACTIVE".to_string(),
                expiration_ns: UnixNanos::from(1_000_000_000_000),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        instrument_id: InstrumentId::from("0xCOND-ACTIVE-0xYES.POLYMARKET"),
                        token_id: "0xYES".to_string(),
                        price_precision: 3,
                        open_position_ids: AHashSet::new(),
                    },
                )]),
                paused: false,
            },
        );

        let selection = collect_resolve_watch_selection(
            &watchlist,
            UnixNanos::from(1_100_000_000_000),
            10,
            1800,
            ResolveWatchSelectionMode::ManualFallback,
        );
        assert_eq!(selection.condition_ids, vec!["0xCOND-PAUSED".to_string()]);
    }

    #[rstest]
    fn resolve_watch_selection_manual_all_eligible_includes_expired_unpaused_entries() {
        let mut watchlist = ahash::AHashMap::new();
        watchlist.insert(
            "0xCOND-ACTIVE".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND-ACTIVE".to_string(),
                expiration_ns: UnixNanos::from(1_000_000_000_000),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        instrument_id: InstrumentId::from("0xCOND-ACTIVE-0xYES.POLYMARKET"),
                        token_id: "0xYES".to_string(),
                        price_precision: 3,
                        open_position_ids: AHashSet::new(),
                    },
                )]),
                paused: false,
            },
        );

        let selection = collect_resolve_watch_selection(
            &watchlist,
            UnixNanos::from(1_100_000_000_000),
            10,
            1800,
            ResolveWatchSelectionMode::ManualAllEligible,
        );
        assert_eq!(selection.condition_ids, vec!["0xCOND-ACTIVE".to_string()]);
    }

    #[rstest]
    fn merge_resolve_watch_entry_unions_existing_token_state() {
        let (ctx, _data_rx) = make_resolve_context();
        let condition_id = "0xCOND-BTC".to_string();
        let token_yes = "0xTOKEN_YES".to_string();
        let token_no = "0xTOKEN_NO".to_string();
        let mut existing_tracked = ahash::AHashMap::new();
        existing_tracked.insert(
            token_yes.clone(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-BTC-0xTOKEN_YES.POLYMARKET"),
                token_id: token_yes.clone(),
                price_precision: 3,
                open_position_ids: AHashSet::from_iter([PositionId::new("P-EXISTING")]),
            },
        );
        ctx.watchlist.insert(
            condition_id.clone(),
            ResolveWatchEntry {
                condition_id: condition_id.clone(),
                expiration_ns: UnixNanos::from(1_000),
                tracked: existing_tracked,
                paused: false,
            },
        );

        let mut incoming_tracked = ahash::AHashMap::new();
        incoming_tracked.insert(
            token_yes.clone(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-BTC-0xTOKEN_YES.POLYMARKET"),
                token_id: token_yes.clone(),
                price_precision: 3,
                open_position_ids: AHashSet::from_iter([PositionId::new("P-INCOMING")]),
            },
        );
        incoming_tracked.insert(
            token_no.clone(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-BTC-0xTOKEN_NO.POLYMARKET"),
                token_id: token_no,
                price_precision: 3,
                open_position_ids: AHashSet::from_iter([PositionId::new("P-NO")]),
            },
        );

        merge_resolve_watch_entry(
            &ctx,
            ResolveWatchEntry {
                condition_id: condition_id.clone(),
                expiration_ns: UnixNanos::from(2_000),
                tracked: incoming_tracked,
                paused: true,
            },
        );

        let watchlist = ctx.watchlist.load();
        let entry = watchlist
            .get(&condition_id)
            .expect("expected merged condition entry");
        assert!(entry.paused);
        assert_eq!(entry.expiration_ns, UnixNanos::from(2_000));
        assert_eq!(entry.tracked.len(), 2);
        let yes = entry
            .tracked
            .get(&token_yes)
            .expect("expected merged yes token");
        assert!(
            yes.open_position_ids
                .contains(&PositionId::new("P-EXISTING"))
        );
        assert!(
            yes.open_position_ids
                .contains(&PositionId::new("P-INCOMING"))
        );
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn resolve_request_summary_to_pyobject_returns_dict_payload() {
        let summary = ResolveRequestSummary {
            requested_condition_ids: vec!["0xCOND-A".to_string()],
            fetched_markets: 1,
            resolved_markets: 1,
            skipped_non_binary_markets: 0,
            clob_fallback_successes: 0,
            emitted_condition_ids: vec!["0xCOND-A".to_string()],
            failed_condition_ids: Vec::new(),
            used_watchlist_fallback: false,
            timed_out_watchlist: 0,
            error: None,
        };
        let payload =
            PolymarketResolveRequestSummaryData::from_summary(summary, UnixNanos::from(123_u64));

        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let obj = payload
                .to_pyobject(py)
                .expect("expected summary conversion to Python object");
            let bound = obj.bind(py);
            let dict = bound
                .cast::<pyo3::types::PyDict>()
                .expect("expected Python dict payload");

            let requested = dict
                .get_item("requested_condition_ids")
                .expect("failed to read requested_condition_ids")
                .expect("expected requested_condition_ids");
            let requested_vec: Vec<String> = requested
                .extract()
                .expect("expected requested_condition_ids as list[str]");
            assert_eq!(requested_vec, vec!["0xCOND-A".to_string()]);

            let resolved = dict
                .get_item("resolved_markets")
                .expect("failed to read resolved_markets")
                .expect("expected resolved_markets");
            let resolved_count: usize = resolved
                .extract()
                .expect("expected resolved_markets as integer");
            assert_eq!(resolved_count, 1);

            let skipped = dict
                .get_item("skipped_non_binary_markets")
                .expect("failed to read skipped_non_binary_markets")
                .expect("expected skipped_non_binary_markets");
            let skipped_count: usize = skipped
                .extract()
                .expect("expected skipped_non_binary_markets as integer");
            assert_eq!(skipped_count, 0);

            let clob_successes = dict
                .get_item("clob_fallback_successes")
                .expect("failed to read clob_fallback_successes")
                .expect("expected clob_fallback_successes");
            let clob_success_count: usize = clob_successes
                .extract()
                .expect("expected clob_fallback_successes");
            assert_eq!(clob_success_count, 0);
        });
    }
}
