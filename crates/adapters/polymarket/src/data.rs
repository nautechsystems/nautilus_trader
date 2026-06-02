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

//! Live market data client implementation for the Polymarket adapter.
//!
//! Tick-size changes are handled as book epoch transitions: the local order
//! book is dropped, incremental `price_change` deltas are gated through
//! `pending_snapshot_after_tick_change`, and the gate clears once the next
//! venue snapshot reseeds the book under the new precision. The quote arm of
//! `price_change` stays open through the gap because each payload carries
//! `best_bid` / `best_ask` on the new grid; `last_quotes` is preserved so the
//! unchanged side's size carries forward. See
//! `docs/integrations/polymarket.md` for the full description.

use std::{
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashSet;
use anyhow::Context;
use dashmap::DashMap;
use nautilus_common::{
    clients::DataClient,
    live::{get_runtime, runner::get_data_event_sender},
    messages::{
        DataEvent, DataResponse,
        data::{
            BookResponse, CustomDataResponse, InstrumentResponse, InstrumentsResponse,
            RequestBookSnapshot, RequestCustomData, RequestInstrument, RequestInstruments,
            RequestTrades, SubscribeBookDeltas, SubscribeInstruments, SubscribeQuotes,
            SubscribeTrades, TradesResponse, UnsubscribeBookDeltas, UnsubscribeQuotes,
            UnsubscribeTrades,
        },
    },
    msgbus::{self, TypedHandler},
    providers::InstrumentProvider,
};
use nautilus_core::{
    AtomicMap, AtomicSet, Params, UnixNanos,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{
        CustomData, Data as NautilusData, HasTsInit, InstrumentClose, InstrumentStatus,
        OrderBookDeltas_API, QuoteTick, custom::CustomDataTrait,
    },
    enums::{BookType, InstrumentCloseType, MarketStatusAction},
    events::PositionEvent,
    identifiers::{ClientId, InstrumentId, PositionId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::Price,
};
#[cfg(feature = "python")]
use pyo3::types::PyDictMethods;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::consts::{GAMMA_CONDITION_IDS_BATCH_SIZE, POLYMARKET_VENUE},
    config::PolymarketDataClientConfig,
    filters::InstrumentFilter,
    http::{
        clob::PolymarketClobPublicClient,
        data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient,
        models::{ClobMarketResponse, GammaMarket},
        parse::rebuild_instrument_with_tick_size,
        query::GetGammaMarketsParams,
    },
    providers::{PolymarketInstrumentProvider, extract_condition_id, fetch_configured_instruments},
    websocket::{
        client::PolymarketWebSocketClient,
        messages::{MarketWsMessage, PolymarketQuotes, PolymarketWsMessage},
        parse::{
            parse_book_deltas, parse_book_snapshot, parse_quote_from_price_change,
            parse_quote_from_snapshot, parse_timestamp_ms, parse_trade_tick,
        },
    },
};

const RESOLVE_REQUEST_TYPE_NAME: &str = "PolymarketResolveRequest";

fn resolve_token_id_from(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument_id: InstrumentId,
) -> anyhow::Result<String> {
    let loaded = instruments.load();
    let instrument = loaded
        .get(&instrument_id)
        .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
    Ok(instrument.raw_symbol().as_str().to_string())
}

// Reconciles the WS subscription for `instrument_id` with the union of caller
// intents. Holds `ws_sub_mutex` across the async WS send so concurrent
// subscribe/unsubscribe calls arrive at the WS handler in mutex-release order;
// that makes the final wire state consistent with the last writer.
#[allow(
    clippy::too_many_arguments,
    reason = "shared state comes in as Arc refs"
)]
async fn sync_ws_subscription_async(
    instrument_id: InstrumentId,
    token_id_str: String,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    ws_open_tokens: Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    ws: crate::websocket::client::WsSubscriptionHandle,
) {
    let token_id = Ustr::from(token_id_str.as_str());
    let _guard = ws_sub_mutex.lock().await;

    let wants_subscribe = active_quote_subs.contains(&instrument_id)
        || active_delta_subs.contains(&instrument_id)
        || active_trade_subs.contains(&instrument_id);
    let is_open = ws_open_tokens.contains(&token_id);

    if wants_subscribe && !is_open {
        ws_open_tokens.insert(token_id);

        if let Err(e) = ws.subscribe_market(vec![token_id_str]).await {
            log::error!("Failed to subscribe to market data: {e:?}");
            // Roll back tracked WS state so a retry can take effect.
            ws_open_tokens.remove(&token_id);
        }
    } else if !wants_subscribe && is_open {
        ws_open_tokens.remove(&token_id);

        if let Err(e) = ws.unsubscribe_market(vec![token_id_str]).await {
            log::error!("Failed to unsubscribe from market data: {e:?}");
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TokenMeta {
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrackedInstrument {
    instrument_id: InstrumentId,
    token_id: String,
    price_precision: u8,
    open_position_ids: AHashSet<PositionId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolveWatchEntry {
    condition_id: String,
    expiration_ns: UnixNanos,
    tracked: ahash::AHashMap<String, TrackedInstrument>,
    paused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolveWatchSelectionMode {
    AutoPoll,
    ManualFallback,
    ManualAllEligible,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ResolveWatchSelection {
    condition_ids: Vec<String>,
    skipped_not_expired: usize,
    timed_out_watchlist: usize,
    paused_watchlist: usize,
    min_ready_in_secs: Option<u64>,
    pause_condition_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ResolveRequestSummary {
    requested_condition_ids: Vec<String>,
    fetched_markets: usize,
    resolved_markets: usize,
    emitted_condition_ids: Vec<String>,
    failed_condition_ids: Vec<String>,
    used_watchlist_fallback: bool,
    timed_out_watchlist: usize,
    error: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ResolveApplyBatchStats {
    fetched_markets: usize,
    resolved_markets: usize,
    emitted_condition_ids: Vec<String>,
    failed_condition_ids: Vec<String>,
    error: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ResolveBatchErrorMode {
    Continue,
    StopOnFirstError,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PolymarketResolveRequestSummaryData {
    requested_condition_ids: Vec<String>,
    fetched_markets: usize,
    resolved_markets: usize,
    emitted_condition_ids: Vec<String>,
    failed_condition_ids: Vec<String>,
    used_watchlist_fallback: bool,
    timed_out_watchlist: usize,
    error: Option<String>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

impl PolymarketResolveRequestSummaryData {
    fn from_summary(summary: ResolveRequestSummary, ts_now: UnixNanos) -> Self {
        Self {
            requested_condition_ids: summary.requested_condition_ids,
            fetched_markets: summary.fetched_markets,
            resolved_markets: summary.resolved_markets,
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
struct StrictResolvedMarket {
    condition_id: String,
    winning_asset_id: String,
    winning_outcome: String,
}

fn instrument_market_context(
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

fn upsert_resolve_watch_entry_from_instrument(
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

fn update_resolve_watchlist_from_position_event(
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

fn collect_resolve_watch_selection(
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

fn pause_resolve_watch_entries(
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

fn build_strict_resolved_market(market: &GammaMarket) -> Option<StrictResolvedMarket> {
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

fn build_resolved_market_from_clob_market(
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

fn parse_condition_ids_from_request_params(params: &Option<Params>) -> Vec<String> {
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

fn request_params_has_explicit_condition_selector(params: &Option<Params>) -> bool {
    let Some(params) = params.as_ref() else {
        return false;
    };

    params.contains_key("condition_id")
        || params.contains_key("condition_ids")
        || params.contains_key("instrument_ids")
}

// Inserts `instrument` into the live instrument cache and updates the
// `token_meta` routing index in one step. Every path that populates the live
// cache must go through here so WS messages can always resolve token_id back
// to an InstrumentId.
fn cache_instrument(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    instrument: &InstrumentAny,
) {
    let instrument_id = instrument.id();
    token_meta.insert(
        Ustr::from(instrument.raw_symbol().as_str()),
        TokenMeta {
            instrument_id,
            price_precision: instrument.price_precision(),
            size_precision: instrument.size_precision(),
        },
    );
    instruments.insert(instrument_id, instrument.clone());
}

fn cache_and_publish_instruments(
    instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Vec<InstrumentAny>,
) -> usize {
    let total = instruments.len();

    for instrument in instruments {
        let instrument_id = instrument.id();
        cache_instrument(instruments_cache, token_meta, &instrument);

        if let Err(e) = data_sender.send(DataEvent::Instrument(instrument)) {
            log::warn!("Failed to publish instrument {instrument_id}: {e}");
        }
    }

    total
}

async fn refresh_scoped_instruments(
    http_client: PolymarketGammaHttpClient,
    instrument_config: Option<crate::config::PolymarketInstrumentProviderConfig>,
    filters: Vec<Arc<dyn InstrumentFilter>>,
    instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) -> anyhow::Result<usize> {
    let Some(instrument_config) = instrument_config else {
        return Ok(0);
    };
    let refreshed =
        fetch_configured_instruments(&http_client, &instrument_config, &filters).await?;

    Ok(cache_and_publish_instruments(
        instruments_cache,
        token_meta,
        data_sender,
        refreshed,
    ))
}

struct WsMessageContext {
    clock: &'static AtomicTime,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    token_meta: Arc<DashMap<Ustr, TokenMeta>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    gamma_client: PolymarketGammaHttpClient,
    clob_public_client: PolymarketClobPublicClient,
    filters: Vec<Arc<dyn InstrumentFilter>>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    resolve_poll_watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    resolve_watch_apply_mutex: Arc<StdMutex<()>>,
    pending_snapshot_after_tick_change: Arc<AtomicSet<InstrumentId>>,
    subscribe_new_markets: bool,
    new_market_filter: Option<Arc<dyn InstrumentFilter>>,
    cancellation_token: CancellationToken,
}

/// Polymarket data client for live market data streaming.
///
/// Integrates with the Nautilus DataEngine to provide:
/// - Real-time order book snapshots and deltas via WebSocket
/// - Quote ticks synthesized from book data
/// - Trade ticks from last trade price messages
/// - Automatic instrument discovery from the Gamma API
#[derive(Debug)]
pub struct PolymarketDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: PolymarketDataClientConfig,
    provider: PolymarketInstrumentProvider,
    clob_public_client: PolymarketClobPublicClient,
    data_api_client: PolymarketDataApiHttpClient,
    ws_client: PolymarketWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: Arc<DashMap<Ustr, TokenMeta>>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    resolve_poll_watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    resolve_watch_apply_mutex: Arc<StdMutex<()>>,
    pending_snapshot_after_tick_change: Arc<AtomicSet<InstrumentId>>,
    ws_open_tokens: Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    pending_auto_loads: Arc<StdMutex<AHashSet<InstrumentId>>>,
    auto_load_scheduled: Arc<AtomicBool>,
    position_event_handler: Option<TypedHandler<PositionEvent>>,
}

impl PolymarketDataClient {
    fn merge_resolve_watch_entry(ctx: &WsMessageContext, entry: ResolveWatchEntry) {
        let _guard = ctx
            .resolve_watch_apply_mutex
            .lock()
            .expect("resolve_watch_apply_mutex poisoned");
        let condition_id = entry.condition_id.clone();
        let incoming_expiration_ns = entry.expiration_ns;
        let incoming_paused = entry.paused;
        let incoming_tracked = entry.tracked;

        ctx.resolve_poll_watchlist.rcu(|entries| {
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

    /// Creates a new [`PolymarketDataClient`].
    pub fn new(
        client_id: ClientId,
        config: PolymarketDataClientConfig,
        gamma_client: PolymarketGammaHttpClient,
        clob_public_client: PolymarketClobPublicClient,
        data_api_client: PolymarketDataApiHttpClient,
        ws_client: PolymarketWebSocketClient,
    ) -> Self {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let provider =
            PolymarketInstrumentProvider::new(gamma_client, config.instrument_config.clone());

        Self {
            clock,
            client_id,
            config,
            provider,
            clob_public_client,
            data_api_client,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            token_meta: Arc::new(DashMap::new()),
            order_books: Arc::new(DashMap::new()),
            last_quotes: Arc::new(DashMap::new()),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_delta_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            resolve_poll_watchlist: Arc::new(AtomicMap::new()),
            resolve_watch_apply_mutex: Arc::new(StdMutex::new(())),
            pending_snapshot_after_tick_change: Arc::new(AtomicSet::new()),
            ws_open_tokens: Arc::new(AtomicSet::new()),
            ws_sub_mutex: Arc::new(tokio::sync::Mutex::new(())),
            pending_auto_loads: Arc::new(StdMutex::new(AHashSet::new())),
            auto_load_scheduled: Arc::new(AtomicBool::new(false)),
            position_event_handler: None,
        }
    }

    /// Returns a reference to the client configuration.
    #[must_use]
    pub fn config(&self) -> &PolymarketDataClientConfig {
        &self.config
    }

    async fn fetch_and_apply_resolutions_by_condition_ids(
        gamma_client: &PolymarketGammaHttpClient,
        clob_public_client: &PolymarketClobPublicClient,
        ctx: &WsMessageContext,
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
                    let resolved_by_condition = markets
                        .into_iter()
                        .filter_map(|market| {
                            build_strict_resolved_market(&market)
                                .map(|resolved| (resolved.condition_id.clone(), resolved))
                        })
                        .collect::<ahash::AHashMap<String, StrictResolvedMarket>>();

                    for condition_id in chunk {
                        let Some(resolved) = resolved_by_condition.get(condition_id) else {
                            continue;
                        };

                        stats.resolved_markets += 1;
                        let emitted = Self::apply_condition_resolution(
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

                        stats.resolved_markets += 1;
                        let emitted = Self::apply_condition_resolution(
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

        stats
    }

    fn ensure_position_event_subscription(&mut self) {
        if self.position_event_handler.is_some() {
            return;
        }

        let watchlist = self.resolve_poll_watchlist.clone();
        let instruments = self.instruments.clone();
        let handler = TypedHandler::from(move |event: &PositionEvent| {
            update_resolve_watchlist_from_position_event(&watchlist, &instruments, event);
        });

        msgbus::subscribe_position_events("events.position.*".into(), handler.clone(), Some(10));
        self.position_event_handler = Some(handler);
    }

    fn clear_position_event_subscription(&mut self) {
        if let Some(handler) = self.position_event_handler.take() {
            msgbus::unsubscribe_position_events("events.position.*".into(), &handler);
        }
    }

    /// Returns the venue for this data client.
    #[must_use]
    pub fn venue(&self) -> Venue {
        *POLYMARKET_VENUE
    }

    /// Returns a reference to the instrument provider.
    #[must_use]
    pub fn provider(&self) -> &PolymarketInstrumentProvider {
        &self.provider
    }

    /// Adds an instrument filter on the underlying provider.
    pub fn add_instrument_filter(&mut self, filter: Arc<dyn InstrumentFilter>) {
        self.provider.add_filter(filter);
    }

    /// Returns `true` when the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn resolve_token_id(&self, instrument_id: InstrumentId) -> anyhow::Result<String> {
        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
        Ok(instrument.raw_symbol().as_str().to_string())
    }

    // Spawns an async task that reconciles the WS subscription for
    // `instrument_id`. The task holds `ws_sub_mutex` across the wire send so
    // concurrent subscribe/unsubscribe calls deliver commands to the WS handler
    // in a consistent order with the final `active_*_subs` state.
    fn sync_ws_subscription(&self, instrument_id: InstrumentId) {
        let token_id_str = match self.resolve_token_id(instrument_id) {
            Ok(s) => s,
            Err(_) => return,
        };
        let active_quote_subs = self.active_quote_subs.clone();
        let active_delta_subs = self.active_delta_subs.clone();
        let active_trade_subs = self.active_trade_subs.clone();
        let ws_open_tokens = self.ws_open_tokens.clone();
        let ws_sub_mutex = self.ws_sub_mutex.clone();
        let ws = self.ws_client.clone_subscription_handle();

        get_runtime().spawn(sync_ws_subscription_async(
            instrument_id,
            token_id_str,
            active_quote_subs,
            active_delta_subs,
            active_trade_subs,
            ws_open_tokens,
            ws_sub_mutex,
            ws,
        ));
    }

    fn queue_pending_load(&self, instrument_id: InstrumentId) {
        {
            let mut pending = self
                .pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned");
            pending.insert(instrument_id);
        }

        self.ensure_auto_load_task();
    }

    fn drop_pending_if_unwanted(&self, instrument_id: InstrumentId) {
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

    fn drop_local_book_state_if_unwanted(&self, instrument_id: InstrumentId) {
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
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
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
        let cancellation = self.cancellation_token.clone();

        get_runtime().spawn(async move {
            // Coalesce concurrent misses into one Gamma call.
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(debounce_ms)) => {}
                () = cancellation.cancelled() => {
                    scheduled.store(false, Ordering::Release);
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
            scheduled.store(false, Ordering::Release);

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
                    for inst in loaded {
                        if !filters.iter().all(|f| f.accept(&inst)) {
                            log::debug!("Auto-loaded instrument {} filtered out", inst.id());
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
                    let reason = if chunk_failed {
                        "Gamma fetch failed"
                    } else {
                        "no usable token_id"
                    };

                    for id in &next_batch {
                        log::error!(
                            "Cannot find instrument for {id}: {reason} after {max_retries} retries (CLOB lifecycle race)"
                        );
                    }
                    return;
                }

                let delay = crate::common::retry::auto_load_retry_delay(
                    attempt, base_secs, max_secs,
                );
                let kind = if chunk_failed { "chunk failure" } else { "transient" };
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

    async fn bootstrap_instruments(&mut self) -> anyhow::Result<()> {
        self.provider.initialize(false).await?;

        let total = cache_and_publish_instruments(
            &self.instruments,
            &self.token_meta,
            &self.data_sender,
            self.provider
                .store()
                .list_all()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
        );

        log::info!("Published {total} Polymarket instruments to data engine");
        Ok(())
    }

    fn spawn_instrument_refresh_task(&mut self) {
        let Some(interval_mins) = self.config.update_instruments_interval_mins else {
            return;
        };

        if interval_mins == 0 || self.config.instrument_config.is_none() {
            return;
        }

        let interval = Duration::from_secs(interval_mins.saturating_mul(60));
        let cancellation = self.cancellation_token.clone();
        let http_client = self.provider.http_client().clone();
        let instrument_config = self.config.instrument_config.clone();
        let filters = self.provider.filters();
        let instruments_cache = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let data_sender = self.data_sender.clone();

        let handle = get_runtime().spawn(async move {
            log::debug!("Polymarket instrument refresh task started");

            loop {
                tokio::select! {
                    () = tokio::time::sleep(interval) => {}
                    () = cancellation.cancelled() => {
                        log::debug!("Polymarket instrument refresh task cancelled");
                        break;
                    }
                }

                match refresh_scoped_instruments(
                    http_client.clone(),
                    instrument_config.clone(),
                    filters.clone(),
                    &instruments_cache,
                    &token_meta,
                    &data_sender,
                )
                .await
                {
                    Ok(total) => {
                        if total > 0 {
                            log::info!(
                                "Refreshed {total} Polymarket instruments into the live cache"
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to refresh Polymarket instruments: {e}");
                    }
                }
            }

            log::debug!("Polymarket instrument refresh task ended");
        });

        self.tasks.push(handle);
    }

    fn spawn_message_handler(
        &mut self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>,
    ) {
        let cancellation = self.cancellation_token.clone();

        for (token_id, instrument) in self.provider.build_token_map() {
            self.token_meta.insert(
                token_id,
                TokenMeta {
                    instrument_id: instrument.id(),
                    price_precision: instrument.price_precision(),
                    size_precision: instrument.size_precision(),
                },
            );
        }

        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            gamma_client: self.provider.http_client().clone(),
            clob_public_client: self.clob_public_client.clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            cancellation_token: cancellation.clone(),
        };

        let handle = get_runtime().spawn(async move {
            log::debug!("Polymarket message handler started");

            loop {
                tokio::select! {
                    maybe_msg = rx.recv() => {
                        match maybe_msg {
                            Some(msg) => Self::handle_ws_message(msg, &ctx),
                            None => {
                                log::debug!("WebSocket message channel closed");
                                break;
                            }
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("Polymarket message handler cancelled");
                        break;
                    }
                }
            }

            log::debug!("Polymarket message handler ended");
        });

        self.tasks.push(handle);
    }

    fn spawn_resolve_poll_task(&mut self) {
        if !self.config.resolve_poll_enabled {
            log::info!("Polymarket resolve polling disabled");
            return;
        }

        let cancellation = self.cancellation_token.clone();
        let gamma_client = self.provider.http_client().clone();
        let clob_public_client = self.clob_public_client.clone();
        let clock = self.clock;
        let interval_secs = self.config.resolve_poll_interval_secs.max(1);
        let grace_secs = self.config.resolve_poll_grace_secs;
        let max_wait_secs = self.config.resolve_poll_max_wait_secs.max(grace_secs);

        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            gamma_client: gamma_client.clone(),
            clob_public_client: clob_public_client.clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            cancellation_token: cancellation.clone(),
        };

        let watchlist = self.resolve_poll_watchlist.clone();

        let handle = get_runtime().spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    _ = interval.tick() => {
                        let now_ns = clock.get_time_ns();
                        let snapshot = watchlist.load();
                        let selection = collect_resolve_watch_selection(
                            &snapshot,
                            now_ns,
                            grace_secs,
                            max_wait_secs,
                            ResolveWatchSelectionMode::AutoPoll,
                        );
                        drop(snapshot);

                        pause_resolve_watch_entries(&watchlist, &selection.pause_condition_ids);

                        let _ = Self::fetch_and_apply_resolutions_by_condition_ids(
                            &gamma_client,
                            &clob_public_client,
                            &ctx,
                            &selection.condition_ids,
                            ResolveBatchErrorMode::Continue,
                        )
                        .await;
                    }
                }
            }
        });

        self.tasks.push(handle);
    }

    fn handle_ws_message(message: PolymarketWsMessage, ctx: &WsMessageContext) {
        match message {
            PolymarketWsMessage::Market(market_msg) => {
                Self::handle_market_message(market_msg, ctx);
            }
            PolymarketWsMessage::User(_) => {
                log::debug!("Ignoring user message on data client");
            }
            PolymarketWsMessage::Reconnected => {
                log::info!("Polymarket WS reconnected");
            }
        }
    }

    fn apply_condition_resolution(
        ctx: &WsMessageContext,
        condition_id: &str,
        winning_asset_id: &str,
        winning_outcome: &str,
    ) -> usize {
        let entry = {
            let _guard = ctx
                .resolve_watch_apply_mutex
                .lock()
                .expect("resolve_watch_apply_mutex poisoned");
            let Some(entry) = ctx
                .resolve_poll_watchlist
                .get_cloned(&condition_id.to_string())
            else {
                log::debug!(
                    "Ignoring resolution for condition_id={condition_id}: no local watch entry"
                );
                return 0;
            };

            ctx.resolve_poll_watchlist.remove(&condition_id.to_string());
            entry
        };

        if entry.tracked.is_empty() {
            return 0;
        }

        let ts_init = ctx.clock.get_time_ns();
        let reason = Ustr::from(&format!("Winner: {winning_asset_id} ({winning_outcome})"));
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
                Self::merge_resolve_watch_entry(ctx, entry);
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
                Self::merge_resolve_watch_entry(ctx, entry);
                return 0;
            }
        }

        tracked_instruments.len()
    }

    fn handle_market_message(message: MarketWsMessage, ctx: &WsMessageContext) {
        match message {
            MarketWsMessage::Book(snap) => {
                let token_id = Ustr::from(snap.asset_id.as_str());
                let meta = match ctx.token_meta.get(&token_id) {
                    Some(m) => *m,
                    None => {
                        log::debug!("No instrument for token_id {token_id}");
                        return;
                    }
                };
                let instrument_id = meta.instrument_id;
                let ts_init = ctx.clock.get_time_ns();
                let mut book_seeded = false;

                if ctx.active_delta_subs.contains(&instrument_id) {
                    match parse_book_snapshot(
                        &snap,
                        instrument_id,
                        meta.price_precision,
                        meta.size_precision,
                        ts_init,
                    ) {
                        Ok(deltas) => {
                            let mut book = ctx
                                .order_books
                                .entry(instrument_id)
                                .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

                            match book.apply_deltas(&deltas) {
                                Ok(()) => book_seeded = true,
                                Err(e) => log::error!(
                                    "Failed to apply book snapshot for {instrument_id}: {e}"
                                ),
                            }

                            let data: NautilusData = OrderBookDeltas_API::new(deltas).into();
                            if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                                log::error!("Failed to emit book deltas: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse book snapshot: {e}"),
                    }
                }

                if ctx.active_quote_subs.contains(&instrument_id) {
                    match parse_quote_from_snapshot(
                        &snap,
                        instrument_id,
                        meta.price_precision,
                        meta.size_precision,
                        ts_init,
                    ) {
                        Ok(Some(quote)) => Self::emit_quote_if_changed(ctx, instrument_id, quote),
                        Ok(None) => {}
                        Err(e) => log::error!("Failed to parse quote from snapshot: {e}"),
                    }
                }

                if book_seeded
                    && ctx
                        .pending_snapshot_after_tick_change
                        .contains(&instrument_id)
                {
                    ctx.pending_snapshot_after_tick_change
                        .remove(&instrument_id);
                    log::info!("Resumed book for {instrument_id} after tick size change");
                }
            }

            MarketWsMessage::PriceChange(quotes) => {
                let ts_init = ctx.clock.get_time_ns();
                let ts_event = match parse_timestamp_ms(&quotes.timestamp) {
                    Ok(ts) => ts,
                    Err(e) => {
                        log::error!("Failed to parse price change timestamp: {e}");
                        return;
                    }
                };

                // Each change may belong to a different asset, so resolve per-change
                for change in &quotes.price_changes {
                    let token_id = Ustr::from(change.asset_id.as_str());
                    let meta = match ctx.token_meta.get(&token_id) {
                        Some(m) => *m,
                        None => {
                            log::debug!("No instrument for token_id {token_id}");
                            continue;
                        }
                    };
                    let instrument_id = meta.instrument_id;
                    let pending = ctx
                        .pending_snapshot_after_tick_change
                        .contains(&instrument_id);

                    if pending && ctx.active_delta_subs.contains(&instrument_id) {
                        log::debug!(
                            "Dropping book delta for {instrument_id}: awaiting snapshot after tick size change",
                        );
                    } else if ctx.active_delta_subs.contains(&instrument_id) {
                        let per_asset = PolymarketQuotes {
                            market: quotes.market,
                            price_changes: vec![change.clone()],
                            timestamp: quotes.timestamp.clone(),
                        };

                        match parse_book_deltas(
                            &per_asset,
                            instrument_id,
                            meta.price_precision,
                            meta.size_precision,
                            ts_init,
                        ) {
                            Ok(deltas) => {
                                if let Some(mut book) = ctx.order_books.get_mut(&instrument_id)
                                    && let Err(e) = book.apply_deltas(&deltas)
                                {
                                    log::error!(
                                        "Failed to apply book deltas for {instrument_id}: {e}"
                                    );
                                }

                                let data: NautilusData = OrderBookDeltas_API::new(deltas).into();

                                if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to emit book deltas: {e}");
                                }
                            }
                            Err(e) => log::error!("Failed to parse book deltas: {e}"),
                        }
                    }

                    if ctx.active_quote_subs.contains(&instrument_id) {
                        // Clone and drop guard before emit to avoid DashMap deadlock
                        let last_quote = ctx.last_quotes.get(&instrument_id).map(|r| *r);

                        match parse_quote_from_price_change(
                            change,
                            instrument_id,
                            meta.price_precision,
                            meta.size_precision,
                            last_quote.as_ref(),
                            ts_event,
                            ts_init,
                        ) {
                            Ok(Some(quote)) => {
                                Self::emit_quote_if_changed(ctx, instrument_id, quote);
                            }
                            Ok(None) => {} // Missing best_bid/best_ask
                            Err(e) => {
                                log::error!("Failed to parse quote from price change: {e}");
                            }
                        }
                    }
                }
            }

            MarketWsMessage::LastTradePrice(trade) => {
                let token_id = Ustr::from(trade.asset_id.as_str());
                let meta = match ctx.token_meta.get(&token_id) {
                    Some(m) => *m,
                    None => {
                        log::debug!("No instrument for token_id {token_id}");
                        return;
                    }
                };
                let instrument_id = meta.instrument_id;

                if ctx.active_trade_subs.contains(&instrument_id) {
                    let ts_init = ctx.clock.get_time_ns();

                    match parse_trade_tick(
                        &trade,
                        instrument_id,
                        meta.price_precision,
                        meta.size_precision,
                        ts_init,
                    ) {
                        Ok(tick) => {
                            if let Err(e) = ctx
                                .data_sender
                                .send(DataEvent::Data(NautilusData::Trade(tick)))
                            {
                                log::error!("Failed to emit trade tick: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse trade tick: {e}"),
                    }
                }
            }

            MarketWsMessage::TickSizeChange(change) => {
                let token_id = Ustr::from(change.asset_id.as_str());
                let meta = match ctx.token_meta.get(&token_id) {
                    Some(m) => *m,
                    None => {
                        log::error!("No instrument for token_id {token_id}");
                        return;
                    }
                };

                let tick_size: rust_decimal::Decimal = match change.new_tick_size.parse() {
                    Ok(d) => d,
                    Err(e) => {
                        log::error!(
                            "Failed to parse new tick size '{}': {e}",
                            change.new_tick_size
                        );
                        return;
                    }
                };
                let new_price_precision = tick_size.scale() as u8;

                let instruments = ctx.instruments.load();
                let existing = instruments.get(&meta.instrument_id);

                // No-op tick_size_change must not trigger an epoch transition.
                if let Some(existing_inst) = existing
                    && existing_inst.price_increment().as_decimal() == tick_size
                {
                    log::debug!(
                        "Ignoring duplicate tick size change for {}: {} -> {}",
                        change.asset_id,
                        change.old_tick_size,
                        change.new_tick_size,
                    );
                    return;
                }

                log::info!(
                    "Tick size changed for {}: {} -> {}",
                    change.asset_id,
                    change.old_tick_size,
                    change.new_tick_size
                );

                ctx.token_meta.insert(
                    token_id,
                    TokenMeta {
                        price_precision: new_price_precision,
                        ..meta
                    },
                );

                if let Some(existing) = existing {
                    let ts_init = ctx.clock.get_time_ns();

                    match rebuild_instrument_with_tick_size(
                        existing,
                        &change.new_tick_size,
                        ts_init,
                        ts_init,
                    ) {
                        Ok(rebuilt) => {
                            ctx.instruments.insert(rebuilt.id(), rebuilt.clone());
                            if let Err(e) = ctx.data_sender.send(DataEvent::Instrument(rebuilt)) {
                                log::error!("Failed to emit rebuilt instrument: {e}");
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to rebuild instrument for tick size change: {e}");
                        }
                    }
                }

                // Book epoch transition; see module docs.
                let instrument_id = meta.instrument_id;
                ctx.order_books.remove(&instrument_id);

                if ctx.active_delta_subs.contains(&instrument_id) {
                    ctx.pending_snapshot_after_tick_change.insert(instrument_id);
                }
            }

            MarketWsMessage::NewMarket(nm) => {
                if !ctx.subscribe_new_markets {
                    log::trace!("Ignoring new market event (subscribe_new_markets=false)");
                    return;
                }

                if let Some(ref nf) = ctx.new_market_filter
                    && !nf.accept_new_market(&nm)
                {
                    log::debug!("New market slug={} rejected by new_market_filter", nm.slug);
                    return;
                }

                let gamma_client = ctx.gamma_client.clone();
                let filters = ctx.filters.clone();
                let token_meta = ctx.token_meta.clone();
                let instruments = ctx.instruments.clone();
                let data_sender = ctx.data_sender.clone();
                let clock = ctx.clock;
                let cancellation = ctx.cancellation_token.clone();
                let slug = nm.slug;
                let active = nm.active;

                get_runtime().spawn(async move {
                    let fetch = gamma_client
                        .request_instruments_by_slugs_with_retry(vec![slug.clone()]);

                    let result = tokio::select! {
                        r = fetch => r,
                        () = cancellation.cancelled() => {
                            log::debug!("New market fetch for '{slug}' cancelled during shutdown");
                            return;
                        }
                    };

                    match result {
                        Ok(new_instruments) => {
                            for inst in new_instruments {
                                if cancellation.is_cancelled() {
                                    log::debug!("New market processing cancelled during shutdown");
                                    return;
                                }

                                if !filters.iter().all(|f| f.accept(&inst)) {
                                    log::debug!("New market instrument {} filtered out", inst.id());
                                    continue;
                                }

                                cache_instrument(&instruments, &token_meta, &inst);

                                let instrument_id = inst.id();
                                if let Err(e) = data_sender.send(DataEvent::Instrument(inst)) {
                                    log::error!(
                                        "Failed to emit new market instrument {instrument_id}: {e}"
                                    );
                                }

                                // Emit instrument status based on WS active flag
                                let ts_now = clock.get_time_ns();
                                let action = if active {
                                    MarketStatusAction::Trading
                                } else {
                                    MarketStatusAction::PreOpen
                                };
                                let status = InstrumentStatus::new(
                                    instrument_id,
                                    action,
                                    ts_now,
                                    ts_now,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                );

                                if let Err(e) =
                                    data_sender.send(DataEvent::InstrumentStatus(status))
                                {
                                    log::error!(
                                        "Failed to emit instrument status for {instrument_id}: {e}"
                                    );
                                }
                            }
                        }
                        Err(e) => log::warn!(
                            "Failed to fetch instruments for new market slug '{slug}' after retries: {e}"
                        ),
                    }
                });
            }

            MarketWsMessage::MarketResolved(resolved) => {
                let emitted = Self::apply_condition_resolution(
                    ctx,
                    resolved.market.as_str(),
                    &resolved.winning_asset_id,
                    &resolved.winning_outcome,
                );

                if emitted > 0 {
                    log::info!(
                        "Applied market_resolved for condition_id={} winner={} ({}) tracked_instruments={emitted}",
                        resolved.market,
                        resolved.winning_asset_id,
                        resolved.winning_outcome
                    );
                }
            }

            MarketWsMessage::BestBidAsk(bba) => {
                log::trace!(
                    "best_bid_ask for {}: bid={} ask={}",
                    bba.asset_id,
                    bba.best_bid,
                    bba.best_ask
                );
            }
        }
    }

    fn emit_quote_if_changed(
        ctx: &WsMessageContext,
        instrument_id: InstrumentId,
        quote: QuoteTick,
    ) {
        // Compare prices and sizes only; timestamps always differ between messages
        let emit = !matches!(
            ctx.last_quotes.get(&instrument_id),
            Some(existing) if existing.bid_price == quote.bid_price
                && existing.ask_price == quote.ask_price
                && existing.bid_size == quote.bid_size
                && existing.ask_size == quote.ask_size
        );

        if emit {
            ctx.last_quotes.insert(instrument_id, quote);
            if let Err(e) = ctx
                .data_sender
                .send(DataEvent::Data(NautilusData::Quote(quote)))
            {
                log::error!("Failed to emit quote tick: {e}");
            }
        }
    }

    async fn await_tasks_with_timeout(&mut self, timeout: tokio::time::Duration) {
        for handle in self.tasks.drain(..) {
            let _ = tokio::time::timeout(timeout, handle).await;
        }
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for PolymarketDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*POLYMARKET_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting Polymarket data client: {}", self.client_id);
        self.ensure_position_event_subscription();
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Polymarket data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        self.clear_position_event_subscription();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting Polymarket data client: {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.resolve_poll_watchlist.store(ahash::AHashMap::new());
        self.clear_position_event_subscription();

        for handle in self.tasks.drain(..) {
            handle.abort();
        }
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        self.cancellation_token = CancellationToken::new();
        self.ensure_position_event_subscription();

        log::info!("Connecting Polymarket data client");

        log::info!("Bootstrapping instruments from Gamma API...");
        self.bootstrap_instruments().await?;
        log::info!(
            "Bootstrap complete, {} instruments loaded",
            self.instruments.load().len(),
        );

        self.ws_client.connect().await?;

        if self.config.subscribe_new_markets {
            log::info!("Subscribing to new markets...");
            self.ws_client.subscribe_market(vec![]).await?;
        }

        let rx = self
            .ws_client
            .take_message_receiver()
            .ok_or_else(|| anyhow::anyhow!("WS message receiver not available after connect"))?;

        self.spawn_message_handler(rx);
        self.spawn_instrument_refresh_task();
        self.spawn_resolve_poll_task();

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected Polymarket data client");

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        log::info!("Disconnecting Polymarket data client");

        self.cancellation_token.cancel();
        self.await_tasks_with_timeout(tokio::time::Duration::from_secs(5))
            .await;

        self.ws_client.disconnect().await?;

        self.is_connected.store(false, Ordering::Relaxed);
        self.clear_position_event_subscription();
        log::info!("Disconnected Polymarket data client");

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn request_data(&self, request: RequestCustomData) -> anyhow::Result<()> {
        if request.data_type.type_name() != RESOLVE_REQUEST_TYPE_NAME {
            log::debug!(
                "Ignoring unsupported custom data request type: {}",
                request.data_type.type_name()
            );
            return Ok(());
        }

        let RequestCustomData {
            data_type,
            request_id,
            client_id,
            params: request_params,
            start,
            end,
            ..
        } = request;

        let gamma_client = self.provider.http_client().clone();
        let sender = self.data_sender.clone();
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let clock = self.clock;
        let watchlist = self.resolve_poll_watchlist.clone();
        let resolve_poll_enabled = self.config.resolve_poll_enabled;
        let grace_secs = self.config.resolve_poll_grace_secs;
        let max_wait_secs = self.config.resolve_poll_max_wait_secs.max(grace_secs);
        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            gamma_client: self.provider.http_client().clone(),
            clob_public_client: self.clob_public_client.clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            cancellation_token: self.cancellation_token.clone(),
        };

        get_runtime().spawn(async move {
            let mut summary = ResolveRequestSummary {
                requested_condition_ids: Vec::new(),
                fetched_markets: 0,
                resolved_markets: 0,
                emitted_condition_ids: Vec::new(),
                failed_condition_ids: Vec::new(),
                used_watchlist_fallback: false,
                timed_out_watchlist: 0,
                error: None,
            };

            let has_explicit_selector =
                request_params_has_explicit_condition_selector(&request_params);
            let mut condition_ids = parse_condition_ids_from_request_params(&request_params);
            if condition_ids.is_empty() {
                if has_explicit_selector {
                    summary.error = Some(
                        "No valid Polymarket condition_ids could be resolved from request params"
                            .to_string(),
                    );
                } else {
                    summary.used_watchlist_fallback = true;
                    let snapshot = watchlist.load();
                    let selection_mode = if resolve_poll_enabled {
                        ResolveWatchSelectionMode::ManualFallback
                    } else {
                        ResolveWatchSelectionMode::ManualAllEligible
                    };
                    let selection = collect_resolve_watch_selection(
                        &snapshot,
                        clock.get_time_ns(),
                        grace_secs,
                        max_wait_secs,
                        selection_mode,
                    );
                    drop(snapshot);

                    pause_resolve_watch_entries(&watchlist, &selection.pause_condition_ids);
                    summary.timed_out_watchlist = selection.timed_out_watchlist;
                    condition_ids = selection.condition_ids;
                }
            }

            summary.requested_condition_ids = condition_ids.clone();

            let stats = Self::fetch_and_apply_resolutions_by_condition_ids(
                &gamma_client,
                &ctx.clob_public_client,
                &ctx,
                &condition_ids,
                ResolveBatchErrorMode::StopOnFirstError,
            )
            .await;
            summary.fetched_markets = stats.fetched_markets;
            summary.resolved_markets = stats.resolved_markets;
            summary.emitted_condition_ids = stats.emitted_condition_ids;
            summary.failed_condition_ids = stats.failed_condition_ids;
            if summary.error.is_none() {
                summary.error = stats.error;
            }

            let ts_now = clock.get_time_ns();
            let payload = Arc::new(PolymarketResolveRequestSummaryData::from_summary(
                summary, ts_now,
            ));
            let custom = CustomData::new(payload, data_type.clone());

            let response = DataResponse::Data(CustomDataResponse::new(
                request_id,
                client_id,
                Some(*POLYMARKET_VENUE),
                data_type,
                custom,
                start_nanos,
                end_nanos,
                ts_now,
                request_params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send resolve custom data response: {e}");
            }
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let sender = self.data_sender.clone();
        let instruments = self
            .instruments
            .load()
            .values()
            .filter(|instrument| instrument.id().venue == *POLYMARKET_VENUE)
            .cloned()
            .collect::<Vec<_>>();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = *POLYMARKET_VENUE;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            let response = DataResponse::Instruments(InstrumentsResponse::new(
                request_id,
                client_id,
                venue,
                instruments,
                start_nanos,
                end_nanos,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send instruments response: {e}");
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let http = self.provider.http_client().clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            let condition_id = match extract_condition_id(&instrument_id) {
                Ok(cid) => cid,
                Err(e) => {
                    log::error!("Failed to extract condition_id for {instrument_id}: {e}");
                    return;
                }
            };

            let query_params = GetGammaMarketsParams {
                condition_ids: Some(condition_id),
                ..Default::default()
            };

            let instrument = match http.request_instruments_by_params(query_params).await {
                Ok(instruments) => instruments.into_iter().find(|i| i.id() == instrument_id),
                Err(e) => {
                    log::error!("Failed to fetch instrument {instrument_id} from Gamma API: {e}");
                    return;
                }
            };

            if let Some(inst) = instrument {
                cache_instrument(&instruments_cache, &token_meta, &inst);

                // Publish onto the data bus so other clients (e.g. the exec
                // client's token map) can update from the same fetch.
                if let Err(e) = sender.send(DataEvent::Instrument(inst.clone())) {
                    log::warn!("Failed to publish instrument {instrument_id}: {e}");
                }

                let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                    request_id,
                    client_id,
                    instrument_id,
                    inst,
                    datetime_to_unix_nanos(start),
                    datetime_to_unix_nanos(end),
                    clock.get_time_ns(),
                    params,
                )));

                if let Err(e) = sender.send(DataEvent::Response(response)) {
                    log::error!("Failed to send instrument response: {e}");
                }
            } else {
                log::error!("Instrument {instrument_id} not found on Polymarket");
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;

        let token_id = instrument.raw_symbol().as_str().to_string();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let clob_client = self.clob_public_client.clone();
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match clob_client
                .request_book_snapshot(instrument_id, &token_id, price_precision, size_precision)
                .await
                .context("failed to request book snapshot from Polymarket")
            {
                Ok(book) => {
                    let response = DataResponse::Book(BookResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        book,
                        None,
                        None,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send book snapshot response: {e}");
                    }
                }
                Err(e) => log::error!("Book snapshot request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;

        let condition_id = extract_condition_id(&instrument_id)?;
        let token_id = instrument.raw_symbol().as_str().to_string();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let limit = request.limit.map(|n| n.get() as u32);

        let data_api_client = self.data_api_client.clone();
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);

        get_runtime().spawn(async move {
            match data_api_client
                .request_trade_ticks(
                    instrument_id,
                    &condition_id,
                    &token_id,
                    price_precision,
                    size_precision,
                    limit,
                )
                .await
                .context("failed to request trades from Polymarket Data API")
            {
                Ok(trades) => {
                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        trades,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send trades response: {e}");
                    }
                }
                Err(e) => log::error!("Trade request failed for {instrument_id}: {e:?}"),
            }
        });

        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("subscribe_instruments: subscribed individually via data subscription methods");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!(
                "Polymarket only supports L2_MBP order book deltas, received {:?}",
                cmd.book_type
            );
        }

        let instrument_id = cmd.instrument_id;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        // Mark intent before routing so unsubscribe can race-safely clear it.
        self.active_delta_subs.insert(instrument_id);
        self.order_books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        log::debug!("Subscribed to book deltas for {instrument_id}");
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        self.active_quote_subs.insert(instrument_id);

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        log::debug!("Subscribed to quotes for {instrument_id}");
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        self.active_trade_subs.insert(instrument_id);

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        log::debug!("Subscribed to trades for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_delta_subs.remove(&instrument_id);
        self.pending_snapshot_after_tick_change
            .remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.drop_local_book_state_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from book deltas for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_quote_subs.remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.drop_local_book_state_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from quotes for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_trade_subs.remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from trades for {instrument_id}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration as StdDuration};

    use axum::{
        Router,
        extract::{Path, State},
        http::StatusCode,
        response::Json,
        routing::get,
    };
    use nautilus_common::{
        live::runner::replace_data_event_sender,
        messages::{DataResponse, data::RequestCustomData},
        testing::wait_until_async,
    };
    use nautilus_core::{Params, UUID4, UnixNanos};
    use nautilus_model::{
        data::{CustomData as ModelCustomData, DataType},
        enums::{AssetClass, InstrumentCloseType, OrderSide, PositionSide},
        events::{PositionClosed, PositionEvent, PositionOpened},
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol,
            TraderId,
        },
        instruments::BinaryOption,
        types::{Currency, Money, Price, Quantity},
    };
    use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
    #[cfg(feature = "python")]
    use pyo3::types::PyAnyMethods;
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::{
        common::{consts::POLYMARKET_CLIENT_ID, enums::PolymarketOrderSide},
        config::PolymarketDataClientConfig,
        http::{clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient},
        websocket::{
            client::{PolymarketWebSocketClient, WsSubscriptionHandle},
            handler::HandlerCommand,
            messages::{
                MarketWsMessage, PolymarketBookLevel, PolymarketBookSnapshot,
                PolymarketMarketResolved, PolymarketQuote, PolymarketQuotes,
                PolymarketTickSizeChange,
            },
        },
    };

    fn make_handle() -> (
        WsSubscriptionHandle,
        tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        (WsSubscriptionHandle::from_sender(tx), rx)
    }

    type ActiveSet = Arc<AtomicSet<InstrumentId>>;
    type OpenTokens = Arc<AtomicSet<Ustr>>;
    type WsMutex = Arc<tokio::sync::Mutex<()>>;

    fn make_state() -> (ActiveSet, ActiveSet, ActiveSet, OpenTokens, WsMutex) {
        (
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(tokio::sync::Mutex::new(())),
        )
    }

    fn is_resolve_response(event: &DataEvent) -> bool {
        matches!(event, DataEvent::Response(DataResponse::Data(_)))
    }

    fn count_instrument_close_events(events: &[DataEvent]) -> usize {
        events
            .iter()
            .filter(|event| matches!(event, DataEvent::Data(NautilusData::InstrumentClose(_))))
            .count()
    }

    async fn collect_events_until<F>(
        data_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        timeout: StdDuration,
        mut done: F,
    ) -> Vec<DataEvent>
    where
        F: FnMut(&[DataEvent]) -> bool,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut events = Vec::new();

        loop {
            while let Ok(event) = data_rx.try_recv() {
                events.push(event);
            }

            if done(&events) || tokio::time::Instant::now() >= deadline {
                break;
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            let wait_for = remaining.min(StdDuration::from_millis(100));
            if let Ok(Some(event)) = tokio::time::timeout(wait_for, data_rx.recv()).await {
                events.push(event);
            }
        }

        events
    }

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET")
    }

    fn token_ustr() -> Ustr {
        Ustr::from("0xCOND-0xTOKEN")
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_subscribes_when_intent_present_and_ws_closed() {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        // Intent: quotes subscribed.
        let inst = instrument_id();
        quotes.insert(inst);

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes.clone(),
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(open.contains(&token_ustr()));

        match rx.try_recv().expect("expected SubscribeMarket command") {
            HandlerCommand::SubscribeMarket(ids) => {
                assert_eq!(ids, vec![inst.symbol.as_str().to_string()]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_unsubscribes_when_intent_absent_and_ws_open() {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        // WS currently open, but no caller wants it anymore.
        let inst = instrument_id();
        open.insert(token_ustr());

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(!open.contains(&token_ustr()));

        match rx.try_recv().expect("expected UnsubscribeMarket command") {
            HandlerCommand::UnsubscribeMarket(ids) => {
                assert_eq!(ids, vec![inst.symbol.as_str().to_string()]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[rstest]
    #[case::intent_matches_open(true, true, false)]
    #[case::no_intent_not_open(false, false, false)]
    #[tokio::test]
    async fn sync_ws_no_op_when_state_already_matches(
        #[case] want: bool,
        #[case] is_open_initial: bool,
        #[case] expect_command: bool,
    ) {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();

        if want {
            quotes.insert(inst);
        }

        if is_open_initial {
            open.insert(token_ustr());
        }

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        // State is preserved either way.
        assert_eq!(open.contains(&token_ustr()), is_open_initial);
        assert_eq!(rx.try_recv().is_ok(), expect_command);
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_rolls_back_open_tokens_on_send_failure() {
        // Drop the receiver so the channel send fails.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        drop(rx);
        let ws = WsSubscriptionHandle::from_sender(tx);

        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();
        quotes.insert(inst);

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        // Send failed, so the tracked WS state must be rolled back.
        assert!(!open.contains(&token_ustr()));
    }

    #[rstest]
    #[case::any_kind(true, false, false)]
    #[case::another_kind(false, true, false)]
    #[case::third_kind(false, false, true)]
    #[tokio::test]
    async fn sync_ws_opens_for_any_active_kind(#[case] q: bool, #[case] d: bool, #[case] t: bool) {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();

        if q {
            quotes.insert(inst);
        }

        if d {
            deltas.insert(inst);
        }

        if t {
            trades.insert(inst);
        }

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(open.contains(&token_ustr()));
        assert!(matches!(
            rx.try_recv(),
            Ok(HandlerCommand::SubscribeMarket(_))
        ));
    }

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

        cache_instrument(&instruments, &token_meta, &inst);

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
        cache_instrument(&instruments, &token_meta, &first);

        let second = stub_instrument(raw_symbol, Price::from("0.0001"), Quantity::from("0.001"));
        cache_instrument(&instruments, &token_meta, &second);

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
            cache_instrument(&instruments, &token_meta, inst);
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

    fn make_ws_ctx() -> (
        WsMessageContext,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let gamma_client = PolymarketGammaHttpClient::new(
            Some("http://localhost".to_string()),
            5,
            RetryConfig::default(),
        )
        .expect("gamma client");
        let clob_public_client =
            PolymarketClobPublicClient::new(Some("http://localhost".to_string()), 5)
                .expect("clob client");

        let ctx = WsMessageContext {
            clock: get_atomic_clock_realtime(),
            data_sender: data_tx,
            token_meta: Arc::new(DashMap::new()),
            instruments: Arc::new(AtomicMap::new()),
            gamma_client,
            clob_public_client,
            filters: vec![],
            order_books: Arc::new(DashMap::new()),
            last_quotes: Arc::new(DashMap::new()),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_delta_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            resolve_poll_watchlist: Arc::new(AtomicMap::new()),
            resolve_watch_apply_mutex: Arc::new(StdMutex::new(())),
            pending_snapshot_after_tick_change: Arc::new(AtomicSet::new()),
            subscribe_new_markets: false,
            new_market_filter: None,
            cancellation_token: CancellationToken::new(),
        };

        (ctx, data_rx)
    }

    fn seed_instrument(
        ctx: &WsMessageContext,
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
    ) -> InstrumentAny {
        let inst = stub_instrument(raw_symbol, price_increment, size_increment);
        cache_instrument(&ctx.instruments, &ctx.token_meta, &inst);
        inst
    }

    #[derive(Clone, Copy, Default)]
    struct SeedInstrumentContext<'a> {
        market_slug: Option<&'a str>,
        market_id: Option<&'a str>,
        condition_id: Option<&'a str>,
        expiration_ns: Option<UnixNanos>,
    }

    fn seed_instrument_with_context(
        ctx: &WsMessageContext,
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

        cache_instrument(&ctx.instruments, &ctx.token_meta, &inst);
        inst
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

    fn make_market_resolved(
        condition_id: &str,
        winner_asset_id: &str,
        loser_asset_id: &str,
    ) -> MarketWsMessage {
        MarketWsMessage::MarketResolved(PolymarketMarketResolved {
            id: "resolved-1".to_string(),
            market: Ustr::from(condition_id),
            assets_ids: vec![winner_asset_id.to_string(), loser_asset_id.to_string()],
            winning_asset_id: winner_asset_id.to_string(),
            winning_outcome: "Yes".to_string(),
            timestamp: "1700000004000".to_string(),
            tags: vec![],
        })
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

    fn make_clob_market_value(
        condition_id: &str,
        winner_token_id: &str,
        loser_token_id: &str,
        closed: bool,
    ) -> Value {
        serde_json::json!({
            "condition_id": condition_id,
            "closed": closed,
            "tokens": [
                {"token_id": winner_token_id, "outcome": "Yes", "winner": true},
                {"token_id": loser_token_id, "outcome": "No", "winner": false}
            ]
        })
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

    #[derive(Clone, Default)]
    struct TestServerState {
        gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
        clob_market_by_condition: Arc<tokio::sync::Mutex<ahash::AHashMap<String, Value>>>,
    }

    async fn handle_gamma_markets(State(state): State<TestServerState>) -> Json<Value> {
        let body = state
            .gamma_response
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| serde_json::json!([]));
        Json(body)
    }

    async fn handle_clob_market(
        State(state): State<TestServerState>,
        Path(condition_id): Path<String>,
    ) -> (StatusCode, Json<Value>) {
        let body = state.clob_market_by_condition.lock().await;
        if let Some(value) = body.get(&condition_id) {
            (StatusCode::OK, Json(value.clone()))
        } else {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"market not found"})),
            )
        }
    }

    async fn start_mock_server(state: TestServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let router = Router::new()
            .route("/markets", get(handle_gamma_markets))
            .route("/markets/{condition_id}", get(handle_clob_market))
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
        addr
    }

    fn create_test_client(
        addr: SocketAddr,
    ) -> (
        PolymarketDataClient,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(tx);

        let base_url = format!("http://{addr}");
        let gamma =
            PolymarketGammaHttpClient::new(Some(base_url.clone()), 5, RetryConfig::default())
                .expect("gamma client");
        let clob_public =
            PolymarketClobPublicClient::new(Some(base_url.clone()), 5).expect("clob client");
        let data_api =
            PolymarketDataApiHttpClient::new(Some(base_url.clone()), 5).expect("data api client");
        let ws = PolymarketWebSocketClient::new_market(
            Some(format!("ws://{addr}/ws/market")),
            false,
            TransportBackend::default(),
        );

        let config = PolymarketDataClientConfig {
            base_url_http: Some(base_url.clone()),
            base_url_ws: Some(format!("ws://{addr}/ws")),
            base_url_gamma: Some(base_url.clone()),
            base_url_data_api: Some(base_url),
            resolve_poll_enabled: false,
            ..PolymarketDataClientConfig::default()
        };

        let client = PolymarketDataClient::new(
            *POLYMARKET_CLIENT_ID,
            config,
            gamma,
            clob_public,
            data_api,
            ws,
        );

        (client, rx)
    }

    fn make_client_ws_ctx(client: &PolymarketDataClient) -> WsMessageContext {
        WsMessageContext {
            clock: client.clock,
            data_sender: client.data_sender.clone(),
            token_meta: client.token_meta.clone(),
            instruments: client.instruments.clone(),
            gamma_client: client.provider.http_client().clone(),
            clob_public_client: client.clob_public_client.clone(),
            filters: client.provider.filters(),
            order_books: client.order_books.clone(),
            last_quotes: client.last_quotes.clone(),
            active_quote_subs: client.active_quote_subs.clone(),
            active_delta_subs: client.active_delta_subs.clone(),
            active_trade_subs: client.active_trade_subs.clone(),
            resolve_poll_watchlist: client.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: client.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: client.pending_snapshot_after_tick_change.clone(),
            subscribe_new_markets: client.config.subscribe_new_markets,
            new_market_filter: client.config.new_market_filter.clone(),
            cancellation_token: client.cancellation_token.clone(),
        }
    }

    fn level(price: &str, size: &str) -> PolymarketBookLevel {
        PolymarketBookLevel {
            price: price.to_string(),
            size: size.to_string(),
        }
    }

    fn make_snapshot(market: &str, asset_id: &str, prices: &[(&str, &str)]) -> MarketWsMessage {
        let mid = prices.len() / 2;
        let bids = prices[..mid].iter().map(|(p, s)| level(p, s)).collect();
        let asks = prices[mid..].iter().map(|(p, s)| level(p, s)).collect();
        MarketWsMessage::Book(PolymarketBookSnapshot {
            market: Ustr::from(market),
            asset_id: Ustr::from(asset_id),
            bids,
            asks,
            timestamp: "1700000000000".to_string(),
        })
    }

    fn make_tick_change(market: &str, asset_id: &str, old: &str, new: &str) -> MarketWsMessage {
        MarketWsMessage::TickSizeChange(PolymarketTickSizeChange {
            market: Ustr::from(market),
            asset_id: Ustr::from(asset_id),
            new_tick_size: new.to_string(),
            old_tick_size: old.to_string(),
            timestamp: "1700000001000".to_string(),
        })
    }

    fn make_price_change(market: &str, asset_id: &str, price: &str, size: &str) -> MarketWsMessage {
        MarketWsMessage::PriceChange(PolymarketQuotes {
            market: Ustr::from(market),
            price_changes: vec![PolymarketQuote {
                asset_id: Ustr::from(asset_id),
                price: price.to_string(),
                side: PolymarketOrderSide::Buy,
                size: size.to_string(),
                hash: String::new(),
                best_bid: None,
                best_ask: None,
            }],
            timestamp: "1700000002000".to_string(),
        })
    }

    #[rstest]
    fn build_strict_resolved_market_requires_closed_and_binary_settlement_prices() {
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

        // Real Gamma/CLOB data contains some resolved markets with
        // `closed=true` and `acceptingOrders=true`; these are still resolvable.
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
        // Closed settled fixture should resolve under strict rules.
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

        // Active fixture must fail strict close rules.
        let active = load_gamma_market_fixture("gamma_market.json");
        assert!(build_strict_resolved_market(&active).is_none());
    }

    #[rstest]
    fn build_strict_resolved_market_real_gamma_samples_cover_resolution_buckets() {
        // Case A: closed + binary 1/0 + acceptingOrders=false => resolve.
        let closed_binary_accepting_false =
            load_gamma_market_fixture("gamma_market_closed_binary_accepting_false.json");
        let resolved = build_strict_resolved_market(&closed_binary_accepting_false)
            .expect("expected resolved market for binary accepting=false fixture");
        assert_eq!(
            resolved.condition_id,
            "0x8ccc3f4951ff02c1d34b87988752b4444ad17228732780a6cf22afefe8478bb6"
        );

        // Case B: closed + binary 1/0 + acceptingOrders=true => still resolve.
        let closed_binary_accepting_true =
            load_gamma_market_fixture("gamma_market_closed_binary_accepting_true.json");
        let resolved = build_strict_resolved_market(&closed_binary_accepting_true)
            .expect("expected resolved market for binary accepting=true fixture");
        assert_eq!(
            resolved.condition_id,
            "0xd57eed0d44f5b8ca54925d8d6ff440b146b3e6e071da18136ee3ee572d34479e"
        );

        // Case C: closed + 0/0 => skip (no winner).
        let closed_zero_zero =
            load_gamma_market_fixture("gamma_market_closed_zero_zero_legacy.json");
        assert!(build_strict_resolved_market(&closed_zero_zero).is_none());

        // Case D: closed + non-binary scalar-like distribution => skip.
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
        let (ctx, _data_rx) = make_ws_ctx();
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &ctx,
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
            &ctx,
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
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(yes.id()),
        );
        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(no.id()),
        );

        let watchlist = ctx.resolve_poll_watchlist.load();
        let entry = watchlist
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
        drop(watchlist);

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_closed_event(yes.id()),
        );
        let watchlist = ctx.resolve_poll_watchlist.load();
        let entry = watchlist
            .get("0xCOND-BTC")
            .expect("expected remaining condition entry");
        assert_eq!(entry.tracked.len(), 1);
        drop(watchlist);

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_closed_event(no.id()),
        );
        assert!(
            !ctx.resolve_poll_watchlist
                .contains_key(&"0xCOND-BTC".to_string())
        );
    }

    #[rstest]
    fn position_events_keep_token_watched_until_last_position_id_closes() {
        let (ctx, _data_rx) = make_ws_ctx();
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &ctx,
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
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event_with_position_id(yes.id(), "P-1"),
        );
        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event_with_position_id(yes.id(), "P-2"),
        );

        let watchlist = ctx.resolve_poll_watchlist.load();
        let entry = watchlist
            .get("0xCOND-BTC")
            .expect("expected watch entry for condition");
        let yes_tracked = entry
            .tracked
            .get("0xTOKEN_YES")
            .expect("expected tracked yes token");
        assert_eq!(yes_tracked.open_position_ids.len(), 2);
        drop(watchlist);

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_closed_event_with_position_id(yes.id(), "P-1"),
        );

        let watchlist = ctx.resolve_poll_watchlist.load();
        let entry = watchlist
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
        drop(watchlist);

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_closed_event_with_position_id(yes.id(), "P-2"),
        );

        assert!(
            !ctx.resolve_poll_watchlist
                .contains_key(&"0xCOND-BTC".to_string())
        );
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
    fn market_resolved_emits_grouped_close_and_removes_watch_entry() {
        let (ctx, mut data_rx) = make_ws_ctx();
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &ctx,
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
            &ctx,
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
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(yes.id()),
        );
        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(no.id()),
        );

        PolymarketDataClient::handle_market_message(
            make_market_resolved("0xCOND-BTC", "0xTOKEN_YES", "0xTOKEN_NO"),
            &ctx,
        );

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        let statuses = events
            .iter()
            .filter(|event| matches!(event, DataEvent::InstrumentStatus(_)))
            .count();
        assert_eq!(statuses, 2);

        let mut yes_close = None;
        let mut no_close = None;

        for event in events {
            if let DataEvent::Data(NautilusData::InstrumentClose(close)) = event {
                if close.instrument_id == yes.id() {
                    yes_close = Some(close);
                } else if close.instrument_id == no.id() {
                    no_close = Some(close);
                }
            }
        }

        let yes_close = yes_close.expect("expected yes close");
        let no_close = no_close.expect("expected no close");
        assert_eq!(yes_close.close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(no_close.close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(
            yes_close.close_price.as_decimal(),
            rust_decimal::Decimal::ONE
        );
        assert_eq!(
            no_close.close_price.as_decimal(),
            rust_decimal::Decimal::ZERO
        );
        assert!(
            !ctx.resolve_poll_watchlist
                .contains_key(&"0xCOND-BTC".to_string())
        );
    }

    #[rstest]
    fn duplicate_market_resolved_after_watch_removal_is_a_noop() {
        let (ctx, mut data_rx) = make_ws_ctx();
        let yes = seed_instrument_with_context(
            &ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-BTC"),
                expiration_ns: Some(UnixNanos::from(1_000_000_000)),
                ..SeedInstrumentContext::default()
            },
        );

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(yes.id()),
        );

        let resolved = make_market_resolved("0xCOND-BTC", "0xTOKEN_YES", "0xTOKEN_NO");
        PolymarketDataClient::handle_market_message(resolved.clone(), &ctx);
        let _ = std::iter::from_fn(|| data_rx.try_recv().ok()).collect::<Vec<_>>();

        PolymarketDataClient::handle_market_message(resolved, &ctx);
        assert!(data_rx.try_recv().is_err());
    }

    #[rstest]
    fn market_resolved_emit_failure_merges_watch_entry_back() {
        let (ctx, data_rx) = make_ws_ctx();
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &ctx,
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
            &ctx,
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
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(yes.id()),
        );
        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_opened_event(no.id()),
        );

        drop(data_rx);

        PolymarketDataClient::handle_market_message(
            make_market_resolved("0xCOND-BTC", "0xTOKEN_YES", "0xTOKEN_NO"),
            &ctx,
        );

        let watchlist = ctx.resolve_poll_watchlist.load();
        let entry = watchlist
            .get("0xCOND-BTC")
            .expect("expected watch entry restored after emit failure");
        assert_eq!(entry.tracked.len(), 2);
    }

    #[rstest]
    fn merge_resolve_watch_entry_unions_existing_token_state() {
        let (ctx, _data_rx) = make_ws_ctx();
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
        ctx.resolve_poll_watchlist.insert(
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

        PolymarketDataClient::merge_resolve_watch_entry(
            &ctx,
            ResolveWatchEntry {
                condition_id: condition_id.clone(),
                expiration_ns: UnixNanos::from(2_000),
                tracked: incoming_tracked,
                paused: true,
            },
        );

        let watchlist = ctx.resolve_poll_watchlist.load();
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

    #[rstest]
    #[tokio::test]
    async fn request_data_manual_fallback_resolves_paused_entries() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-REQ",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(1_000_000_000);
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );

        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );
        pause_resolve_watch_entries(&client.resolve_poll_watchlist, &["0xCOND-REQ".to_string()]);

        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );
        client.request_data(request).expect("request_data");

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-REQ".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 2
        })
        .await;

        assert!(
            events.iter().any(is_resolve_response),
            "expected custom data response, received: {events:?}"
        );
        let response = events
            .iter()
            .find_map(|event| match event {
                DataEvent::Response(DataResponse::Data(response)) => Some(response),
                _ => None,
            })
            .expect("expected custom data response");
        let custom = response
            .data
            .as_ref()
            .downcast_ref::<ModelCustomData>()
            .expect("expected CustomData response payload");
        assert_eq!(custom.data_type.type_name(), RESOLVE_REQUEST_TYPE_NAME);
        let summary = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketResolveRequestSummaryData>()
            .expect("expected resolve summary payload");
        assert_eq!(
            summary.emitted_condition_ids,
            vec!["0xCOND-REQ".to_string()]
        );
        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 2);
    }

    #[rstest]
    #[tokio::test]
    async fn request_data_manual_fallback_with_auto_poll_disabled_resolves_expired_entries() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-REQ",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(60_000_000_000),
        );
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );

        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );

        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );
        client.request_data(request).expect("request_data");

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-REQ".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 2
        })
        .await;

        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 2);
    }

    #[rstest]
    #[tokio::test]
    async fn request_data_manual_fallback_uses_clob_when_gamma_is_not_strict() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-REQ",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                Some("[\"0.58\",\"0.42\"]"),
                Some(true),
                Some(false),
            )
        ]));
        state.clob_market_by_condition.lock().await.insert(
            "0xCOND-REQ".to_string(),
            make_clob_market_value("0xCOND-REQ", "0xTOKEN_YES", "0xTOKEN_NO", true),
        );

        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(60_000_000_000),
        );
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );

        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );

        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );
        client.request_data(request).expect("request_data");

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-REQ".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 2
        })
        .await;

        let response = events
            .iter()
            .find_map(|event| match event {
                DataEvent::Response(DataResponse::Data(response)) => Some(response),
                _ => None,
            })
            .expect("expected custom data response");
        let custom = response
            .data
            .as_ref()
            .downcast_ref::<ModelCustomData>()
            .expect("expected CustomData response payload");
        let summary = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketResolveRequestSummaryData>()
            .expect("expected resolve summary payload");
        assert_eq!(summary.resolved_markets, 1);
        assert_eq!(
            summary.emitted_condition_ids,
            vec!["0xCOND-REQ".to_string()]
        );

        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 2);
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_fallback_clob_success_after_gamma_error_does_not_mark_failed() {
        let state = TestServerState::default();
        state.clob_market_by_condition.lock().await.insert(
            "0xCOND-REQ".to_string(),
            make_clob_market_value("0xCOND-REQ", "0xTOKEN_YES", "0xTOKEN_NO", true),
        );

        let addr = start_mock_server(state).await;
        let (client, _data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(60_000_000_000),
        );
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );

        let failing_gamma = PolymarketGammaHttpClient::new(
            Some("http://127.0.0.1:1".to_string()),
            1,
            RetryConfig {
                max_retries: 0,
                initial_delay_ms: 1,
                max_delay_ms: 1,
                backoff_factor: 1.0,
                jitter_ms: 0,
                operation_timeout_ms: Some(200),
                immediate_first: true,
                max_elapsed_ms: Some(200),
            },
        )
        .expect("gamma client");

        let stats = PolymarketDataClient::fetch_and_apply_resolutions_by_condition_ids(
            &failing_gamma,
            &client.clob_public_client,
            &ws_ctx,
            &["0xCOND-REQ".to_string()],
            ResolveBatchErrorMode::StopOnFirstError,
        )
        .await;

        assert_eq!(stats.resolved_markets, 1);
        assert_eq!(stats.emitted_condition_ids, vec!["0xCOND-REQ".to_string()]);
        assert!(stats.failed_condition_ids.is_empty());
        assert_eq!(stats.error, None);
    }

    #[rstest]
    #[tokio::test]
    async fn request_data_explicit_multiple_condition_ids_resolves_all_requested_conditions() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-A",
                "[\"0xA_YES\",\"0xA_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            ),
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-B",
                "[\"0xB_YES\",\"0xB_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(1_000_000_000);
        let instruments = [
            seed_instrument_with_context(
                &ws_ctx,
                "0xA_YES",
                Price::from("0.001"),
                Quantity::from("0.01"),
                SeedInstrumentContext {
                    condition_id: Some("0xCOND-A"),
                    expiration_ns: Some(expiration_ns),
                    ..SeedInstrumentContext::default()
                },
            ),
            seed_instrument_with_context(
                &ws_ctx,
                "0xA_NO",
                Price::from("0.001"),
                Quantity::from("0.01"),
                SeedInstrumentContext {
                    condition_id: Some("0xCOND-A"),
                    expiration_ns: Some(expiration_ns),
                    ..SeedInstrumentContext::default()
                },
            ),
            seed_instrument_with_context(
                &ws_ctx,
                "0xB_YES",
                Price::from("0.001"),
                Quantity::from("0.01"),
                SeedInstrumentContext {
                    condition_id: Some("0xCOND-B"),
                    expiration_ns: Some(expiration_ns),
                    ..SeedInstrumentContext::default()
                },
            ),
            seed_instrument_with_context(
                &ws_ctx,
                "0xB_NO",
                Price::from("0.001"),
                Quantity::from("0.01"),
                SeedInstrumentContext {
                    condition_id: Some("0xCOND-B"),
                    expiration_ns: Some(expiration_ns),
                    ..SeedInstrumentContext::default()
                },
            ),
        ];

        for instrument in &instruments {
            upsert_resolve_watch_entry_from_instrument(
                &client.resolve_poll_watchlist,
                instrument,
                PositionId::new("P-1"),
            );
        }

        let mut params = Params::new();
        params.insert(
            "condition_ids".to_string(),
            serde_json::json!(["0xCOND-A", "0xCOND-B"]),
        );
        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            Some(params),
        );
        client.request_data(request).expect("request_data");

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-A".to_string())
                    && !client
                        .resolve_poll_watchlist
                        .contains_key(&"0xCOND-B".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 4
        })
        .await;

        let response = events
            .iter()
            .find_map(|event| match event {
                DataEvent::Response(DataResponse::Data(response)) => Some(response),
                _ => None,
            })
            .expect("expected custom data response");
        let custom = response
            .data
            .as_ref()
            .downcast_ref::<ModelCustomData>()
            .expect("expected CustomData response payload");
        let summary = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketResolveRequestSummaryData>()
            .expect("expected resolve summary payload");
        assert_eq!(
            summary.requested_condition_ids,
            vec!["0xCOND-A".to_string(), "0xCOND-B".to_string()]
        );
        assert_eq!(summary.resolved_markets, 2);
        assert_eq!(
            summary.emitted_condition_ids,
            vec!["0xCOND-A".to_string(), "0xCOND-B".to_string()]
        );

        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 4);
    }

    #[rstest]
    #[tokio::test]
    async fn request_data_explicit_invalid_selector_does_not_fallback_to_watchlist() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-REQ",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
        let (client, mut data_rx) = create_test_client(addr);
        let ws_ctx = make_client_ws_ctx(&client);

        let expiration_ns = UnixNanos::from(1_000_000_000);
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-REQ"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );
        pause_resolve_watch_entries(&client.resolve_poll_watchlist, &["0xCOND-REQ".to_string()]);

        let mut params = Params::new();
        params.insert(
            "instrument_ids".to_string(),
            serde_json::json!(["BTCUSDT-PERP.BINANCE"]),
        );
        let request = RequestCustomData::new(
            ClientId::from("POLYMARKET"),
            DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            Some(params),
        );
        client.request_data(request).expect("request_data");

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
            events.iter().any(is_resolve_response)
        })
        .await;

        let response = events
            .iter()
            .find_map(|event| match event {
                DataEvent::Response(DataResponse::Data(response)) => Some(response),
                _ => None,
            })
            .expect("expected custom data response");
        let custom = response
            .data
            .as_ref()
            .downcast_ref::<ModelCustomData>()
            .expect("expected CustomData response payload");
        let summary = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketResolveRequestSummaryData>()
            .expect("expected resolve summary payload");
        assert!(!summary.used_watchlist_fallback);
        assert_eq!(summary.requested_condition_ids, Vec::<String>::new());
        assert!(summary.error.is_some());

        let closes = count_instrument_close_events(&events);
        assert_eq!(closes, 0);
        assert!(
            client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-REQ".to_string())
        );
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_poll_task_emits_grouped_close_for_expired_watch_entries() {
        let state = TestServerState::default();
        *state.gamma_response.lock().await = Some(serde_json::json!([
            make_gamma_market_value_with_outcome_prices(
                "0xCOND-POLL",
                "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
                Some("[\"1\",\"0\"]"),
                Some(true),
                Some(false),
            )
        ]));
        let addr = start_mock_server(state).await;
        let (mut client, mut data_rx) = create_test_client(addr);
        client.config.resolve_poll_enabled = true;
        client.config.resolve_poll_interval_secs = 1;
        client.config.resolve_poll_grace_secs = 0;
        client.config.resolve_poll_max_wait_secs = 300;

        let ws_ctx = make_client_ws_ctx(&client);
        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(1_000_000_000),
        );
        let inst_yes = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-POLL"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        let inst_no = seed_instrument_with_context(
            &ws_ctx,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-POLL"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_yes,
            PositionId::new("P-1"),
        );
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst_no,
            PositionId::new("P-2"),
        );

        client.spawn_resolve_poll_task();

        wait_until_async(
            || async {
                !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-POLL".to_string())
            },
            StdDuration::from_secs(5),
        )
        .await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await;

        let events = collect_events_until(&mut data_rx, StdDuration::from_secs(1), |events| {
            count_instrument_close_events(events) >= 2
        })
        .await;
        let closes = count_instrument_close_events(&events);

        assert_eq!(closes, 2);
        assert!(
            !client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-POLL".to_string())
        );
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn resolve_request_summary_to_pyobject_returns_dict_payload() {
        let summary = ResolveRequestSummary {
            requested_condition_ids: vec!["0xCOND-A".to_string()],
            fetched_markets: 1,
            resolved_markets: 1,
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
                .expect("expected requested_condition_ids")
                .expect("requested_condition_ids missing");
            let requested_vec: Vec<String> = requested
                .extract()
                .expect("expected requested_condition_ids as list[str]");
            assert_eq!(requested_vec, vec!["0xCOND-A".to_string()]);

            let resolved = dict
                .get_item("resolved_markets")
                .expect("expected resolved_markets")
                .expect("resolved_markets missing");
            let resolved_count: usize = resolved
                .extract()
                .expect("expected resolved_markets as integer");
            assert_eq!(resolved_count, 1);
        });
    }

    #[rstest]
    fn tick_size_change_clears_book_and_marks_pending() {
        // Coarsens 0.001 -> 0.01. last_quote is preserved (carried forward by
        // parse_quote_from_price_change). raw_symbol == asset_id because
        // token_meta is keyed on raw_symbol.
        let asset_id_str = "0xTOKEN";
        let token_ustr = Ustr::from(asset_id_str);
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);

        // Seed last_quote so we can assert it survives the tick-size change.
        let prior_quote = QuoteTick::new(
            instrument_id,
            Price::from("0.504"),
            Price::from("0.506"),
            Quantity::from("5.00"),
            Quantity::from("8.00"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        ctx.last_quotes.insert(instrument_id, prior_quote);

        let snap = make_snapshot(
            market,
            asset_id_str,
            &[
                ("0.501", "10"),
                ("0.504", "5"),
                ("0.506", "8"),
                ("0.509", "12"),
            ],
        );
        PolymarketDataClient::handle_market_message(snap, &ctx);
        assert!(ctx.order_books.contains_key(&instrument_id));
        // Drain the snapshot DataEvents we just produced so the assertion
        // below only sees what the tick-size change emits.
        while data_rx.try_recv().is_ok() {}

        let change = make_tick_change(market, asset_id_str, "0.001", "0.01");
        PolymarketDataClient::handle_market_message(change, &ctx);

        assert!(!ctx.order_books.contains_key(&instrument_id));
        // last_quote is intentionally preserved across the epoch.
        assert!(ctx.last_quotes.contains_key(&instrument_id));
        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );

        let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
        assert_eq!(meta.price_precision, 2);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
            "expected rebuilt instrument event, found: {events:?}",
        );
        assert!(
            !events.iter().any(|e| matches!(e, DataEvent::Data(_))),
            "tick size change must not emit Data events: {events:?}",
        );
    }

    #[rstest]
    fn pending_drops_price_change_until_snapshot() {
        // Acceptance criterion 4: a price_change arriving while pending
        // must be dropped; the next snapshot reseeds the book and clears
        // the pending flag.
        let asset_id_str = "0xTOKEN2";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);

        let pc = make_price_change(market, asset_id_str, "0.50", "20");
        PolymarketDataClient::handle_market_message(pc, &ctx);

        assert!(!ctx.order_books.contains_key(&instrument_id));
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.is_empty(),
            "price_change while pending must not emit any events: {events:?}",
        );

        let snap = make_snapshot(
            market,
            asset_id_str,
            &[("0.45", "5"), ("0.49", "10"), ("0.51", "8"), ("0.55", "12")],
        );
        PolymarketDataClient::handle_market_message(snap, &ctx);

        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(ctx.order_books.contains_key(&instrument_id));
    }

    #[rstest]
    fn tick_size_change_noop_preserves_book_and_quote() {
        // Same tick_size on both sides must be ignored, not treated as an epoch.
        let asset_id_str = "0xTOKEN_NOOP";
        let token_ustr = Ustr::from(asset_id_str);
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);

        let snap = make_snapshot(
            market,
            asset_id_str,
            &[("0.50", "10"), ("0.54", "5"), ("0.56", "8"), ("0.59", "12")],
        );
        PolymarketDataClient::handle_market_message(snap, &ctx);
        let book_ts_before = ctx
            .order_books
            .get(&instrument_id)
            .expect("book entry")
            .ts_last;

        while data_rx.try_recv().is_ok() {}

        let change = make_tick_change(market, asset_id_str, "0.01", "0.01");
        PolymarketDataClient::handle_market_message(change, &ctx);

        let book_after = ctx.order_books.get(&instrument_id).expect("book entry");
        assert_eq!(book_after.ts_last, book_ts_before);
        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
        assert_eq!(meta.price_precision, 2);
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.is_empty(),
            "no-op tick change must not emit events: {events:?}",
        );
    }

    #[rstest]
    fn tick_size_change_same_precision_different_value_triggers_epoch() {
        // Regression lock: a precision-only no-op check would skip 0.005 -> 0.001
        // (both precision 3) even though the tick value really changed.
        let asset_id_str = "0xTOKEN_VALUE";
        let token_ustr = Ustr::from(asset_id_str);
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.005"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );

        let change = make_tick_change(market, asset_id_str, "0.005", "0.001");
        PolymarketDataClient::handle_market_message(change, &ctx);

        assert!(!ctx.order_books.contains_key(&instrument_id));
        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
        assert_eq!(meta.price_precision, 3);

        let rebuilt = ctx
            .instruments
            .load()
            .get(&instrument_id)
            .cloned()
            .expect("rebuilt instrument");
        assert_eq!(rebuilt.price_increment(), Price::from("0.001"));

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
            "expected rebuilt instrument event, found: {events:?}",
        );
    }

    #[rstest]
    fn tick_size_change_does_not_mark_pending_for_trade_only_sub() {
        // Trade-only subs don't read the book; pending would be dead state.
        let asset_id_str = "0xTOKEN6";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_trade_subs.insert(instrument_id);

        let change = make_tick_change(market, asset_id_str, "0.001", "0.01");
        PolymarketDataClient::handle_market_message(change, &ctx);

        assert!(
            !ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
            "instrument update must still be emitted: {events:?}",
        );
    }

    #[rstest]
    fn pending_persists_when_snapshot_has_corrupt_level() {
        // parse_book_snapshot must fail on a malformed mid-book level; pending
        // stays set even though parse_quote_from_snapshot succeeds on the top.
        let asset_id_str = "0xTOKEN7";

        let (ctx, _data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.active_quote_subs.insert(instrument_id);
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);

        let snap = MarketWsMessage::Book(PolymarketBookSnapshot {
            market: Ustr::from("0xMARKET"),
            asset_id: Ustr::from(asset_id_str),
            bids: vec![level("not-a-number", "1"), level("0.49", "10")],
            asks: vec![level("0.51", "8"), level("0.55", "12")],
            timestamp: "1700000000000".to_string(),
        });
        PolymarketDataClient::handle_market_message(snap, &ctx);

        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(!ctx.order_books.contains_key(&instrument_id));
    }

    #[rstest]
    fn price_change_emits_delta_when_not_pending() {
        // Positive complement to pending_drops_price_change_until_snapshot.
        let asset_id_str = "0xTOKEN10";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );

        let pc = make_price_change(market, asset_id_str, "0.50", "20");
        PolymarketDataClient::handle_market_message(pc, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DataEvent::Data(NautilusData::Deltas(_)))),
            "delta must be emitted on the not-pending happy path: {events:?}",
        );

        let book = ctx.order_books.get(&instrument_id).expect("book entry");
        assert_eq!(book.best_bid_price(), Some(Price::from("0.50")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("20.00")));
    }

    #[rstest]
    fn quote_path_open_during_pending_window() {
        // Only the delta arm is gated. Unchanged ask_size carries forward
        // from the preserved last_quote rather than defaulting to zero.
        let asset_id_str = "0xTOKEN8";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.active_quote_subs.insert(instrument_id);
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);

        // Sizes must use the instrument's size_precision; QuoteTick::new_checked
        // rejects cross-precision construction.
        let prior = QuoteTick::new(
            instrument_id,
            Price::from("0.49"),
            Price::from("0.51"),
            Quantity::from("100.00"),
            Quantity::from("75.00"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        ctx.last_quotes.insert(instrument_id, prior);

        let pc = MarketWsMessage::PriceChange(PolymarketQuotes {
            market: Ustr::from(market),
            price_changes: vec![PolymarketQuote {
                asset_id: Ustr::from(asset_id_str),
                price: "0.50".to_string(),
                side: PolymarketOrderSide::Buy,
                size: "20".to_string(),
                hash: String::new(),
                best_bid: Some("0.50".to_string()),
                best_ask: Some("0.52".to_string()),
            }],
            timestamp: "1700000003000".to_string(),
        });
        PolymarketDataClient::handle_market_message(pc, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DataEvent::Data(NautilusData::Deltas(_)))),
            "delta must be dropped while pending: {events:?}",
        );
        let emitted_quote = events
            .iter()
            .find_map(|e| match e {
                DataEvent::Data(NautilusData::Quote(q)) => Some(q),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected quote event, found: {events:?}"));
        assert_eq!(emitted_quote.bid_size, Quantity::from("20.00"));
        // ask_size carried forward from the prior quote, not defaulted to zero.
        assert_eq!(emitted_quote.ask_size, Quantity::from("75.00"));
    }

    #[rstest]
    fn pending_persists_when_snapshot_fails_to_seed() {
        // An empty snapshot must leave pending in place.
        let asset_id_str = "0xTOKEN5";
        let market = "0xMARKET";

        let (ctx, mut data_rx) = make_ws_ctx();
        let inst = seed_instrument(
            &ctx,
            asset_id_str,
            Price::from("0.01"),
            Quantity::from("0.01"),
        );
        let instrument_id = inst.id();
        ctx.active_delta_subs.insert(instrument_id);
        ctx.pending_snapshot_after_tick_change.insert(instrument_id);

        let empty = MarketWsMessage::Book(PolymarketBookSnapshot {
            market: Ustr::from(market),
            asset_id: Ustr::from(asset_id_str),
            bids: vec![],
            asks: vec![],
            timestamp: "1700000000000".to_string(),
        });
        PolymarketDataClient::handle_market_message(empty, &ctx);

        assert!(
            ctx.pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        assert!(
            !events.iter().any(|e| matches!(e, DataEvent::Data(_))),
            "empty snapshot must not emit Data events: {events:?}",
        );
    }
}
