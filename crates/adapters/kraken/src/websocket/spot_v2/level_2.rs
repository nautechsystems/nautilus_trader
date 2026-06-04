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

//! Runtime state for Kraken Spot L2 book handling.

use std::sync::Arc;

use ahash::AHashMap;
use nautilus_core::{AtomicMap, UnixNanos};
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, BookType, RecordFlag},
    identifiers::InstrumentId,
    instruments::{Instrument, any::InstrumentAny},
    orderbook::OrderBook,
};

use super::{messages::KrakenWsBookData, parse::parse_book_deltas};

#[derive(Debug, Clone)]
pub(crate) struct L2Depths {
    depths: Arc<AtomicMap<String, u32>>,
}

impl Default for L2Depths {
    fn default() -> Self {
        Self {
            depths: Arc::new(AtomicMap::new()),
        }
    }
}

impl L2Depths {
    pub(crate) fn get(&self, symbol: &str) -> Option<u32> {
        self.depths.load().get(symbol).copied()
    }

    pub(crate) fn insert(&self, symbol: &str, depth: u32) {
        self.depths.insert(symbol.to_string(), depth);
    }

    pub(crate) fn remove(&self, symbol: &str) {
        self.depths.rcu(|depths| {
            depths.remove(symbol);
        });
    }

    pub(crate) fn clear(&self) {
        self.depths.store(AHashMap::new());
    }
}

#[derive(Debug, Default)]
pub(crate) struct L2BookState {
    pub(crate) books: AHashMap<InstrumentId, OrderBook>,
}

impl L2BookState {
    pub(crate) fn process_book(
        &mut self,
        book: &KrakenWsBookData,
        instrument: &InstrumentAny,
        sequence: u64,
        is_snapshot: bool,
        depth: Option<u32>,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Option<(OrderBookDeltas, u64)>> {
        let instrument_id = instrument.id();
        let mut deltas = parse_book_deltas(book, instrument, sequence, is_snapshot, ts_init)?;
        if deltas.is_empty() {
            return Ok(None);
        }

        let mut next_sequence = sequence + deltas.len() as u64;
        let book_state = self
            .books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

        if let Err(e) =
            book_state.apply_deltas(&OrderBookDeltas::new(instrument_id, deltas.clone()))
        {
            log::error!("Failed to apply Kraken L2 deltas to shadow book: {e}");
        } else if let Some(depth) = depth {
            prune_deltas_to_depth(
                book_state,
                depth,
                is_snapshot,
                &mut next_sequence,
                ts_init,
                &mut deltas,
            );
        }

        set_last_delta_flag(&mut deltas);
        Ok(Some((
            OrderBookDeltas::new(instrument_id, deltas),
            next_sequence,
        )))
    }
}

fn prune_deltas_to_depth(
    book: &mut OrderBook,
    depth: u32,
    is_snapshot: bool,
    next_sequence: &mut u64,
    ts_init: UnixNanos,
    deltas: &mut Vec<OrderBookDelta>,
) {
    if depth == 0 {
        return;
    }

    let prune_orders: Vec<BookOrder> = book
        .bids(None)
        .skip(depth as usize)
        .chain(book.asks(None).skip(depth as usize))
        .filter_map(|level| level.first().copied())
        .collect();

    if prune_orders.is_empty() {
        return;
    }

    let ts_event = deltas.last().map_or(ts_init, |delta| delta.ts_event);
    let mut flags = RecordFlag::F_MBP as u8;
    if is_snapshot {
        flags |= RecordFlag::F_SNAPSHOT as u8;
    }

    for order in prune_orders {
        let delta = OrderBookDelta::new(
            book.instrument_id,
            BookAction::Delete,
            order,
            flags,
            *next_sequence,
            ts_event,
            ts_init,
        );
        *next_sequence += 1;

        if let Err(e) = book.apply_delta(&delta) {
            log::error!("Failed to apply Kraken L2 depth prune delta to shadow book: {e}");
            continue;
        }

        deltas.push(delta);
    }
}

fn set_last_delta_flag(deltas: &mut [OrderBookDelta]) {
    for delta in deltas.iter_mut() {
        delta.flags &= !(RecordFlag::F_LAST as u8);
    }

    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }
}
