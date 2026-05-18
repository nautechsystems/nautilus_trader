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

use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, Ordering},
};

use ahash::{AHashMap, AHashSet};
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
        Data as NautilusData, InstrumentClose, InstrumentStatus, OrderBookDeltas_API, QuoteTick,
    },
    enums::{BookType, InstrumentCloseType, MarketStatusAction},
    events::PositionEvent,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::Price,
};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::consts::{GAMMA_CONDITION_IDS_BATCH_SIZE, POLYMARKET_VENUE},
    config::PolymarketDataClientConfig,
    filters::InstrumentFilter,
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient, models::GammaMarket,
        parse::rebuild_instrument_with_tick_size, query::GetGammaMarketsParams,
    },
    providers::{PolymarketInstrumentProvider, extract_condition_id, fetch_instruments},
    websocket::{
        client::PolymarketWebSocketClient,
        messages::{MarketWsMessage, PolymarketQuotes, PolymarketWsMessage},
        parse::{
            parse_book_deltas, parse_book_snapshot, parse_quote_from_price_change,
            parse_quote_from_snapshot, parse_timestamp_ms, parse_trade_tick,
        },
    },
};

const RESOLVE_TRIGGER_TYPE_NAME: &str = "PolymarketResolveRequest";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ResolveRequestSummary {
    requested_condition_ids: Vec<String>,
    used_watchlist_fallback: bool,
    fetched_markets: usize,
    resolved_candidates: usize,
    compensation_emitted: usize,
    timed_out_watchlist: usize,
    error: Option<String>,
}
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

