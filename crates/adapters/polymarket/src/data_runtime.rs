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

//! Shared runtime helpers for the Polymarket data client.

use std::sync::{Arc, Mutex as StdMutex};

use ahash::AHashSet;
use dashmap::DashMap;
use nautilus_core::{AtomicMap, AtomicSet, UnixNanos};
use nautilus_model::{
    data::QuoteTick,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use ustr::Ustr;

use crate::resolve::ResolveWatchEntry;

#[derive(Clone, Copy, Debug)]
pub(crate) struct TokenMeta {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) price_precision: u8,
    pub(crate) size_precision: u8,
}

pub(crate) fn resolve_token_id_from(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument_id: InstrumentId,
) -> anyhow::Result<String> {
    let loaded = instruments.load();
    let instrument = loaded
        .get(&instrument_id)
        .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
    Ok(instrument.raw_symbol().as_str().to_string())
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared state comes in as Arc refs"
)]
pub(crate) async fn sync_ws_subscription_async(
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
            ws_open_tokens.remove(&token_id);
        }
    } else if !wants_subscribe && is_open {
        ws_open_tokens.remove(&token_id);

        if let Err(e) = ws.unsubscribe_market(vec![token_id_str]).await {
            log::error!("Failed to unsubscribe from market data: {e:?}");
        }
    }
}

pub(crate) fn is_instrument_expired(instrument: &InstrumentAny, now_ns: UnixNanos) -> bool {
    instrument
        .expiration_ns()
        .is_some_and(|expiration_ns| expiration_ns.as_u64() != 0 && expiration_ns <= now_ns)
}

pub(crate) fn seed_token_meta_from_live_instruments(
    now_ns: UnixNanos,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
) {
    let loaded = instruments.load();

    for instrument in loaded.values() {
        if is_instrument_expired(instrument, now_ns) {
            continue;
        }

        token_meta.insert(
            Ustr::from(instrument.raw_symbol().as_str()),
            TokenMeta {
                instrument_id: instrument.id(),
                price_precision: instrument.price_precision(),
                size_precision: instrument.size_precision(),
            },
        );
    }
}

fn is_watchlisted_instrument(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    instrument_id: InstrumentId,
) -> bool {
    let snapshot = watchlist.load();
    snapshot.values().any(|entry| {
        entry
            .tracked
            .values()
            .any(|tracked| tracked.instrument_id == instrument_id)
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared adapter state is held in Arcs"
)]
pub(crate) async fn retire_local_instrument_state(
    instrument_id: InstrumentId,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    order_books: &Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: &Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: &Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: &Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: &Arc<AtomicSet<InstrumentId>>,
    resolve_poll_watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    pending_snapshot_after_tick_change: &Arc<AtomicSet<InstrumentId>>,
    pending_auto_loads: &Arc<StdMutex<AHashSet<InstrumentId>>>,
    ws_open_tokens: &Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: &Arc<tokio::sync::Mutex<()>>,
    ws: &crate::websocket::client::WsSubscriptionHandle,
) {
    let token_id = resolve_token_id_from(instruments, instrument_id).ok();
    let keep_local_metadata = is_watchlisted_instrument(resolve_poll_watchlist, instrument_id);

    active_quote_subs.remove(&instrument_id);
    active_delta_subs.remove(&instrument_id);
    active_trade_subs.remove(&instrument_id);

    if let Some(token_id) = token_id.as_ref() {
        sync_ws_subscription_async(
            instrument_id,
            token_id.clone(),
            active_quote_subs.clone(),
            active_delta_subs.clone(),
            active_trade_subs.clone(),
            ws_open_tokens.clone(),
            ws_sub_mutex.clone(),
            ws.clone(),
        )
        .await;
    }

    pending_snapshot_after_tick_change.remove(&instrument_id);
    {
        let mut pending = pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned");
        pending.remove(&instrument_id);
    }

    order_books.remove(&instrument_id);
    last_quotes.remove(&instrument_id);

    if let Some(token_id) = token_id {
        token_meta.remove(&Ustr::from(token_id.as_str()));
    }

    if !keep_local_metadata {
        instruments.remove(&instrument_id);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared adapter state is held in Arcs"
)]
pub(crate) async fn retire_expired_local_instruments(
    now_ns: UnixNanos,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    order_books: &Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: &Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: &Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: &Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: &Arc<AtomicSet<InstrumentId>>,
    resolve_poll_watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    pending_snapshot_after_tick_change: &Arc<AtomicSet<InstrumentId>>,
    pending_auto_loads: &Arc<StdMutex<AHashSet<InstrumentId>>>,
    ws_open_tokens: &Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: &Arc<tokio::sync::Mutex<()>>,
    ws: &crate::websocket::client::WsSubscriptionHandle,
) {
    let expired_ids: Vec<InstrumentId> = {
        let loaded = instruments.load();
        loaded
            .iter()
            .filter_map(|(instrument_id, instrument)| {
                is_instrument_expired(instrument, now_ns).then_some(*instrument_id)
            })
            .collect()
    };

    for instrument_id in expired_ids {
        retire_local_instrument_state(
            instrument_id,
            instruments,
            token_meta,
            order_books,
            last_quotes,
            active_quote_subs,
            active_delta_subs,
            active_trade_subs,
            resolve_poll_watchlist,
            pending_snapshot_after_tick_change,
            pending_auto_loads,
            ws_open_tokens,
            ws_sub_mutex,
            ws,
        )
        .await;
    }
}
