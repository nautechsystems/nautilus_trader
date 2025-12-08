// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Order book utilities shared by REST + WebSocket code paths.

use anyhow::{Context, Result};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick},
    enums::{BookAction, OrderSide, RecordFlag},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::models::LighterOrderBookDepth;

/// Convert a depth snapshot/delta payload into Nautilus order book deltas and best bid/ask quote.
///
/// The returned [`OrderBookDeltas`] always begins with a `clear` delta, allowing callers to
/// rebuild deterministic state from either REST snapshots or WebSocket subscription payloads.
pub fn depth_to_deltas_and_quote(
    depth: &LighterOrderBookDepth,
    instrument: &InstrumentAny,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Result<(OrderBookDeltas, Option<QuoteTick>)> {
    let sequence = depth.offset.unwrap_or_default();
    let mut deltas = Vec::with_capacity(depth.bids.len() + depth.asks.len() + 1);
    deltas.push(OrderBookDelta::clear(
        instrument.id(),
        sequence,
        ts_event,
        ts_init,
    ));

    let mut best_bid: Option<(Price, Quantity)> = None;
    let mut best_ask: Option<(Price, Quantity)> = None;

    for level in &depth.bids {
        let (price, size) = parse_level(level.price, level.size, instrument, "bid")?;
        if size.is_zero() {
            deltas.push(make_delta(
                instrument,
                OrderSide::Buy,
                BookAction::Delete,
                price,
                size,
                sequence,
                ts_event,
                ts_init,
            ));
            continue;
        }

        if best_bid
            .as_ref()
            .map_or(true, |(best_price, _)| price > *best_price)
        {
            best_bid = Some((price, size));
        }

        deltas.push(make_delta(
            instrument,
            OrderSide::Buy,
            BookAction::Add,
            price,
            size,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    for level in &depth.asks {
        let (price, size) = parse_level(level.price, level.size, instrument, "ask")?;
        if size.is_zero() {
            deltas.push(make_delta(
                instrument,
                OrderSide::Sell,
                BookAction::Delete,
                price,
                size,
                sequence,
                ts_event,
                ts_init,
            ));
            continue;
        }

        if best_ask
            .as_ref()
            .map_or(true, |(best_price, _)| price < *best_price)
        {
            best_ask = Some((price, size));
        }

        deltas.push(make_delta(
            instrument,
            OrderSide::Sell,
            BookAction::Add,
            price,
            size,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    let quote = match (best_bid, best_ask) {
        (Some((bid_price, bid_size)), Some((ask_price, ask_size))) => Some(QuoteTick::new(
            instrument.id(),
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        )),
        _ => None,
    };

    Ok((OrderBookDeltas::new(instrument.id(), deltas), quote))
}

fn parse_level(
    price: Decimal,
    size: Decimal,
    instrument: &InstrumentAny,
    field: &str,
) -> Result<(Price, Quantity)> {
    let price_f64 = price
        .to_f64()
        .with_context(|| format!("invalid {field} price {price}"))?;
    let size_f64 = size
        .abs()
        .to_f64()
        .with_context(|| format!("invalid {field} size {size}"))?;

    Ok((
        Price::new(price_f64, instrument.price_precision()),
        Quantity::new(size_f64, instrument.size_precision()),
    ))
}

fn make_delta(
    instrument: &InstrumentAny,
    side: OrderSide,
    action: BookAction,
    price: Price,
    size: Quantity,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    let order = BookOrder::new(side, price, size, 0);
    OrderBookDelta::new(
        instrument.id(),
        action,
        order,
        RecordFlag::F_LAST as u8,
        sequence,
        ts_event,
        ts_init,
    )
}