#[derive(Clone, Debug)]
struct ResolveWatchEntry {
    condition_id: Option<String>,
    expiration_ns: Option<UnixNanos>,
    auto_poll_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolveWatchSelectionMode {
    AutoPoll,
    ManualFallback,
}

#[derive(Debug, Default)]
struct ResolveWatchSelection {
    condition_ids: Vec<String>,
    skipped_not_expired: usize,
    missing_expiration: usize,
    timed_out_watchlist: usize,
    paused_watchlist: usize,
    min_ready_in_secs: Option<u64>,
    not_ready_samples: Vec<String>,
    pause_auto_poll: Vec<InstrumentId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvePollSkipLogKey {
    tracked: usize,
    skipped_not_expired: usize,
    missing_expiration: usize,
    timed_out_watchlist: usize,
    paused_watchlist: usize,
    min_ready_bucket_mins: Option<u64>,
}

type ResolvedTarget = (
    InstrumentId,
    Ustr,
    u8,
    Option<String>,
    Option<String>,
    Option<String>,
    &'static str,
);

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

fn instrument_market_context(
    instrument: &InstrumentAny,
) -> (Option<String>, Option<String>, Option<String>) {
    match instrument {
        InstrumentAny::BinaryOption(bo) => {
            let slug = bo
                .info
                .as_ref()
                .and_then(|info| info.get_str("market_slug"))
                .map(ToString::to_string);
            let market_id = bo
                .info
                .as_ref()
                .and_then(|info| info.get_str("market_id"))
                .map(ToString::to_string);
            let condition_id = bo
                .info
                .as_ref()
                .and_then(|info| info.get_str("condition_id"))
                .map(ToString::to_string);
            (slug, market_id, condition_id)
        }
        _ => (None, None, None),
    }
}

fn build_resolve_watch_entry(instrument: &InstrumentAny) -> ResolveWatchEntry {
    let instrument_id = instrument.id();
    let (_, _, condition_id) = instrument_market_context(instrument);

    ResolveWatchEntry {
        condition_id: condition_id.or_else(|| extract_condition_id(&instrument_id).ok()),
        expiration_ns: instrument.expiration_ns(),
        auto_poll_active: true,
    }
}

fn should_track_resolve_candidate(instrument: &InstrumentAny) -> bool {
    matches!(instrument, InstrumentAny::BinaryOption(_)) && instrument.expiration_ns().is_some()
}

fn upsert_resolve_watch_entry_from_instrument(
    watchlist: &Arc<AtomicMap<InstrumentId, ResolveWatchEntry>>,
    instrument: &InstrumentAny,
) {
    if !should_track_resolve_candidate(instrument) {
        return;
    }

    let instrument_id = instrument.id();
    let existing = watchlist.load();
    let mut entry = build_resolve_watch_entry(instrument);
    if let Some(existing_entry) = existing.get(&instrument_id) {
        entry.auto_poll_active = existing_entry.auto_poll_active;
    }
    drop(existing);

    watchlist.insert(instrument_id, entry);
}

fn update_resolve_watchlist_from_position_event(
    watchlist: &Arc<AtomicMap<InstrumentId, ResolveWatchEntry>>,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    event: &PositionEvent,
) {
    let instrument_id = event.instrument_id();
    if instrument_id.venue != *POLYMARKET_VENUE {
        return;
    }

    match event {
        PositionEvent::PositionClosed(_) => {
            watchlist.remove(&instrument_id);
        }
        PositionEvent::PositionOpened(_)
        | PositionEvent::PositionChanged(_)
        | PositionEvent::PositionAdjusted(_) => {
            let loaded = instruments.load();
            let Some(instrument) = loaded.get(&instrument_id) else {
                return;
            };
            upsert_resolve_watch_entry_from_instrument(watchlist, instrument);
        }
    }
}

fn collect_resolve_watch_selection(
    watchlist: &AHashMap<InstrumentId, ResolveWatchEntry>,
    now_ns: UnixNanos,
    expiry_grace_secs: u64,
    max_wait_secs: u64,
    mode: ResolveWatchSelectionMode,
) -> ResolveWatchSelection {
    let mut selection = ResolveWatchSelection::default();
    let mut seen_conditions = AHashSet::new();
    let grace_ns = expiry_grace_secs.saturating_mul(1_000_000_000);
    let max_wait_ns = max_wait_secs.saturating_mul(1_000_000_000);

    for (instrument_id, entry) in watchlist {
        let Some(expiration_ns) = entry.expiration_ns else {
            selection.missing_expiration += 1;
            continue;
        };

        let ready_at_ns = expiration_ns.as_u64().saturating_add(grace_ns);
        if now_ns.as_u64() < ready_at_ns {
            selection.skipped_not_expired += 1;
            let ready_in_secs = (ready_at_ns - now_ns.as_u64()) / 1_000_000_000;
            selection.min_ready_in_secs = Some(
                selection
                    .min_ready_in_secs
                    .map_or(ready_in_secs, |current| current.min(ready_in_secs)),
            );

            if selection.not_ready_samples.len() < 8 {
                selection.not_ready_samples.push(format!(
                    "{}:exp_ns={} ready_in_secs={}",
                    instrument_id,
                    expiration_ns.as_u64(),
                    ready_in_secs
                ));
            }
            continue;
        }

        let max_wait_reached =
            now_ns.as_u64() >= expiration_ns.as_u64().saturating_add(max_wait_ns);

        if max_wait_reached {
            selection.timed_out_watchlist += 1;
            if entry.auto_poll_active {
                selection.pause_auto_poll.push(*instrument_id);
            } else {
                selection.paused_watchlist += 1;
            }

            if mode == ResolveWatchSelectionMode::AutoPoll {
                continue;
            }
        } else if mode == ResolveWatchSelectionMode::AutoPoll && !entry.auto_poll_active {
            selection.paused_watchlist += 1;
            continue;
        }

        let condition_id = entry
            .condition_id
            .clone()
            .or_else(|| extract_condition_id(instrument_id).ok());

        if let Some(condition_id) = condition_id
            && seen_conditions.insert(condition_id.clone())
        {
            selection.condition_ids.push(condition_id);
        }
    }

    selection
}

fn pause_resolve_watch_entries(
    watchlist: &Arc<AtomicMap<InstrumentId, ResolveWatchEntry>>,
    instrument_ids: &[InstrumentId],
) {
    if instrument_ids.is_empty() {
        return;
    }

    for instrument_id in instrument_ids {
        let loaded = watchlist.load();
        let Some(entry) = loaded.get(instrument_id).cloned() else {
            continue;
        };
        drop(loaded);

        if !entry.auto_poll_active {
            continue;
        }

        let mut updated = entry;
        updated.auto_poll_active = false;
        watchlist.insert(*instrument_id, updated);
    }
}

fn parse_json_string_array(raw: &str) -> Option<Vec<String>> {
    serde_json::from_str::<Vec<String>>(raw)
        .ok()
        .filter(|values| !values.is_empty())
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
        let parsed = value.parse::<f64>().ok()?;
        values.push(parsed);
    }

    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn winning_index_from_outcome_prices(prices: &[f64]) -> Option<usize> {
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

fn winning_index_from_outcome_prices_soft(
    prices: &[f64],
    min_winner_prob: f64,
    max_loser_prob: f64,
) -> Option<usize> {
    if prices.is_empty() {
        return None;
    }

    let mut best_idx = 0usize;
    let mut best_val = prices[0];

    for (idx, value) in prices.iter().copied().enumerate().skip(1) {
        if value > best_val {
            best_val = value;
            best_idx = idx;
        }
    }

    if best_val < min_winner_prob {
        return None;
    }
    let second = prices
        .iter()
        .enumerate()
        .filter_map(|(idx, v)| (idx != best_idx).then_some(*v))
        .fold(0.0_f64, f64::max);

    if second > max_loser_prob {
        return None;
    }

    Some(best_idx)
}

fn build_market_resolved_from_gamma(
    market: &GammaMarket,
    ts_init: UnixNanos,
) -> Option<crate::websocket::messages::PolymarketMarketResolved> {
    let assets_ids = parse_json_string_array(&market.clob_token_ids)?;
    let outcomes = parse_json_string_array(&market.outcomes).unwrap_or_default();
    let prices = parse_outcome_prices(&market.outcome_prices)?;
    let winner_idx = winning_index_from_outcome_prices(&prices)?;
    let winning_asset_id = assets_ids.get(winner_idx)?.clone();
    let winning_outcome = outcomes
        .get(winner_idx)
        .cloned()
        .unwrap_or_else(|| "UNKNOWN".to_string());

    Some(crate::websocket::messages::PolymarketMarketResolved {
        id: format!("gamma-resolve-{}", market.id),
        slug: market.market_slug.clone(),
        market: Ustr::from(market.condition_id.as_str()),
        assets_ids,
        winning_asset_id,
        winning_outcome,
        timestamp: (ts_init.as_u64() / 1_000_000).to_string(),
        tags: Vec::new(),
    })
}

fn build_market_resolved_from_gamma_soft(
    market: &GammaMarket,
    ts_init: UnixNanos,
) -> Option<crate::websocket::messages::PolymarketMarketResolved> {
    // Soft fallback is only considered once the market is operationally closed.
    let is_closed = market.closed.unwrap_or(false);
    let not_accepting = market.accepting_orders.is_some_and(|v| !v);
    if !(is_closed || not_accepting) {
        return None;
    }

    let assets_ids = parse_json_string_array(&market.clob_token_ids)?;
    let outcomes = parse_json_string_array(&market.outcomes).unwrap_or_default();
    let prices = parse_outcome_prices(&market.outcome_prices)?;
    let winner_idx = winning_index_from_outcome_prices_soft(&prices, 0.95, 0.05)?;
    let winning_asset_id = assets_ids.get(winner_idx)?.clone();
    let winning_outcome = outcomes
        .get(winner_idx)
        .cloned()
        .unwrap_or_else(|| "UNKNOWN".to_string());

    Some(crate::websocket::messages::PolymarketMarketResolved {
        id: format!("gamma-resolve-soft-{}", market.id),
        slug: market.market_slug.clone(),
        market: Ustr::from(market.condition_id.as_str()),
        assets_ids,
        winning_asset_id,
        winning_outcome,
        timestamp: (ts_init.as_u64() / 1_000_000).to_string(),
        tags: Vec::new(),
    })
}

fn parse_csv_values(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_condition_ids_from_request_params(params: &Option<Params>) -> Vec<String> {
    let mut ids = Vec::new();

    if let Some(params) = params {
        if let Some(raw) = params.get_str("condition_id") {
            ids.extend(parse_csv_values(raw));
        }

        if let Some(raw) = params.get_str("condition_ids") {
            ids.extend(parse_csv_values(raw));

            if let Ok(arr) = serde_json::from_str::<Vec<String>>(raw) {
                ids.extend(arr.into_iter().filter(|s| !s.trim().is_empty()));
            }
        }

        if let Some(raw) = params.get_str("instrument_id")
            && let Ok(instrument_id) = raw.parse::<InstrumentId>()
            && let Ok(condition_id) = extract_condition_id(&instrument_id)
        {
            ids.push(condition_id);
        }

        if let Some(raw) = params.get_str("instrument_ids") {
            for value in parse_csv_values(raw) {
                if let Ok(instrument_id) = value.parse::<InstrumentId>()
                    && let Ok(condition_id) = extract_condition_id(&instrument_id)
                {
                    ids.push(condition_id);
                }
            }

            if let Ok(arr) = serde_json::from_str::<Vec<String>>(raw) {
                for value in arr {
                    if let Ok(instrument_id) = value.parse::<InstrumentId>()
                        && let Ok(condition_id) = extract_condition_id(&instrument_id)
                    {
                        ids.push(condition_id);
                    }
                }
            }
        }
    }

    ids.sort();
    ids.dedup();
    ids
}

struct WsMessageContext {
    clock: &'static AtomicTime,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    token_meta: Arc<DashMap<Ustr, TokenMeta>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    gamma_client: PolymarketGammaHttpClient,
    filters: Vec<Arc<dyn InstrumentFilter>>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    resolve_poll_watchlist: Arc<AtomicMap<InstrumentId, ResolveWatchEntry>>,
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
    resolve_poll_watchlist: Arc<AtomicMap<InstrumentId, ResolveWatchEntry>>,
    pending_snapshot_after_tick_change: Arc<AtomicSet<InstrumentId>>,
    ws_open_tokens: Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    resolve_compensated_markets: Arc<AtomicSet<Ustr>>,
    pending_auto_loads: Arc<StdMutex<AHashSet<InstrumentId>>>,
    auto_load_scheduled: Arc<AtomicBool>,
    position_event_handler: Option<TypedHandler<PositionEvent>>,
}

impl PolymarketDataClient {
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
        let provider = PolymarketInstrumentProvider::new(gamma_client);

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
            pending_snapshot_after_tick_change: Arc::new(AtomicSet::new()),
            ws_open_tokens: Arc::new(AtomicSet::new()),
            ws_sub_mutex: Arc::new(tokio::sync::Mutex::new(())),
            resolve_compensated_markets: Arc::new(AtomicSet::new()),
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
            // Loop until the pending map is quiescent. Each iteration runs one
            // debounce window, then snapshots, fetches, and applies. A chunk
            // failure or a late-arriving miss keeps us in the loop; we exit
            // (releasing `scheduled`) only once `pending` is empty. This means a
            // transient Gamma failure is retried on the next debounce without
            // relying on some unrelated future miss to trigger it.
            loop {
                tokio::select! {
                    () = tokio::time::sleep(tokio::time::Duration::from_millis(debounce_ms)) => {}
                    () = cancellation.cancelled() => {
                        scheduled.store(false, Ordering::Release);
                        return;
                    }
                }

                let ids: Vec<InstrumentId> = {
                    let guard = pending.lock().expect("pending_auto_loads mutex poisoned");
                    guard.iter().copied().collect()
                };

                if ids.is_empty() {
                    scheduled.store(false, Ordering::Release);
                    return;
                }

                log::info!("Auto-loading {} missing instrument(s): {ids:?}", ids.len());

                let mut condition_ids: Vec<String> = ids
                    .iter()
                    .filter_map(|id| extract_condition_id(id).ok())
                    .collect();
                condition_ids.sort();
                condition_ids.dedup();

                if condition_ids.is_empty() {
                    log::error!("Auto-load aborted: no condition_ids could be extracted");
                    // Drop the stranded entries so we do not loop forever.
                    let mut guard = pending.lock().expect("pending_auto_loads mutex poisoned");
                    for id in &ids {
                        guard.remove(id);
                    }
                    continue;
                }

                // Gamma rejects condition_id queries larger than ~100, so chunk
                // the request and merge the results. This matches the provider's
                // own `_load_ids_using_gamma_markets` chunking policy.
                let mut loaded: Vec<InstrumentAny> =
                    Vec::with_capacity(condition_ids.len().min(GAMMA_CONDITION_IDS_BATCH_SIZE));
                let mut chunk_failed = false;

                for chunk in condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
                    let params = GetGammaMarketsParams {
                        condition_ids: Some(chunk.join(",")),
                        ..Default::default()
                    };

                    match http.request_instruments_by_params(params).await {
                        Ok(insts) => loaded.extend(insts),
                        Err(e) => {
                            log::error!(
                                "Auto-load batch failed for chunk of {} condition_id(s): {e:?}",
                                chunk.len()
                            );
                            chunk_failed = true;
                            break;
                        }
                    }
                }

                if chunk_failed {
                    // Leave entries in `pending` and loop around; the next
                    // iteration retries after another debounce window.
                    continue;
                }

                for inst in loaded {
                    if !filters.iter().all(|f| f.accept(&inst)) {
                        log::debug!("Auto-loaded instrument {} filtered out", inst.id());
                        continue;
                    }

                    cache_instrument(&instruments, &token_meta, &inst);

                    let instrument_id = inst.id();
                    if let Err(e) = data_sender.send(DataEvent::Instrument(inst)) {
                        log::error!("Failed to emit auto-loaded instrument {instrument_id}: {e}");
                    }
                }

                for instrument_id in ids {
                    // Pop the pending entry under the lock; if `unsubscribe_*`
                    // already cleared it, skip.
                    let was_pending = {
                        let mut guard = pending.lock().expect("pending_auto_loads mutex poisoned");
                        guard.remove(&instrument_id)
                    };

                    if !was_pending {
                        continue;
                    }

                    let Ok(token_id) = resolve_token_id_from(&instruments, instrument_id) else {
                        log::error!("Auto-load did not return instrument {instrument_id}");
                        continue;
                    };

                    // Reconcile WS state with whichever `active_*_subs` still
                    // hold intent. A concurrent unsubscribe makes this a no-op.
                    sync_ws_subscription_async(
                        instrument_id,
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
            }
        });
    }

    async fn bootstrap_instruments(&mut self) -> anyhow::Result<()> {
        self.provider.load_all(None).await?;

        let all_instruments = self.provider.store().list_all();
        let total = all_instruments.len();
        for instrument in all_instruments {
            cache_instrument(&self.instruments, &self.token_meta, instrument);
            let instrument_id = instrument.id();

            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::warn!("Failed to publish instrument {instrument_id}: {e}");
            }
        }

        log::info!("Published all {total} instruments to data engine");
        Ok(())
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
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
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
            return;
        }

