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

use std::{ffi::c_char, num::NonZeroUsize};

use databento::dbn;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, DEPTH10_LEN, Data, InstrumentStatus,
        OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{AggregationSource, BarAggregation, FromU16, MarketStatusAction, OrderSide, PriceType},
    identifiers::{InstrumentId, TradeId},
};

use super::primitives::{
    decode_price_or_undef, decode_quantity, parse_aggressor_side, parse_book_action,
    parse_optional_bool, parse_order_side, parse_status_reason, parse_status_trading_event,
};

const STEP_ONE: NonZeroUsize = NonZeroUsize::new(1).unwrap();

const BAR_SPEC_1S: BarSpecification = BarSpecification {
    step: STEP_ONE,
    aggregation: BarAggregation::Second,
    price_type: PriceType::Last,
};
const BAR_SPEC_1M: BarSpecification = BarSpecification {
    step: STEP_ONE,
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
const BAR_SPEC_1H: BarSpecification = BarSpecification {
    step: STEP_ONE,
    aggregation: BarAggregation::Hour,
    price_type: PriceType::Last,
};
const BAR_SPEC_1D: BarSpecification = BarSpecification {
    step: STEP_ONE,
    aggregation: BarAggregation::Day,
    price_type: PriceType::Last,
};

pub(super) const BAR_CLOSE_ADJUSTMENT_1S: u64 = NANOSECONDS_IN_SECOND;
pub(super) const BAR_CLOSE_ADJUSTMENT_1M: u64 = NANOSECONDS_IN_SECOND * 60;
pub(super) const BAR_CLOSE_ADJUSTMENT_1H: u64 = NANOSECONDS_IN_SECOND * 60 * 60;
pub(super) const BAR_CLOSE_ADJUSTMENT_1D: u64 = NANOSECONDS_IN_SECOND * 60 * 60 * 24;

// FNV-1a 64-bit constants (see http://www.isthe.com/chongo/tech/comp/fnv/).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn fnv1a_mix(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(FNV_PRIME);
}

/// Derives a deterministic [`TradeId`] for Databento schemas that do not
/// publish a native trade identifier (e.g. CMBP1, TCBBO).
///
/// The hash combines the instrument, timestamps, price, size and aggressor
/// side so that replayed data yields the same identifier across runs.
pub(super) fn derive_cmbp_trade_id(
    instrument_id: InstrumentId,
    ts_event: u64,
    ts_recv: u64,
    price: i64,
    size: u32,
    side: c_char,
) -> TradeId {
    let mut hash: u64 = FNV_OFFSET_BASIS;
    fnv1a_mix(&mut hash, instrument_id.to_string().as_bytes());
    fnv1a_mix(&mut hash, &ts_event.to_le_bytes());
    fnv1a_mix(&mut hash, &ts_recv.to_le_bytes());
    fnv1a_mix(&mut hash, &price.to_le_bytes());
    fnv1a_mix(&mut hash, &size.to_le_bytes());
    fnv1a_mix(&mut hash, &[side as u8]);
    TradeId::new(format!("{hash:016x}"))
}

#[inline(always)]
#[must_use]
pub(super) fn is_trade_msg(action: c_char) -> bool {
    action as u8 as char == 'T'
}

/// Returns `true` if both bid and ask prices are defined (not `i64::MAX`).
///
/// Databento uses `i64::MAX` as a sentinel value for undefined/null prices.
/// A valid quote requires both sides to be defined.
#[inline(always)]
#[must_use]
fn has_valid_bid_ask(bid_px: i64, ask_px: i64) -> bool {
    bid_px != i64::MAX && ask_px != i64::MAX
}

