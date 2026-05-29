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

//! WebSocket message dispatch context for the Derive data client.

use std::sync::Arc;

use nautilus_common::{cache::quote::QuoteCache, messages::DataEvent};
use nautilus_core::{AtomicMap, AtomicSet, time::AtomicTime};
use nautilus_model::{identifiers::InstrumentId, instruments::InstrumentAny};

pub(crate) struct WsMessageContext {
    pub(crate) clock: &'static AtomicTime,
    pub(crate) data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    pub(crate) instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    pub(crate) active_book_delta_channels: Arc<AtomicMap<InstrumentId, String>>,
    pub(crate) active_book_depth10_channels: Arc<AtomicMap<InstrumentId, String>>,
    pub(crate) active_ticker_channels: Arc<AtomicMap<InstrumentId, String>>,
    pub(crate) active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) active_mark_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) active_index_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) active_funding_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) active_greeks_subs: Arc<AtomicSet<InstrumentId>>,
    pub(crate) quote_cache: QuoteCache,
}
