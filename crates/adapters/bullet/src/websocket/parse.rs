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

//! Parsing helpers: Bullet `ServerMessage` wire types → Nautilus model types.

use anyhow::Context;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{
        BookOrder, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::{
    common::enums::BulletMessageType,
    websocket::messages::{
        AggTradeUpdate, BookTickerUpdate, DepthUpdate,
        MarkPriceUpdate as BulletMarkPriceUpdate,
    },
};

/// Convert a millisecond timestamp to nanoseconds.
#[inline]
pub fn millis_to_nanos(ms: i64) -> UnixNanos {
    UnixNanos::from(ms.max(0) as u64 * 1_000_000)
}

/// Parse a `BookTickerUpdate` into a [`QuoteTick`].
pub fn book_ticker_to_quote(
    msg: &BookTickerUpdate,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let pp = instrument.price_precision();
    let sp = instrument.size_precision();

    let bid_price = Price::from_decimal_dp(msg.bid_price, pp)
        .with_context(|| format!("bid_price '{}' overflows p={pp}", msg.bid_price))?;
    let ask_price = Price::from_decimal_dp(msg.ask_price, pp)
        .with_context(|| format!("ask_price '{}' overflows p={pp}", msg.ask_price))?;
    let bid_size = Quantity::from_decimal_dp(msg.bid_qty, sp)
        .with_context(|| format!("bid_qty '{}' overflows p={sp}", msg.bid_qty))?;
    let ask_size = Quantity::from_decimal_dp(msg.ask_qty, sp)
        .with_context(|| format!("ask_qty '{}' overflows p={sp}", msg.ask_qty))?;

    let ts_event = millis_to_nanos(msg.transaction_time);

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("failed to construct QuoteTick from BookTicker")
}

/// Parse an `AggTradeUpdate` into a [`TradeTick`].
pub fn agg_trade_to_trade(
    msg: &AggTradeUpdate,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let pp = instrument.price_precision();
    let sp = instrument.size_precision();

    let price = Price::from_decimal_dp(msg.price, pp)
        .with_context(|| format!("price '{}' overflows p={pp}", msg.price))?;
    let size = Quantity::from_decimal_dp(msg.quantity, sp)
        .with_context(|| format!("quantity '{}' overflows p={sp}", msg.quantity))?;

    // is_buyer_maker=true → buyer was the passive side → seller was the aggressor
    let aggressor = if msg.is_buyer_maker {
        AggressorSide::Seller
    } else {
        AggressorSide::Buyer
    };

    let trade_id = TradeId::new_checked(msg.agg_trade_id.to_string())
        .context("invalid trade_id in AggTrade")?;

    let ts_event = millis_to_nanos(msg.trade_time);

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("failed to construct TradeTick from AggTrade")
}

/// Parse a `DepthUpdate` into [`OrderBookDeltas`].
///
/// Snapshot frames (`mt: "s"`) emit a `Clear` delta before all `Add` levels.
/// Incremental frames (`mt: "u"`) emit `Update` or `Delete` (qty = 0) per level.
pub fn depth_to_deltas(
    msg: &DepthUpdate,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let pp = instrument.price_precision();
    let sp = instrument.size_precision();
    let ts_event = millis_to_nanos(msg.event_time);
    let id = instrument.id();
    let seq = msg.update_id;

    let is_snapshot = msg.mt == BulletMessageType::Snapshot;
    let mut deltas: Vec<OrderBookDelta> = Vec::with_capacity(
        msg.bids.len() + msg.asks.len() + if is_snapshot { 1 } else { 0 },
    );

    if is_snapshot {
        deltas.push(OrderBookDelta::clear(id, seq, ts_event, ts_init));
    }

    let add_action = if is_snapshot { BookAction::Add } else { BookAction::Update };

    for [price_str, qty_str] in &msg.bids {
        let price_dec = Decimal::from_str(price_str)
            .with_context(|| format!("bid price parse: '{price_str}'"))?;
        let qty_dec = Decimal::from_str(qty_str)
            .with_context(|| format!("bid qty parse: '{qty_str}'"))?;

        let price = Price::from_decimal_dp(price_dec, pp)
            .with_context(|| format!("bid price '{price_dec}' overflows p={pp}"))?;

        let (action, size) = if qty_dec.is_zero() {
            (BookAction::Delete, Quantity::new(0.0, sp))
        } else {
            let s = Quantity::from_decimal_dp(qty_dec, sp)
                .with_context(|| format!("bid qty '{qty_dec}' overflows p={sp}"))?;
            (add_action, s)
        };

        let order = BookOrder::new(OrderSide::Buy, price, size, 0);
        deltas.push(OrderBookDelta::new(
            id,
            action,
            order,
            RecordFlag::F_LAST as u8,
            seq,
            ts_event,
            ts_init,
        ));
    }

    for [price_str, qty_str] in &msg.asks {
        let price_dec = Decimal::from_str(price_str)
            .with_context(|| format!("ask price parse: '{price_str}'"))?;
        let qty_dec = Decimal::from_str(qty_str)
            .with_context(|| format!("ask qty parse: '{qty_str}'"))?;

        let price = Price::from_decimal_dp(price_dec, pp)
            .with_context(|| format!("ask price '{price_dec}' overflows p={pp}"))?;

        let (action, size) = if qty_dec.is_zero() {
            (BookAction::Delete, Quantity::new(0.0, sp))
        } else {
            let s = Quantity::from_decimal_dp(qty_dec, sp)
                .with_context(|| format!("ask qty '{qty_dec}' overflows p={sp}"))?;
            (add_action, s)
        };

        let order = BookOrder::new(OrderSide::Sell, price, size, 0);
        deltas.push(OrderBookDelta::new(
            id,
            action,
            order,
            RecordFlag::F_LAST as u8,
            seq,
            ts_event,
            ts_init,
        ));
    }

    OrderBookDeltas::new_checked(id, deltas).context("failed to construct OrderBookDeltas")
}

/// Parse a `MarkPriceUpdate` into a Nautilus [`MarkPriceUpdate`].
pub fn mark_price_to_update(
    msg: &BulletMarkPriceUpdate,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<MarkPriceUpdate> {
    let value = Price::from_decimal_dp(msg.mark_price, price_precision)
        .with_context(|| {
            format!("mark_price '{}' overflows p={price_precision}", msg.mark_price)
        })?;
    let ts_event = millis_to_nanos(msg.event_time);

    Ok(MarkPriceUpdate::new(instrument_id, value, ts_event, ts_init))
}