/// Decodes a Databento MBO (Market by Order) message into an order book delta or trade.
///
/// Returns a tuple containing either an `OrderBookDelta` or a `TradeTick`, depending on
/// whether the message represents an order book update or a trade execution.
///
/// # Errors
///
/// Returns an error if decoding the MBO message fails.
pub fn decode_mbo_msg(
    msg: &dbn::MboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
    include_trades: bool,
) -> anyhow::Result<(Option<OrderBookDelta>, Option<TradeTick>)> {
    let side = parse_order_side(msg.side);
    if is_trade_msg(msg.action) {
        if include_trades && msg.size > 0 {
            let price = decode_price_or_undef(msg.price, price_precision);
            let size = decode_quantity(msg.size as u64);
            let aggressor_side = parse_aggressor_side(msg.side);
            let trade_id = TradeId::new(itoa::Buffer::new().format(msg.sequence));
            let ts_event = msg.ts_recv.into();
            let ts_init = ts_init.unwrap_or(ts_event);

            let trade = TradeTick::new(
                instrument_id,
                price,
                size,
                aggressor_side,
                trade_id,
                ts_event,
                ts_init,
            );
            return Ok((None, Some(trade)));
        }

        return Ok((None, None));
    }

    let action = parse_book_action(msg.action)?;
    let price = decode_price_or_undef(msg.price, price_precision);
    let size = decode_quantity(msg.size as u64);
    let order = BookOrder::new(side, price, size, msg.order_id);

    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let delta = OrderBookDelta::new(
        instrument_id,
        action,
        order,
        msg.flags.raw(),
        msg.sequence.into(),
        ts_event,
        ts_init,
    );

    Ok((Some(delta), None))
}