        let interval_secs = self.config.resolve_poll_interval_secs.max(5);
        let expiry_grace_secs = self.config.resolve_poll_expiry_grace_secs;
        let max_wait_secs = self
            .config
            .resolve_poll_max_wait_secs
            .max(expiry_grace_secs);
        let cancellation = self.cancellation_token.clone();
        let gamma_client = self.provider.http_client().clone();
        let resolve_poll_watchlist = self.resolve_poll_watchlist.clone();
        let emit_compensation = self.config.resolve_poll_emit_compensation;
        let resolve_compensated_markets = self.resolve_compensated_markets.clone();

        let poll_ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            gamma_client: self.provider.http_client().clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            cancellation_token: cancellation.clone(),
        };

        let handle = get_runtime().spawn(async move {
            let mut ticker =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut last_skip_log_key: Option<ResolvePollSkipLogKey> = None;
            let mut last_skip_log_ns: u64 = 0;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let watchlist = resolve_poll_watchlist.load();
                        if watchlist.is_empty() {
                            continue;
                        }

                        let now_ns = poll_ctx.clock.get_time_ns();
                        let selection = collect_resolve_watch_selection(
                            &watchlist,
                            now_ns,
                            expiry_grace_secs,
                            max_wait_secs,
                            ResolveWatchSelectionMode::AutoPoll,
                        );
                        pause_resolve_watch_entries(
                            &resolve_poll_watchlist,
                            &selection.pause_auto_poll,
                        );

                        if !selection.pause_auto_poll.is_empty() {
                            log::warn!(
                                "Resolve GET poll max_wait reached for {} instrument(s), automatic polling paused (max_wait_secs={})",
                                selection.pause_auto_poll.len(),
                                max_wait_secs
                            );
                        }

                        if selection.condition_ids.is_empty() {
                            let skip_log_key = ResolvePollSkipLogKey {
                                tracked: watchlist.len(),
                                skipped_not_expired: selection.skipped_not_expired,
                                missing_expiration: selection.missing_expiration,
                                timed_out_watchlist: selection.timed_out_watchlist,
                                paused_watchlist: selection.paused_watchlist,
                                min_ready_bucket_mins: selection.min_ready_in_secs.map(|secs| secs / 60),
                            };
                            let should_log_skip = (last_skip_log_key.as_ref()
                                != Some(&skip_log_key))
                                || now_ns.as_u64().saturating_sub(last_skip_log_ns)
                                    >= 60_000_000_000;

                            if should_log_skip {
                                log::debug!(
                                    "Resolve GET poll waiting: tracked={} expired_ready=0 skipped_not_expired={} missing_expiration={} timed_out_watchlist={} paused_watchlist={} min_ready_in_secs={:?} not_ready_samples={:?}",
                                    watchlist.len(),
                                    selection.skipped_not_expired,
                                    selection.missing_expiration,
                                    selection.timed_out_watchlist,
                                    selection.paused_watchlist,
                                    selection.min_ready_in_secs,
                                    selection.not_ready_samples
                                );
                                last_skip_log_key = Some(skip_log_key);
                                last_skip_log_ns = now_ns.as_u64();
                            }
                            continue;
                        }

                        last_skip_log_key = None;
                        last_skip_log_ns = 0;

                        let mut fetched_markets: Vec<GammaMarket> = Vec::new();
                        let mut request_failed = false;

                        for condition_id in &selection.condition_ids {
                            let params = GetGammaMarketsParams {
                                condition_ids: Some(condition_id.clone()),
                                closed: Some(true),
                                ..Default::default()
                            };

                            match gamma_client.request_markets_by_params(params).await {
                                Ok(markets) => fetched_markets.extend(markets),
                                Err(e) => {
                                    request_failed = true;
                                    log::warn!(
                                        "Resolve GET poll failed for condition_id={condition_id}: {e}"
                                    );
                                    break;
                                }
                            }
                        }

                        if request_failed {
                            continue;
                        }

                        if fetched_markets.is_empty() {
                            log::debug!(
                                "Resolve GET poll: no markets returned for {} condition_ids",
                                selection.condition_ids.len()
                            );
                            continue;
                        }

                        let mut markets_by_condition: AHashMap<String, &GammaMarket> =
                            AHashMap::new();

                        for market in &fetched_markets {
                            markets_by_condition.insert(market.condition_id.clone(), market);
                        }

                        let mut resolved_candidates = 0usize;

                        for condition_id in &selection.condition_ids {
                            let Some(market) = markets_by_condition.get(condition_id) else {
                                log::debug!(
                                    "Resolve GET poll: condition_id={condition_id} missing in response",
                                );
                                continue;
                            };

                            let outcome_prices = parse_outcome_prices(&market.outcome_prices);
                            let assets_ids = parse_json_string_array(&market.clob_token_ids)
                                .unwrap_or_default();
                            log::debug!(
                                "Resolve GET diagnostic: condition_id={} market_id={} slug={:?} active={:?} closed={:?} assets_ids={:?} outcome_prices={:?}",
                                market.condition_id,
                                market.id,
                                market.market_slug,
                                market.active,
                                market.closed,
                                assets_ids,
                                outcome_prices,
                            );

                            let ts_init = poll_ctx.clock.get_time_ns();
                            let resolved = build_market_resolved_from_gamma(market, ts_init)
                                .or_else(|| build_market_resolved_from_gamma_soft(market, ts_init));
                            let Some(resolved) = resolved else {
                                continue;
                            };
                            resolved_candidates += 1;

                            if !emit_compensation {
                                continue;
                            }

                            let dedupe_key = Ustr::from(
                                format!(
                                    "{}:{}",
                                    resolved.market, resolved.winning_asset_id
                                )
                                .as_str(),
                            );

                            if resolve_compensated_markets.contains(&dedupe_key) {
                                continue;
                            }
                            resolve_compensated_markets.insert(dedupe_key);

                            log::info!(
                                "Resolve GET compensation emitting synthetic market_resolved: condition_id={} winning_asset_id={} winning_outcome={}",
                                resolved.market,
                                resolved.winning_asset_id,
                                resolved.winning_outcome
                            );
                            Self::handle_market_message(MarketWsMessage::MarketResolved(resolved), &poll_ctx);
                        }

                        if resolved_candidates > 0 || !selection.pause_auto_poll.is_empty() {
                            log::info!(
                                "Resolve GET poll cycle complete: tracked_instruments={} condition_ids={} skipped_not_expired={} missing_expiration={} timed_out_watchlist={} paused_watchlist={} resolved_candidates={} compensation={}",
                                watchlist.len(),
                                selection.condition_ids.len(),
                                selection.skipped_not_expired,
                                selection.missing_expiration,
                                selection.timed_out_watchlist,
                                selection.paused_watchlist,
                                resolved_candidates,
                                emit_compensation,
                            );
                        } else {
                            log::debug!(
                                "Resolve GET poll cycle complete: tracked_instruments={} condition_ids={} resolved_candidates={} compensation={}",
                                watchlist.len(),
                                selection.condition_ids.len(),
                                resolved_candidates,
                                emit_compensation,
                            );
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("Resolve GET poll task cancelled");
                        break;
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
                log::debug!(
                    "Market resolved raw event: id={} slug={:?} market={} ts={} winner={} ({}) assets_ids={:?}",
                    resolved.id,
                    resolved.slug,
                    resolved.market,
                    resolved.timestamp,
                    resolved.winning_asset_id,
                    resolved.winning_outcome,
                    resolved.assets_ids
                );

                let ts_init = ctx.clock.get_time_ns();
                let winning_asset_id = Ustr::from(resolved.winning_asset_id.as_str());
                let reason = Ustr::from(&format!(
                    "Winner: {} ({})",
                    resolved.winning_asset_id, resolved.winning_outcome
                ));
                let loaded_instruments = ctx.instruments.load();
                let mut mapped_assets = 0usize;
                let mut missing_assets: Vec<String> = Vec::new();
                let mut seen_instruments = AHashSet::new();
                let mut resolved_targets: Vec<ResolvedTarget> = Vec::new();
                let resolved_market = resolved.market.to_string();

                for asset_id in &resolved.assets_ids {
                    let token_id = Ustr::from(asset_id.as_str());
                    if let Some(meta) = ctx.token_meta.get(&token_id) {
                        if !seen_instruments.insert(meta.instrument_id) {
                            continue;
                        }
                        mapped_assets += 1;
                        let (slug, market_id, condition_id) = loaded_instruments
                            .get(&meta.instrument_id)
                            .map_or((None, None, None), instrument_market_context);
                        resolved_targets.push((
                            meta.instrument_id,
                            token_id,
                            meta.price_precision,
                            slug,
                            market_id,
                            condition_id,
                            "assets_ids",
                        ));
                    } else {
                        missing_assets.push(asset_id.clone());
                        log::warn!(
                            "Market resolved asset unmapped via token_meta: market={} asset_id={} token_meta_size={}",
                            resolved.market,
                            asset_id,
                            ctx.token_meta.len(),
                        );
                    }
                }

                let needs_context_fallback =
                    resolved_targets.is_empty() || !missing_assets.is_empty();

                if needs_context_fallback {
                    let mut fallback_count = 0usize;

                    for (instrument_id, instrument) in loaded_instruments.iter() {
                        let (slug, market_id, condition_id) = instrument_market_context(instrument);
                        let token_id = Ustr::from(instrument.raw_symbol().as_str());

                        let matches_slug = resolved
                            .slug
                            .as_deref()
                            .is_some_and(|rs| slug.as_deref() == Some(rs));
                        let matches_market_id =
                            market_id.as_deref() == Some(resolved_market.as_str());
                        let matches_condition_id =
                            condition_id.as_deref() == Some(resolved_market.as_str());
                        let matches_assets_list =
                            resolved.assets_ids.iter().any(|id| id == token_id.as_str());

                        if !(matches_slug
                            || matches_market_id
                            || matches_condition_id
                            || matches_assets_list)
                        {
                            continue;
                        }

                        if !seen_instruments.insert(*instrument_id) {
                            continue;
                        }

                        fallback_count += 1;
                        resolved_targets.push((
                            *instrument_id,
                            token_id,
                            instrument.price_precision(),
                            slug,
                            market_id,
                            condition_id,
                            "context_fallback",
                        ));
                    }

                    if fallback_count > 0 {
                        log::debug!(
                            "Market resolved context fallback mapped {} additional instrument(s): market={} slug={:?}",
                            fallback_count,
                            resolved.market,
                            resolved.slug,
                        );
                    }
                }

                if !missing_assets.is_empty() {
                    let known_tokens_sample: Vec<String> = ctx
                        .token_meta
                        .iter()
                        .take(10)
                        .map(|entry| entry.key().to_string())
                        .collect();
                    log::warn!(
                        "Market resolved mapping incomplete via assets_ids: market={} assets_total={} mapped_assets={} missing_assets={:?} known_tokens_sample={:?}",
                        resolved.market,
                        resolved.assets_ids.len(),
                        mapped_assets,
                        missing_assets,
                        known_tokens_sample,
                    );
                }

                if resolved_targets.is_empty() {
                    log::warn!(
                        "Market resolved produced no instrument targets: market={} slug={:?} winner_asset_id={}",
                        resolved.market,
                        resolved.slug,
                        resolved.winning_asset_id,
                    );
                    return;
                }

                for (
                    instrument_id,
                    token_id,
                    price_precision,
                    slug,
                    market_id,
                    condition_id,
                    source,
                ) in resolved_targets
                {
                    log::debug!(
                        "Market resolved mapped target: source={} market={} asset_id={} instrument_id={} slug={:?} market_id={:?} condition_id={:?} winner={}",
                        source,
                        resolved.market,
                        token_id,
                        instrument_id,
                        slug,
                        market_id,
                        condition_id,
                        token_id == winning_asset_id,
                    );

                    let status = InstrumentStatus::new(
                        instrument_id,
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
                        log::error!("Failed to emit instrument status for {instrument_id}: {e}");
                    }

                    let close_price = if token_id == winning_asset_id {
                        Price::new(1.0, price_precision)
                    } else {
                        Price::new(0.0, price_precision)
                    };
                    let close = InstrumentClose::new(
                        instrument_id,
                        close_price,
                        InstrumentCloseType::ContractExpired,
                        ts_init,
                        ts_init,
                    );

                    if let Err(e) = ctx
                        .data_sender
                        .send(DataEvent::Data(NautilusData::InstrumentClose(close)))
                    {
                        log::error!("Failed to emit instrument close for {instrument_id}: {e}");
                    } else {
                        log::info!(
                            "Market resolved emitted InstrumentClose: instrument_id={} close_price={} close_type={:?}",
                            instrument_id,
                            close_price,
                            InstrumentCloseType::ContractExpired,
                        );
                    }
                    ctx.resolve_poll_watchlist.remove(&instrument_id);
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
        self.resolve_compensated_markets.store(AHashSet::new());
        self.resolve_poll_watchlist.store(AHashMap::new());
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
        if request.data_type.type_name() != RESOLVE_TRIGGER_TYPE_NAME {
            log::debug!(
                "Ignoring unsupported custom data request type: {}",
                request.data_type.type_name()
            );
            return Ok(());
        }

        let gamma_client = self.provider.http_client().clone();
        let sender = self.data_sender.clone();
        let data_type = request.data_type.clone();
        let data_type_params = request.data_type.metadata().cloned();
        let request_params = request.params.clone();
        let request_id = request.request_id;
        let client_id = request.client_id;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let clock = self.clock;
        let resolve_poll_watchlist = self.resolve_poll_watchlist.clone();
        let resolve_compensated_markets = self.resolve_compensated_markets.clone();
        let expiry_grace_secs = self.config.resolve_poll_expiry_grace_secs;
        let max_wait_secs = self
            .config
            .resolve_poll_max_wait_secs
            .max(expiry_grace_secs);
        let emit_compensation_default = self.config.resolve_poll_emit_compensation;

        let poll_ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            gamma_client: self.provider.http_client().clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            cancellation_token: self.cancellation_token.clone(),
        };

        get_runtime().spawn(async move {
            let mut summary = ResolveRequestSummary {
                requested_condition_ids: Vec::new(),
                used_watchlist_fallback: false,
                fetched_markets: 0,
                resolved_candidates: 0,
                compensation_emitted: 0,
                timed_out_watchlist: 0,
                error: None,
            };

            let mut condition_ids = parse_condition_ids_from_request_params(&request_params);
            if condition_ids.is_empty() {
                condition_ids = parse_condition_ids_from_request_params(&data_type_params);
            }

            if condition_ids.is_empty() {
                summary.used_watchlist_fallback = true;
                let watchlist = resolve_poll_watchlist.load();
                let now_ns = clock.get_time_ns();
                let selection = collect_resolve_watch_selection(
                    &watchlist,
                    now_ns,
                    expiry_grace_secs,
                    max_wait_secs,
                    ResolveWatchSelectionMode::ManualFallback,
                );
                pause_resolve_watch_entries(&resolve_poll_watchlist, &selection.pause_auto_poll);
                summary.timed_out_watchlist = selection.timed_out_watchlist;
                condition_ids = selection.condition_ids;
            }

            condition_ids.sort();
            condition_ids.dedup();
            summary.requested_condition_ids = condition_ids.clone();

            if condition_ids.is_empty() {
                summary.error = Some("No eligible condition_ids for resolve request".to_string());
                let response = DataResponse::Data(CustomDataResponse::new(
                    request_id,
                    client_id,
                    Some(*POLYMARKET_VENUE),
                    data_type.clone(),
                    summary,
                    start_nanos,
                    end_nanos,
                    clock.get_time_ns(),
                    request_params.clone(),
                ));

                if let Err(e) = sender.send(DataEvent::Response(response)) {
                    log::error!("Failed to send resolve request response: {e}");
                }
                return;
            }

            let mut fetched_markets: Vec<GammaMarket> = Vec::new();

            for condition_id in &condition_ids {
                let params = GetGammaMarketsParams {
                    condition_ids: Some(condition_id.clone()),
                    closed: Some(true),
                    ..Default::default()
                };

                match gamma_client.request_markets_by_params(params).await {
                    Ok(markets) => fetched_markets.extend(markets),
                    Err(e) => {
                        summary.error =
                            Some(format!("Resolve request failed for condition_id={condition_id}: {e}"));
                        log::warn!(
                            "Resolve request failed for condition_id={condition_id}: {e}"
                        );
                        break;
                    }
                }
            }

            summary.fetched_markets = fetched_markets.len();
            let emit_compensation = request_params
                .as_ref()
                .and_then(|params| params.get_bool("emit_compensation"))
                .unwrap_or(emit_compensation_default);

            if summary.error.is_none() {
                let mut markets_by_condition: AHashMap<String, &GammaMarket> = AHashMap::new();

                for market in &fetched_markets {
                    markets_by_condition.insert(market.condition_id.clone(), market);
                }

                for condition_id in &condition_ids {
                    let Some(market) = markets_by_condition.get(condition_id) else {
                        continue;
                    };
                    let ts_init = clock.get_time_ns();
                    let resolved = build_market_resolved_from_gamma(market, ts_init)
                        .or_else(|| build_market_resolved_from_gamma_soft(market, ts_init));
                    let Some(resolved) = resolved else {
                        continue;
                    };
                    summary.resolved_candidates += 1;

                    if !emit_compensation {
                        continue;
                    }

                    let dedupe_key =
                        Ustr::from(format!("{}:{}", resolved.market, resolved.winning_asset_id).as_str());

                    if resolve_compensated_markets.contains(&dedupe_key) {
                        continue;
                    }
                    resolve_compensated_markets.insert(dedupe_key);
                    summary.compensation_emitted += 1;

                    log::info!(
                        "Resolve request compensation emitting synthetic market_resolved: condition_id={} winning_asset_id={} winning_outcome={}",
                        resolved.market,
                        resolved.winning_asset_id,
                        resolved.winning_outcome
                    );
                    Self::handle_market_message(
                        MarketWsMessage::MarketResolved(resolved),
                        &poll_ctx,
                    );
                }
            }

            let response = DataResponse::Data(CustomDataResponse::new(
                request_id,
                client_id,
                Some(*POLYMARKET_VENUE),
                data_type,
                summary,
                start_nanos,
                end_nanos,
                clock.get_time_ns(),
                request_params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send resolve request response: {e}");
            }
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.provider.http_client().clone();
        let filters = self.provider.filters();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = *POLYMARKET_VENUE;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match fetch_instruments(&http, &filters).await {
                Ok(instruments) => {
                    log::info!("Fetched {} instruments from Gamma API", instruments.len());

                    for instrument in &instruments {
                        cache_instrument(&instruments_cache, &token_meta, instrument);
                    }

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
                }
                Err(e) => {
                    log::error!("Failed to fetch instruments from Gamma API: {e:?}");
                }
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
    use nautilus_core::params::Params;
    use nautilus_core::{UUID4, UnixNanos, nanos::DurationNanos};
    use nautilus_model::{
        enums::{AssetClass, InstrumentCloseType, OrderSide, PositionSide},
        events::{PositionClosed, PositionEvent, PositionOpened},
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TraderId,
        },
        instruments::BinaryOption,
        stubs::TestDefault,
        types::{Currency, Money, Price, Quantity},
    };
    use nautilus_network::retry::RetryConfig;
    use rstest::rstest;

    use super::*;
    use crate::{
        common::enums::PolymarketOrderSide,
        websocket::{
            client::WsSubscriptionHandle,
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

    fn make_gamma_market_with_outcome_prices(
        outcome_prices: Option<&str>,
    ) -> crate::http::models::GammaMarket {
        let mut value = serde_json::json!({
            "id": "1557558",
            "conditionId": "0xcondition",
            "questionID": "0xquestion",
            "clobTokenIds": "[\"0xyes\",\"0xno\"]",
            "outcomes": "[\"Yes\",\"No\"]",
            "question": "Will test pass?",
            "description": null,
            "startDate": null,
            "endDate": null,
            "active": false,
            "closed": true,
            "acceptingOrders": false,
            "enableOrderBook": false,
            "slug": "test-market",
            "events": []
        });

        if let Some(prices) = outcome_prices {
            value["outcomePrices"] = serde_json::Value::String(prices.to_string());
        }
        serde_json::from_value(value).expect("valid gamma market json")
    }

    #[rstest]
    fn build_market_resolved_from_gamma_parses_1_0_outcome_prices() {
        let market = make_gamma_market_with_outcome_prices(Some("[\"1\",\"0\"]"));
        let resolved = build_market_resolved_from_gamma(&market, UnixNanos::from(1_700_000_000))
            .expect("expected resolved payload");

        assert_eq!(resolved.market.as_str(), "0xcondition");
        assert_eq!(
            resolved.assets_ids,
            vec!["0xyes".to_string(), "0xno".to_string()]
        );
        assert_eq!(resolved.winning_asset_id, "0xyes");
        assert_eq!(resolved.winning_outcome, "Yes");
    }

    #[rstest]
    fn build_market_resolved_from_gamma_rejects_ambiguous_outcomes() {
        let market = make_gamma_market_with_outcome_prices(Some("[\"0.6\",\"0.4\"]"));
        let resolved = build_market_resolved_from_gamma(&market, UnixNanos::from(1_700_000_000));

        assert!(resolved.is_none());
    }

    #[rstest]
    fn build_market_resolved_from_gamma_soft_accepts_closed_decisive_market() {
        let market = make_gamma_market_with_outcome_prices(Some("[\"0.97\",\"0.03\"]"));
        let resolved =
            build_market_resolved_from_gamma_soft(&market, UnixNanos::from(1_700_000_000))
                .expect("expected soft resolved payload");

        assert_eq!(resolved.market.as_str(), "0xcondition");
        assert_eq!(resolved.winning_asset_id, "0xyes");
        assert_eq!(resolved.winning_outcome, "Yes");
    }

    #[rstest]
    fn build_market_resolved_from_gamma_soft_rejects_not_closed_market() {
        let mut market = make_gamma_market_with_outcome_prices(Some("[\"0.97\",\"0.03\"]"));
        market.closed = Some(false);
        market.accepting_orders = Some(true);

        let resolved =
            build_market_resolved_from_gamma_soft(&market, UnixNanos::from(1_700_000_000));
        assert!(resolved.is_none());
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

        let ctx = WsMessageContext {
            clock: get_atomic_clock_realtime(),
            data_sender: data_tx,
            token_meta: Arc::new(DashMap::new()),
            instruments: Arc::new(AtomicMap::new()),
            gamma_client,
            filters: vec![],
            order_books: Arc::new(DashMap::new()),
            last_quotes: Arc::new(DashMap::new()),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_delta_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            resolve_poll_watchlist: Arc::new(AtomicMap::new()),
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

    fn seed_instrument_with_context(
        ctx: &WsMessageContext,
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
        market_slug: Option<&str>,
        market_id: Option<&str>,
        condition_id: Option<&str>,
    ) -> InstrumentAny {
        seed_instrument_with_context_and_expiration(
            ctx,
            raw_symbol,
            price_increment,
            size_increment,
            market_slug,
            market_id,
            condition_id,
            None,
        )
    }

    #[expect(clippy::too_many_arguments)]
    fn seed_instrument_with_context_and_expiration(
        ctx: &WsMessageContext,
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
        market_slug: Option<&str>,
        market_id: Option<&str>,
        condition_id: Option<&str>,
        expiration_ns: Option<UnixNanos>,
    ) -> InstrumentAny {
        let mut inst = stub_instrument(raw_symbol, price_increment, size_increment);
        if let InstrumentAny::BinaryOption(ref mut bo) = inst {
            if let Some(expiration_ns) = expiration_ns {
                bo.expiration_ns = expiration_ns;
            }
            let mut info = Params::new();
            info.insert(
                "token_id".to_string(),
                serde_json::Value::String(raw_symbol.to_string()),
            );

            if let Some(slug) = market_slug {
                info.insert(
                    "market_slug".to_string(),
                    serde_json::Value::String(slug.to_string()),
                );
            }

            if let Some(id) = market_id {
                info.insert(
                    "market_id".to_string(),
                    serde_json::Value::String(id.to_string()),
                );
            }

            if let Some(id) = condition_id {
                info.insert(
                    "condition_id".to_string(),
                    serde_json::Value::String(id.to_string()),
                );
            }
            bo.info = Some(info);
        }

        cache_instrument(&ctx.instruments, &ctx.token_meta, &inst);
        inst
    }

    fn stub_position_opened_event(instrument_id: InstrumentId) -> PositionEvent {
        PositionEvent::PositionOpened(PositionOpened {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id,
            position_id: PositionId::new("P-1"),
            account_id: AccountId::from("ACCOUNT-001"),
            opening_order_id: ClientOrderId::from("ENTRY-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 5.0,
            quantity: Quantity::from("5"),
            last_qty: Quantity::from("5"),
            last_px: Price::from("0.75"),
            currency: Currency::pUSD(),
            avg_px_open: 0.75,
            event_id: UUID4::new(),
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
        })
    }

    fn stub_position_closed_event(instrument_id: InstrumentId) -> PositionEvent {
        PositionEvent::PositionClosed(PositionClosed {
            trader_id: TraderId::test_default(),
            strategy_id: StrategyId::test_default(),
            instrument_id,
            position_id: PositionId::new("P-1"),
            account_id: AccountId::from("ACCOUNT-001"),
            opening_order_id: ClientOrderId::from("ENTRY-1"),
            closing_order_id: Some(ClientOrderId::from("EXIT-1")),
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::from("0"),
            peak_quantity: Quantity::from("5"),
            last_qty: Quantity::from("5"),
            last_px: Price::from("1.0"),
            currency: Currency::pUSD(),
            avg_px_open: 0.75,
            avg_px_close: Some(1.0),
            realized_return: 0.3333333333,
            realized_pnl: Some(Money::new(1.0, Currency::pUSD())),
            unrealized_pnl: Money::new(0.0, Currency::pUSD()),
            duration: DurationNanos::from(1u64),
            event_id: UUID4::new(),
            ts_opened: UnixNanos::from(1),
            ts_closed: Some(UnixNanos::from(2)),
            ts_event: UnixNanos::from(2),
            ts_init: UnixNanos::from(2),
        })
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

    fn make_market_resolved(
        market: &str,
        assets_ids: Vec<&str>,
        winning_asset_id: &str,
    ) -> MarketWsMessage {
        MarketWsMessage::MarketResolved(PolymarketMarketResolved {
            id: "resolved-1".to_string(),
            slug: None,
            market: Ustr::from(market),
            assets_ids: assets_ids
                .into_iter()
                .map(std::string::ToString::to_string)
                .collect(),
            winning_asset_id: winning_asset_id.to_string(),
            winning_outcome: "Yes".to_string(),
            timestamp: "1700000004000".to_string(),
            tags: vec![],
        })
    }

    fn make_market_resolved_with_slug(
        market: &str,
        slug: Option<&str>,
        assets_ids: Vec<&str>,
        winning_asset_id: &str,
    ) -> MarketWsMessage {
        MarketWsMessage::MarketResolved(PolymarketMarketResolved {
            id: "resolved-2".to_string(),
            slug: slug.map(std::string::ToString::to_string),
            market: Ustr::from(market),
            assets_ids: assets_ids
                .into_iter()
                .map(std::string::ToString::to_string)
                .collect(),
            winning_asset_id: winning_asset_id.to_string(),
            winning_outcome: "Yes".to_string(),
            timestamp: "1700000005000".to_string(),
            tags: vec![],
        })
    }

    fn make_official_market_resolved_payload(
        market: &str,
        slug: &str,
        winner_asset_id: &str,
        loser_asset_id: &str,
    ) -> MarketWsMessage {
        let payload = serde_json::json!({
            "id": "1031769",
            "question": "Will NVIDIA (NVDA) close above $240 end of January?",
            "market": market,
            "slug": slug,
            "description": "Official sample-like payload for integration test",
            "assets_ids": [winner_asset_id, loser_asset_id],
            "outcomes": ["Yes", "No"],
            "winning_asset_id": winner_asset_id,
            "winning_outcome": "Yes",
            "event_message": {
                "id": "125819",
                "ticker": "nvda-above-in-january-2026",
                "slug": "nvda-above-in-january-2026",
                "title": "Will NVIDIA (NVDA) close above ___ end of January?",
                "description": "..."
            },
            "timestamp": "1766790415550",
            "event_type": "market_resolved"
        });

        serde_json::from_value(payload).expect("valid official-style market_resolved payload")
    }

    #[rstest]
    fn market_resolved_emits_status_and_close_for_each_asset() {
        let market = "0xMARKET";
        let winner_asset = "0xTOKEN_WIN";
        let loser_asset = "0xTOKEN_LOSE";

        let (ctx, mut data_rx) = make_ws_ctx();
        let winner = seed_instrument(
            &ctx,
            winner_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );
        let loser = seed_instrument(
            &ctx,
            loser_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );

        let resolved = make_market_resolved(market, vec![winner_asset, loser_asset], winner_asset);
        PolymarketDataClient::handle_market_message(resolved, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        let status_count = events
            .iter()
            .filter(|e| matches!(e, DataEvent::InstrumentStatus(_)))
            .count();
        assert_eq!(
            status_count, 2,
            "expected 2 status events, found: {events:?}"
        );

        let mut winner_close = None;
        let mut loser_close = None;

        for event in events {
            if let DataEvent::Data(NautilusData::InstrumentClose(close)) = event {
                if close.instrument_id == winner.id() {
                    winner_close = Some(close);
                } else if close.instrument_id == loser.id() {
                    loser_close = Some(close);
                }
            }
        }

        let winner_close = winner_close.expect("expected winner close event");
        let loser_close = loser_close.expect("expected loser close event");
        assert_eq!(
            winner_close.close_type,
            InstrumentCloseType::ContractExpired
        );
        assert_eq!(loser_close.close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(winner_close.close_price.as_f64(), 1.0);
        assert_eq!(loser_close.close_price.as_f64(), 0.0);
    }

    #[rstest]
    fn market_resolved_slug_fallback_maps_without_assets_ids() {
        let slug = "btc-updown-5m-1778973900";
        let market_id = "1778973900";
        let winner_asset = "0xTOKEN_WIN";
        let loser_asset = "0xTOKEN_LOSE";

        let (ctx, mut data_rx) = make_ws_ctx();
        let winner = seed_instrument_with_context(
            &ctx,
            winner_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
            Some(slug),
            Some(market_id),
            None,
        );
        let loser = seed_instrument_with_context(
            &ctx,
            loser_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
            Some(slug),
            Some(market_id),
            None,
        );

        let resolved = make_market_resolved_with_slug(market_id, Some(slug), vec![], winner_asset);
        PolymarketDataClient::handle_market_message(resolved, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        let status_count = events
            .iter()
            .filter(|e| matches!(e, DataEvent::InstrumentStatus(_)))
            .count();
        assert_eq!(
            status_count, 2,
            "expected 2 status events, found: {events:?}"
        );

        let mut winner_close = None;
        let mut loser_close = None;

        for event in events {
            if let DataEvent::Data(NautilusData::InstrumentClose(close)) = event {
                if close.instrument_id == winner.id() {
                    winner_close = Some(close);
                } else if close.instrument_id == loser.id() {
                    loser_close = Some(close);
                }
            }
        }

        let winner_close = winner_close.expect("expected winner close event");
        let loser_close = loser_close.expect("expected loser close event");
        assert_eq!(
            winner_close.close_type,
            InstrumentCloseType::ContractExpired
        );
        assert_eq!(loser_close.close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(winner_close.close_price.as_f64(), 1.0);
        assert_eq!(loser_close.close_price.as_f64(), 0.0);
    }

    #[rstest]
    fn market_resolved_official_payload_maps_and_closes_positions() {
        let slug = "btc-updown-5m-1778973900";
        let market = "0xCOND-OFFICIAL";
        let winner_asset = "0xTOKEN_OFFICIAL_WIN";
        let loser_asset = "0xTOKEN_OFFICIAL_LOSE";

        let (ctx, mut data_rx) = make_ws_ctx();
        let winner = seed_instrument_with_context(
            &ctx,
            winner_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
            Some(slug),
            Some("1778973900"),
            Some(market),
        );
        let loser = seed_instrument_with_context(
            &ctx,
            loser_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
            Some(slug),
            Some("1778973900"),
            Some(market),
        );

        let resolved =
            make_official_market_resolved_payload(market, slug, winner_asset, loser_asset);
        PolymarketDataClient::handle_market_message(resolved, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        let status_count = events
            .iter()
            .filter(|e| matches!(e, DataEvent::InstrumentStatus(_)))
            .count();
        assert_eq!(
            status_count, 2,
            "expected 2 status events, found: {events:?}"
        );

        let mut winner_close = None;
        let mut loser_close = None;

        for event in events {
            if let DataEvent::Data(NautilusData::InstrumentClose(close)) = event {
                if close.instrument_id == winner.id() {
                    winner_close = Some(close);
                } else if close.instrument_id == loser.id() {
                    loser_close = Some(close);
                }
            }
        }

        let winner_close = winner_close.expect("expected winner close event");
        let loser_close = loser_close.expect("expected loser close event");
        assert_eq!(winner_close.close_price.as_f64(), 1.0);
        assert_eq!(loser_close.close_price.as_f64(), 0.0);
    }

    #[rstest]
    fn position_events_drive_resolve_watchlist_membership() {
        let (ctx, _data_rx) = make_ws_ctx();
        let instrument = seed_instrument_with_context(
            &ctx,
            "0xTOKEN_POSITION",
            Price::from("0.001"),
            Quantity::from("0.01"),
            Some("btc-updown-5m-1778973900"),
            Some("1778973900"),
            Some("0xCOND-POSITION"),
        );
        let instrument_id = instrument.id();

        let opened = stub_position_opened_event(instrument_id);
        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &opened,
        );

        let watchlist = ctx.resolve_poll_watchlist.load();
        let entry = watchlist
            .get(&instrument_id)
            .expect("expected position-opened to add watchlist entry");
        assert_eq!(entry.condition_id.as_deref(), Some("0xCOND-POSITION"));
        assert!(entry.auto_poll_active);
        drop(watchlist);

        let closed = stub_position_closed_event(instrument_id);
        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &closed,
        );

        assert!(!ctx.resolve_poll_watchlist.contains_key(&instrument_id));
    }

    #[rstest]
    fn position_events_drive_resolve_watchlist_for_shared_and_distinct_markets() {
        let (ctx, _data_rx) = make_ws_ctx();
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let btc_yes = seed_instrument_with_context_and_expiration(
            &ctx,
            "0xTOKEN_BTC_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            Some("btc-updown-5m-1778973900"),
            Some("1778973900"),
            Some("0xCOND-BTC"),
            Some(expiration_ns),
        );
        let btc_no = seed_instrument_with_context_and_expiration(
            &ctx,
            "0xTOKEN_BTC_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            Some("btc-updown-5m-1778973900"),
            Some("1778973900"),
            Some("0xCOND-BTC"),
            Some(expiration_ns),
        );
        let eth_yes = seed_instrument_with_context_and_expiration(
            &ctx,
            "0xTOKEN_ETH_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            Some("eth-updown-5m-1778973900"),
            Some("2778973900"),
            Some("0xCOND-ETH"),
            Some(expiration_ns),
        );

        for instrument_id in [btc_yes.id(), btc_no.id(), eth_yes.id()] {
            let opened = stub_position_opened_event(instrument_id);
            update_resolve_watchlist_from_position_event(
                &ctx.resolve_poll_watchlist,
                &ctx.instruments,
                &opened,
            );
        }

        let now_ns = UnixNanos::from(expiration_ns.as_u64().saturating_add(11_000_000_000));

        let watchlist = ctx.resolve_poll_watchlist.load();
        assert_eq!(watchlist.len(), 3);
        let selection = collect_resolve_watch_selection(
            &watchlist,
            now_ns,
            10,
            1800,
            ResolveWatchSelectionMode::AutoPoll,
        );
        drop(watchlist);

        assert_eq!(selection.condition_ids.len(), 2);
        assert!(selection.condition_ids.contains(&"0xCOND-BTC".to_string()));
        assert!(selection.condition_ids.contains(&"0xCOND-ETH".to_string()));

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_closed_event(btc_yes.id()),
        );
        let watchlist = ctx.resolve_poll_watchlist.load();
        assert_eq!(watchlist.len(), 2);
        let selection = collect_resolve_watch_selection(
            &watchlist,
            now_ns,
            10,
            1800,
            ResolveWatchSelectionMode::AutoPoll,
        );
        drop(watchlist);

        assert_eq!(selection.condition_ids.len(), 2);
        assert!(selection.condition_ids.contains(&"0xCOND-BTC".to_string()));
        assert!(selection.condition_ids.contains(&"0xCOND-ETH".to_string()));

        update_resolve_watchlist_from_position_event(
            &ctx.resolve_poll_watchlist,
            &ctx.instruments,
            &stub_position_closed_event(btc_no.id()),
        );
        let watchlist = ctx.resolve_poll_watchlist.load();
        assert_eq!(watchlist.len(), 1);
        let selection = collect_resolve_watch_selection(
            &watchlist,
            now_ns,
            10,
            1800,
            ResolveWatchSelectionMode::AutoPoll,
        );

        assert_eq!(selection.condition_ids, vec!["0xCOND-ETH".to_string()]);
    }

    #[rstest]
    fn resolve_watch_selection_auto_poll_pauses_timed_out_entries() {
        let instrument_id = InstrumentId::from("0xTOKEN_TIMEOUT.POLYMARKET");
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let now_ns = UnixNanos::from(1_812_000_000_000);

        let mut watchlist = AHashMap::new();
        watchlist.insert(
            instrument_id,
            ResolveWatchEntry {
                condition_id: Some("0xCOND-TIMEOUT".to_string()),
                expiration_ns: Some(expiration_ns),
                auto_poll_active: true,
            },
        );

        let selection = collect_resolve_watch_selection(
            &watchlist,
            now_ns,
            10,
            1800,
            ResolveWatchSelectionMode::AutoPoll,
        );

        assert!(selection.condition_ids.is_empty());
        assert_eq!(selection.timed_out_watchlist, 1);
        assert_eq!(selection.pause_auto_poll, vec![instrument_id]);
        assert_eq!(selection.paused_watchlist, 0);
    }

    #[rstest]
    fn resolve_watch_selection_manual_fallback_includes_paused_entries() {
        let instrument_id = InstrumentId::from("0xTOKEN_PAUSED.POLYMARKET");
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let now_ns = UnixNanos::from(1_812_000_000_000);

        let mut watchlist = AHashMap::new();
        watchlist.insert(
            instrument_id,
            ResolveWatchEntry {
                condition_id: Some("0xCOND-PAUSED".to_string()),
                expiration_ns: Some(expiration_ns),
                auto_poll_active: false,
            },
        );

        let selection = collect_resolve_watch_selection(
            &watchlist,
            now_ns,
            10,
            1800,
            ResolveWatchSelectionMode::ManualFallback,
        );

        assert_eq!(selection.condition_ids, vec!["0xCOND-PAUSED".to_string()]);
        assert_eq!(selection.timed_out_watchlist, 1);
        assert!(selection.pause_auto_poll.is_empty());
        assert_eq!(selection.paused_watchlist, 1);
    }

    #[rstest]
    fn market_resolved_removes_targets_from_resolve_watchlist() {
        let market = "0xMARKET-WATCHLIST";
        let winner_asset = "0xTOKEN_WATCH_WIN";
        let loser_asset = "0xTOKEN_WATCH_LOSE";

        let (ctx, _data_rx) = make_ws_ctx();
        let winner = seed_instrument(
            &ctx,
            winner_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );
        let loser = seed_instrument(
            &ctx,
            loser_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
        );
        ctx.resolve_poll_watchlist
            .insert(winner.id(), build_resolve_watch_entry(&winner));
        ctx.resolve_poll_watchlist
            .insert(loser.id(), build_resolve_watch_entry(&loser));

        let resolved = make_market_resolved(market, vec![winner_asset, loser_asset], winner_asset);
        PolymarketDataClient::handle_market_message(resolved, &ctx);

        assert!(!ctx.resolve_poll_watchlist.contains_key(&winner.id()));
        assert!(!ctx.resolve_poll_watchlist.contains_key(&loser.id()));
    }

    #[rstest]
    fn market_resolved_condition_id_fallback_maps_without_assets_ids() {
        let condition_id = "0xCOND-RESOLVE";
        let winner_asset = "0xTOKEN_WIN2";
        let loser_asset = "0xTOKEN_LOSE2";

        let (ctx, mut data_rx) = make_ws_ctx();
        let winner = seed_instrument_with_context(
            &ctx,
            winner_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
            None,
            None,
            Some(condition_id),
        );
        let loser = seed_instrument_with_context(
            &ctx,
            loser_asset,
            Price::from("0.001"),
            Quantity::from("0.01"),
            None,
            None,
            Some(condition_id),
        );

        let resolved = make_market_resolved_with_slug(condition_id, None, vec![], winner_asset);
        PolymarketDataClient::handle_market_message(resolved, &ctx);

        let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
        let status_count = events
            .iter()
            .filter(|e| matches!(e, DataEvent::InstrumentStatus(_)))
            .count();
        assert_eq!(
            status_count, 2,
            "expected 2 status events, found: {events:?}"
        );

        let mut winner_close = None;
        let mut loser_close = None;

        for event in events {
            if let DataEvent::Data(NautilusData::InstrumentClose(close)) = event {
                if close.instrument_id == winner.id() {
                    winner_close = Some(close);
                } else if close.instrument_id == loser.id() {
                    loser_close = Some(close);
                }
            }
        }

        let winner_close = winner_close.expect("expected winner close event");
        let loser_close = loser_close.expect("expected loser close event");
        assert_eq!(
            winner_close.close_type,
            InstrumentCloseType::ContractExpired
        );
        assert_eq!(loser_close.close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(winner_close.close_price.as_f64(), 1.0);
        assert_eq!(loser_close.close_price.as_f64(), 0.0);
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
