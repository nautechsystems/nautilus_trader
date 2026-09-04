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

//! Effective order book snapshot state transitions.

use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    orderbook::{BookIntegrityError, BookLevel, OrderBook},
    types::{Price, Quantity},
};

pub(super) fn apply_snapshot_and_diff(
    book: &mut OrderBook,
    snapshot: &OrderBookDeltas,
) -> Result<Option<OrderBookDeltas>, BookIntegrityError> {
    if snapshot.instrument_id != book.instrument_id {
        return Err(BookIntegrityError::InstrumentMismatch(
            book.instrument_id,
            snapshot.instrument_id,
        ));
    }

    if book.book_type != BookType::L2_MBP
        || snapshot
            .deltas
            .iter()
            .any(|delta| delta.action != BookAction::Clear && delta.order.side.is_none())
    {
        return apply_snapshot_and_diff_fallback(book, snapshot);
    }

    let old_bids = level_sizes(book.bids(None));
    let old_asks = level_sizes(book.asks(None));

    book.apply_deltas_unchecked(snapshot)?;

    Ok(compute_effective_deltas(
        book,
        old_bids,
        old_asks,
        snapshot.ts_event,
        snapshot.ts_init,
    ))
}

fn apply_snapshot_and_diff_fallback(
    book: &mut OrderBook,
    snapshot: &OrderBookDeltas,
) -> Result<Option<OrderBookDeltas>, BookIntegrityError> {
    let old_bids = level_sizes(book.bids(None));
    let old_asks = level_sizes(book.asks(None));
    let mut book_new = book.clone();
    book_new.apply_deltas(snapshot)?;

    let effective = compute_effective_deltas(
        &book_new,
        old_bids,
        old_asks,
        snapshot.ts_event,
        snapshot.ts_init,
    );
    *book = book_new;

    Ok(effective)
}

fn compute_effective_deltas(
    book_new: &OrderBook,
    mut old_bids: IndexMap<Price, (Quantity, usize)>,
    mut old_asks: IndexMap<Price, (Quantity, usize)>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Option<OrderBookDeltas> {
    let instrument_id = book_new.instrument_id;
    let mut deltas = Vec::with_capacity(old_bids.len() + old_asks.len());

    diff_new_levels(
        &mut deltas,
        book_new.bids(None),
        &mut old_bids,
        instrument_id,
        OrderSide::Buy,
        ts_event,
        ts_init,
    );
    diff_new_levels(
        &mut deltas,
        book_new.asks(None),
        &mut old_asks,
        instrument_id,
        OrderSide::Sell,
        ts_event,
        ts_init,
    );

    append_deletes(
        &mut deltas,
        old_bids,
        instrument_id,
        OrderSide::Buy,
        ts_event,
        ts_init,
    );
    append_deletes(
        &mut deltas,
        old_asks,
        instrument_id,
        OrderSide::Sell,
        ts_event,
        ts_init,
    );

    if deltas.is_empty() {
        return None;
    }

    deltas.last_mut().expect("deltas not empty").flags |= RecordFlag::F_LAST as u8;

    Some(OrderBookDeltas::new(instrument_id, deltas))
}

fn level_sizes<'a>(
    levels: impl Iterator<Item = &'a BookLevel>,
) -> IndexMap<Price, (Quantity, usize)> {
    levels
        .enumerate()
        .filter_map(|(index, level)| {
            level
                .first()
                .map(|order| (order.price, (order.size, index)))
        })
        .collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "the diff needs both book state and output metadata"
)]
fn diff_new_levels<'a>(
    deltas: &mut Vec<OrderBookDelta>,
    new_levels: impl Iterator<Item = &'a BookLevel>,
    old_sizes: &mut IndexMap<Price, (Quantity, usize)>,
    instrument_id: InstrumentId,
    side: OrderSide,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    for level in new_levels {
        let Some(order) = level.first() else {
            continue;
        };

        let action = match old_sizes.swap_remove(&order.price) {
            Some((old_size, _)) if old_size != order.size => BookAction::Update,
            Some(_) => continue,
            None => BookAction::Add,
        };

        deltas.push(effective_delta(
            instrument_id,
            action,
            side,
            order.price,
            order.size,
            ts_event,
            ts_init,
        ));
    }
}

fn append_deletes(
    deltas: &mut Vec<OrderBookDelta>,
    old_sizes: IndexMap<Price, (Quantity, usize)>,
    instrument_id: InstrumentId,
    side: OrderSide,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    let mut removed: Vec<_> = old_sizes
        .into_iter()
        .map(|(price, (size, index))| (index, price, size))
        .collect();
    removed.sort_unstable_by_key(|(index, _, _)| *index);

    for (_, price, size) in removed {
        deltas.push(effective_delta(
            instrument_id,
            BookAction::Delete,
            side,
            price,
            size,
            ts_event,
            ts_init,
        ));
    }
}

fn effective_delta(
    instrument_id: InstrumentId,
    action: BookAction,
    side: OrderSide,
    price: Price,
    size: Quantity,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    OrderBookDelta::new(
        instrument_id,
        action,
        BookOrder::new(side, price, size, 0),
        0,
        0,
        ts_event,
        ts_init,
    )
}