/// Decodes a Databento Trade message into a `TradeTick`.
///
/// # Errors
///
/// Returns an error if decoding the Trade message fails.
pub fn decode_trade_msg(
    msg: &dbn::TradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<TradeTick> {
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let trade = TradeTick::new(
        instrument_id,
        decode_price_or_undef(msg.price, price_precision),
        decode_quantity(msg.size as u64),
        parse_aggressor_side(msg.side),
        TradeId::new(itoa::Buffer::new().format(msg.sequence)),
        ts_event,
        ts_init,
    );

    Ok(trade)
}

/// Decodes a Databento TBBO (Top of Book with Trade) message into quote and trade ticks.
///
/// Returns `None` for the quote if either bid or ask price is undefined (`i64::MAX`).
/// The trade is always returned.
///
/// # Errors
///
/// Returns an error if decoding the TBBO message fails.
pub fn decode_tbbo_msg(
    msg: &dbn::TbboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<(Option<QuoteTick>, TradeTick)> {
    let top_level = &msg.levels[0];
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let maybe_quote = if has_valid_bid_ask(top_level.bid_px, top_level.ask_px) {
        Some(QuoteTick::new(
            instrument_id,
            decode_price_or_undef(top_level.bid_px, price_precision),
            decode_price_or_undef(top_level.ask_px, price_precision),
            decode_quantity(top_level.bid_sz as u64),
            decode_quantity(top_level.ask_sz as u64),
            ts_event,
            ts_init,
        ))
    } else {
        None
    };

    let trade = TradeTick::new(
        instrument_id,
        decode_price_or_undef(msg.price, price_precision),
        decode_quantity(msg.size as u64),
        parse_aggressor_side(msg.side),
        TradeId::new(itoa::Buffer::new().format(msg.sequence)),
        ts_event,
        ts_init,
    );

    Ok((maybe_quote, trade))
}

/// Decodes a Databento MBP1 (Market by Price Level 1) message into quote and optional trade ticks.
///
/// Returns `None` for the quote if either bid or ask price is undefined (`i64::MAX`).
///
/// # Errors
///
/// Returns an error if decoding the MBP1 message fails.
pub fn decode_mbp1_msg(
    msg: &dbn::Mbp1Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
    include_trades: bool,
) -> anyhow::Result<(Option<QuoteTick>, Option<TradeTick>)> {
    let top_level = &msg.levels[0];
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let maybe_quote = if has_valid_bid_ask(top_level.bid_px, top_level.ask_px) {
        Some(QuoteTick::new(
            instrument_id,
            decode_price_or_undef(top_level.bid_px, price_precision),
            decode_price_or_undef(top_level.ask_px, price_precision),
            decode_quantity(top_level.bid_sz as u64),
            decode_quantity(top_level.ask_sz as u64),
            ts_event,
            ts_init,
        ))
    } else {
        None
    };

    let maybe_trade = if include_trades && is_trade_msg(msg.action) {
        Some(TradeTick::new(
            instrument_id,
            decode_price_or_undef(msg.price, price_precision),
            decode_quantity(msg.size as u64),
            parse_aggressor_side(msg.side),
            TradeId::new(itoa::Buffer::new().format(msg.sequence)),
            ts_event,
            ts_init,
        ))
    } else {
        None
    };

    Ok((maybe_quote, maybe_trade))
}

/// Decodes a Databento BBO (Best Bid and Offer) message into a `QuoteTick`.
///
/// Returns `None` if either bid or ask price is undefined (`i64::MAX`).
///
/// # Errors
///
/// Returns an error if decoding the BBO message fails.
pub fn decode_bbo_msg(
    msg: &dbn::BboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<Option<QuoteTick>> {
    let top_level = &msg.levels[0];
    if !has_valid_bid_ask(top_level.bid_px, top_level.ask_px) {
        return Ok(None);
    }

    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let quote = QuoteTick::new(
        instrument_id,
        decode_price_or_undef(top_level.bid_px, price_precision),
        decode_price_or_undef(top_level.ask_px, price_precision),
        decode_quantity(top_level.bid_sz as u64),
        decode_quantity(top_level.ask_sz as u64),
        ts_event,
        ts_init,
    );

    Ok(Some(quote))
}

/// Decodes a Databento MBP10 (Market by Price 10 levels) message into an `OrderBookDepth10`.
///
/// # Errors
///
/// Returns an error if the number of levels in `msg.levels` is not exactly `DEPTH10_LEN`.
pub fn decode_mbp10_msg(
    msg: &dbn::Mbp10Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<OrderBookDepth10> {
    let mut bids = Vec::with_capacity(DEPTH10_LEN);
    let mut asks = Vec::with_capacity(DEPTH10_LEN);
    let mut bid_counts = Vec::with_capacity(DEPTH10_LEN);
    let mut ask_counts = Vec::with_capacity(DEPTH10_LEN);

    for level in &msg.levels {
        // Empty Databento levels carry the i64::MAX sentinel; decode them as
        // NULL_ORDER so the matching engine's precision check skips them.
        let bid_order = if level.bid_px == i64::MAX {
            BookOrder::default()
        } else {
            BookOrder::new(
                OrderSide::Buy,
                decode_price_or_undef(level.bid_px, price_precision),
                decode_quantity(level.bid_sz as u64),
                0,
            )
        };

        let ask_order = if level.ask_px == i64::MAX {
            BookOrder::default()
        } else {
            BookOrder::new(
                OrderSide::Sell,
                decode_price_or_undef(level.ask_px, price_precision),
                decode_quantity(level.ask_sz as u64),
                0,
            )
        };

        bids.push(bid_order);
        asks.push(ask_order);
        bid_counts.push(level.bid_ct);
        ask_counts.push(level.ask_ct);
    }

    let bids: [BookOrder; DEPTH10_LEN] = bids.try_into().map_err(|v: Vec<BookOrder>| {
        anyhow::anyhow!(
            "Expected exactly {DEPTH10_LEN} bid levels, received {}",
            v.len()
        )
    })?;

    let asks: [BookOrder; DEPTH10_LEN] = asks.try_into().map_err(|v: Vec<BookOrder>| {
        anyhow::anyhow!(
            "Expected exactly {DEPTH10_LEN} ask levels, received {}",
            v.len()
        )
    })?;

    let bid_counts: [u32; DEPTH10_LEN] = bid_counts.try_into().map_err(|v: Vec<u32>| {
        anyhow::anyhow!(
            "Expected exactly {DEPTH10_LEN} bid counts, received {}",
            v.len()
        )
    })?;

    let ask_counts: [u32; DEPTH10_LEN] = ask_counts.try_into().map_err(|v: Vec<u32>| {
        anyhow::anyhow!(
            "Expected exactly {DEPTH10_LEN} ask counts, received {}",
            v.len()
        )
    })?;

    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let depth = OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        bid_counts,
        ask_counts,
        msg.flags.raw(),
        msg.sequence.into(),
        ts_event,
        ts_init,
    );

    Ok(depth)
}

/// Decodes a Databento CMBP1 (Consolidated Market by Price Level 1) message.
///
/// Returns a tuple containing an optional `QuoteTick` and an optional `TradeTick`.
/// Returns `None` for the quote if either bid or ask price is undefined (`i64::MAX`).
///
/// # Errors
///
/// Returns an error if decoding the CMBP1 message fails.
pub fn decode_cmbp1_msg(
    msg: &dbn::Cmbp1Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
    include_trades: bool,
) -> anyhow::Result<(Option<QuoteTick>, Option<TradeTick>)> {
    let top_level = &msg.levels[0];
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let maybe_quote = if has_valid_bid_ask(top_level.bid_px, top_level.ask_px) {
        Some(QuoteTick::new(
            instrument_id,
            decode_price_or_undef(top_level.bid_px, price_precision),
            decode_price_or_undef(top_level.ask_px, price_precision),
            decode_quantity(top_level.bid_sz as u64),
            decode_quantity(top_level.ask_sz as u64),
            ts_event,
            ts_init,
        ))
    } else {
        None
    };

    let maybe_trade = if include_trades && is_trade_msg(msg.action) {
        // CMBP1 does not publish a native trade ID; derive a deterministic one
        let trade_id = derive_cmbp_trade_id(
            instrument_id,
            msg.hd.ts_event,
            msg.ts_recv,
            msg.price,
            msg.size,
            msg.side,
        );
        Some(TradeTick::new(
            instrument_id,
            decode_price_or_undef(msg.price, price_precision),
            decode_quantity(msg.size as u64),
            parse_aggressor_side(msg.side),
            trade_id,
            ts_event,
            ts_init,
        ))
    } else {
        None
    };

    Ok((maybe_quote, maybe_trade))
}

/// Decodes a Databento CBBO (Consolidated Best Bid and Offer) message.
///
/// Returns `None` if either bid or ask price is undefined (`i64::MAX`).
///
/// # Errors
///
/// Returns an error if decoding the CBBO message fails.
pub fn decode_cbbo_msg(
    msg: &dbn::CbboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<Option<QuoteTick>> {
    let top_level = &msg.levels[0];
    if !has_valid_bid_ask(top_level.bid_px, top_level.ask_px) {
        return Ok(None);
    }

    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let quote = QuoteTick::new(
        instrument_id,
        decode_price_or_undef(top_level.bid_px, price_precision),
        decode_price_or_undef(top_level.ask_px, price_precision),
        decode_quantity(top_level.bid_sz as u64),
        decode_quantity(top_level.ask_sz as u64),
        ts_event,
        ts_init,
    );

    Ok(Some(quote))
}

/// Decodes a Databento TCBBO (Consolidated Top of Book with Trade) message.
///
/// Returns `None` for the quote if either bid or ask price is undefined (`i64::MAX`).
/// The trade is always returned.
///
/// # Errors
///
/// Returns an error if decoding the TCBBO message fails.
pub fn decode_tcbbo_msg(
    msg: &dbn::TcbboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<(Option<QuoteTick>, TradeTick)> {
    let (maybe_quote, maybe_trade) =
        decode_cmbp1_msg(msg, instrument_id, price_precision, ts_init, true)?;
    let trade = maybe_trade.ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid `TcbboMsg`: expected trade action, was {}",
            msg.action as u8 as char
        )
    })?;

    Ok((maybe_quote, trade))
}

/// # Errors
///
/// Returns an error if `rtype` is not a supported bar aggregation.
pub fn decode_bar_type(
    msg: &dbn::OhlcvMsg,
    instrument_id: InstrumentId,
) -> anyhow::Result<BarType> {
    let bar_type = match msg.hd.rtype {
        32 => {
            // ohlcv-1s
            BarType::new(instrument_id, BAR_SPEC_1S, AggregationSource::External)
        }
        33 => {
            // ohlcv-1m
            BarType::new(instrument_id, BAR_SPEC_1M, AggregationSource::External)
        }
        34 => {
            // ohlcv-1h
            BarType::new(instrument_id, BAR_SPEC_1H, AggregationSource::External)
        }
        35 => {
            // ohlcv-1d
            BarType::new(instrument_id, BAR_SPEC_1D, AggregationSource::External)
        }
        36 => {
            // ohlcv-eod
            BarType::new(instrument_id, BAR_SPEC_1D, AggregationSource::External)
        }
        _ => anyhow::bail!(
            "`rtype` is not a supported bar aggregation, was {}",
            msg.hd.rtype
        ),
    };

    Ok(bar_type)
}

/// # Errors
///
/// Returns an error if `rtype` is not a supported bar aggregation.
pub fn decode_ts_event_adjustment(msg: &dbn::OhlcvMsg) -> anyhow::Result<UnixNanos> {
    let adjustment = match msg.hd.rtype {
        32 => {
            // ohlcv-1s
            BAR_CLOSE_ADJUSTMENT_1S
        }
        33 => {
            // ohlcv-1m
            BAR_CLOSE_ADJUSTMENT_1M
        }
        34 => {
            // ohlcv-1h
            BAR_CLOSE_ADJUSTMENT_1H
        }
        35 | 36 => {
            // ohlcv-1d and ohlcv-eod
            BAR_CLOSE_ADJUSTMENT_1D
        }
        _ => anyhow::bail!(
            "`rtype` is not a supported bar aggregation, was {}",
            msg.hd.rtype
        ),
    };

    Ok(adjustment.into())
}

/// # Errors
///
/// Returns an error if decoding the OHLCV message fails.
pub fn decode_ohlcv_msg(
    msg: &dbn::OhlcvMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
    timestamp_on_close: bool,
) -> anyhow::Result<Bar> {
    let bar_type = decode_bar_type(msg, instrument_id)?;
    let ts_event_adjustment = decode_ts_event_adjustment(msg)?;

    let ts_event_raw = msg.hd.ts_event.into();
    let ts_close = ts_event_raw + ts_event_adjustment;
    let ts_init = ts_init.unwrap_or(ts_close); // received time or close time

    let ts_event = if timestamp_on_close {
        ts_close
    } else {
        ts_event_raw
    };

    let bar = Bar::new(
        bar_type,
        decode_price_or_undef(msg.open, price_precision),
        decode_price_or_undef(msg.high, price_precision),
        decode_price_or_undef(msg.low, price_precision),
        decode_price_or_undef(msg.close, price_precision),
        decode_quantity(msg.volume),
        ts_event,
        ts_init,
    );

    Ok(bar)
}

/// Decodes a Databento status message into an `InstrumentStatus` event.
///
/// # Errors
///
/// Returns an error if decoding the status message fails or if `msg.action` is not a valid `MarketStatusAction`.
pub fn decode_status_msg(
    msg: &dbn::StatusMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<InstrumentStatus> {
    let ts_event = msg.hd.ts_event.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let action = MarketStatusAction::from_u16(msg.action)
        .ok_or_else(|| anyhow::anyhow!("Invalid `MarketStatusAction` value: {}", msg.action))?;

    let status = InstrumentStatus::new(
        instrument_id,
        action,
        ts_event,
        ts_init,
        parse_status_reason(msg.reason)?,
        parse_status_trading_event(msg.trading_event)?,
        parse_optional_bool(msg.is_trading),
        parse_optional_bool(msg.is_quoting),
        parse_optional_bool(msg.is_short_sell_restricted),
    );

    Ok(status)
}

/// # Errors
///
/// Returns an error if decoding the record type fails or encounters unsupported message.
pub fn decode_record(
    record: &dbn::RecordRef,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
    include_trades: bool,
    bars_timestamp_on_close: bool,
) -> anyhow::Result<(Option<Data>, Option<Data>)> {
    let result = if let Some(msg) = record.get::<dbn::MboMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let result = decode_mbo_msg(
            msg,
            instrument_id,
            price_precision,
            Some(ts_init),
            include_trades,
        )?;

        match result {
            (Some(delta), None) => (Some(Data::Delta(delta)), None),
            (None, Some(trade)) => (Some(Data::Trade(trade)), None),
            (None, None) => (None, None),
            _ => anyhow::bail!("Invalid `MboMsg` parsing combination"),
        }
    } else if let Some(msg) = record.get::<dbn::TradeMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let trade = decode_trade_msg(msg, instrument_id, price_precision, Some(ts_init))?;
        (Some(Data::Trade(trade)), None)
    } else if let Some(msg) = record.get::<dbn::Mbp1Msg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let (maybe_quote, maybe_trade) = decode_mbp1_msg(
            msg,
            instrument_id,
            price_precision,
            Some(ts_init),
            include_trades,
        )?;
        (maybe_quote.map(Data::Quote), maybe_trade.map(Data::Trade))
    } else if let Some(msg) = record.get::<dbn::Bbo1SMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let maybe_quote = decode_bbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
        (maybe_quote.map(Data::Quote), None)
    } else if let Some(msg) = record.get::<dbn::Bbo1MMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let maybe_quote = decode_bbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
        (maybe_quote.map(Data::Quote), None)
    } else if let Some(msg) = record.get::<dbn::Mbp10Msg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let depth = decode_mbp10_msg(msg, instrument_id, price_precision, Some(ts_init))?;
        (Some(Data::from(depth)), None)
    } else if let Some(msg) = record.get::<dbn::OhlcvMsg>() {
        // if ts_init is None (like with historical data) we don't want it to be equal to ts_event
        // it will be set correctly in decode_ohlcv_msg instead
        let bar = decode_ohlcv_msg(
            msg,
            instrument_id,
            price_precision,
            ts_init,
            bars_timestamp_on_close,
        )?;
        (Some(Data::Bar(bar)), None)
    } else if let Some(msg) = record.get::<dbn::Cmbp1Msg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        if msg.hd.rtype == dbn::enums::rtype::TCBBO {
            let (maybe_quote, trade) =
                decode_tcbbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
            (maybe_quote.map(Data::Quote), Some(Data::Trade(trade)))
        } else {
            let (maybe_quote, maybe_trade) = decode_cmbp1_msg(
                msg,
                instrument_id,
                price_precision,
                Some(ts_init),
                include_trades,
            )?;
            (maybe_quote.map(Data::Quote), maybe_trade.map(Data::Trade))
        }
    } else if let Some(msg) = record.get::<dbn::TbboMsg>() {
        // TBBO always has a trade, quote may be skipped if prices undefined
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let (maybe_quote, trade) =
            decode_tbbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
        (maybe_quote.map(Data::Quote), Some(Data::Trade(trade)))
    } else if let Some(msg) = record.get::<dbn::CbboMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let maybe_quote = decode_cbbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
        (maybe_quote.map(Data::Quote), None)
    } else {
        anyhow::bail!("DBN message type is not currently supported")
    };

    Ok(result)
}

const fn determine_timestamp(ts_init: Option<UnixNanos>, msg_timestamp: UnixNanos) -> UnixNanos {
    match ts_init {
        Some(ts_init) => ts_init,
        None => msg_timestamp,
    }
}
