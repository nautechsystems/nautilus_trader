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

use nautilus_core::{AtomicMap, AtomicSet};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

pub(super) fn resolve_token_id_from(
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
pub(super) async fn sync_ws_subscription_async(
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
