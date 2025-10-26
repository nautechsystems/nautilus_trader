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

use std::{ffi::c_char, num::NonZeroUsize};

use databento::dbn::{self};
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND, uuid::UUID4};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, DEPTH10_LEN, Data, InstrumentStatus,
        OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction, FromU8, FromU16,
        InstrumentClass, MarketStatusAction, OptionKind, OrderSide, PriceType,
    },
    identifiers::{InstrumentId, TradeId},
    instruments::{
        Equity, FuturesContract, FuturesSpread, InstrumentAny, OptionContract, OptionSpread,
    },
    types::{
        Currency, Price, Quantity,
        price::{PRICE_UNDEF, decode_raw_price_i64},
    },
};
use ustr::Ustr;

use super::{
    enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction},
    types::{DatabentoImbalance, DatabentoStatistics},
};

// SAFETY: Known valid value
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

const BAR_CLOSE_ADJUSTMENT_1S: u64 = NANOSECONDS_IN_SECOND;
const BAR_CLOSE_ADJUSTMENT_1M: u64 = NANOSECONDS_IN_SECOND * 60;
const BAR_CLOSE_ADJUSTMENT_1H: u64 = NANOSECONDS_IN_SECOND * 60 * 60;
const BAR_CLOSE_ADJUSTMENT_1D: u64 = NANOSECONDS_IN_SECOND * 60 * 60 * 24;

#[must_use]
pub const fn parse_optional_bool(c: c_char) -> Option<bool> {
    match c as u8 as char {
        'Y' => Some(true),
        'N' => Some(false),
        _ => None,
    }
}

#[must_use]
pub const fn parse_order_side(c: c_char) -> OrderSide {
    match c as u8 as char {
        'A' => OrderSide::Sell,
        'B' => OrderSide::Buy,
        _ => OrderSide::NoOrderSide,
    }
}

#[must_use]
pub const fn parse_aggressor_side(c: c_char) -> AggressorSide {
    match c as u8 as char {
        'A' => AggressorSide::Seller,
        'B' => AggressorSide::Buyer,
        _ => AggressorSide::NoAggressor,
    }
}

/// Parses a Databento book action character into a `BookAction` enum.
///
/// # Errors
///
/// Returns an error if `c` is not a valid `BookAction` character.
pub fn parse_book_action(c: c_char) -> anyhow::Result<BookAction> {
    match c as u8 as char {
        'A' => Ok(BookAction::Add),
        'C' => Ok(BookAction::Delete),
        'F' => Ok(BookAction::Update),
        'M' => Ok(BookAction::Update),
        'R' => Ok(BookAction::Clear),
        invalid => anyhow::bail!("Invalid `BookAction`, was '{invalid}'"),
    }
}

/// Parses a Databento option kind character into an `OptionKind` enum.
///
/// # Errors
///
/// Returns an error if `c` is not a valid `OptionKind` character.
pub fn parse_option_kind(c: c_char) -> anyhow::Result<OptionKind> {
    match c as u8 as char {
        'C' => Ok(OptionKind::Call),
        'P' => Ok(OptionKind::Put),
        invalid => anyhow::bail!("Invalid `OptionKind`, was '{invalid}'"),
    }
}

fn parse_currency_or_usd_default(value: Result<&str, impl std::error::Error>) -> Currency {
    match value {
        Ok(value) if !value.is_empty() => Currency::try_from_str(value).unwrap_or_else(|| {
            tracing::warn!("Unknown currency code '{value}', defaulting to USD");
            Currency::USD()
        }),
        Ok(_) => Currency::USD(),
        Err(e) => {
            tracing::error!("Error parsing currency: {e}");
            Currency::USD()
        }
    }
}

/// Parses a CFI (Classification of Financial Instruments) code to extract asset and instrument classes.
///
/// # Errors
///
/// Returns an error if `value` has fewer than 3 characters.
pub fn parse_cfi_iso10926(
    value: &str,
) -> anyhow::Result<(Option<AssetClass>, Option<InstrumentClass>)> {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 3 {
        anyhow::bail!("Value string is too short");
    }

    // TODO: A proper CFI parser would be useful: https://en.wikipedia.org/wiki/ISO_10962
    let cfi_category = chars[0];
    let cfi_group = chars[1];
    let cfi_attribute1 = chars[2];
    // let cfi_attribute2 = value[3];
    // let cfi_attribute3 = value[4];
    // let cfi_attribute4 = value[5];

    let mut asset_class = match cfi_category {
        'D' => Some(AssetClass::Debt),
        'E' => Some(AssetClass::Equity),
        'S' => None,
        _ => None,
    };

    let instrument_class = match cfi_group {
        'I' => Some(InstrumentClass::Future),
        _ => None,
    };

    if cfi_attribute1 == 'I' {
        asset_class = Some(AssetClass::Index);
    }

    Ok((asset_class, instrument_class))
}

/// Parses a Databento status reason code into a human-readable string.
///
/// See: <https://databento.com/docs/schemas-and-data-formats/status#types-of-status-reasons>
///
/// # Errors
///
/// Returns an error if `value` is an invalid status reason code.
pub fn parse_status_reason(value: u16) -> anyhow::Result<Option<Ustr>> {
    let value_str = match value {
        0 => return Ok(None),
        1 => "Scheduled",
        2 => "Surveillance intervention",
        3 => "Market event",
        4 => "Instrument activation",
        5 => "Instrument expiration",
        6 => "Recovery in process",
        10 => "Regulatory",
        11 => "Administrative",
        12 => "Non-compliance",
        13 => "Filings not current",
        14 => "SEC trading suspension",
        15 => "New issue",
        16 => "Issue available",
        17 => "Issues reviewed",
        18 => "Filing requirements satisfied",
        30 => "News pending",
        31 => "News released",
        32 => "News and resumption times",
        33 => "News not forthcoming",
        40 => "Order imbalance",
        50 => "LULD pause",
        60 => "Operational",
        70 => "Additional information requested",
        80 => "Merger effective",
        90 => "ETF",
        100 => "Corporate action",
        110 => "New Security offering",
        120 => "Market wide halt level 1",
        121 => "Market wide halt level 2",
        122 => "Market wide halt level 3",
        123 => "Market wide halt carryover",
        124 => "Market wide halt resumption",
        130 => "Quotation not available",
        invalid => anyhow::bail!("Invalid `StatusMsg` reason, was '{invalid}'"),
    };

    Ok(Some(Ustr::from(value_str)))
}

/// Parses a Databento status trading event code into a human-readable string.
///
/// # Errors
///
/// Returns an error if `value` is an invalid status trading event code.
pub fn parse_status_trading_event(value: u16) -> anyhow::Result<Option<Ustr>> {
    let value_str = match value {
        0 => return Ok(None),
        1 => "No cancel",
        2 => "Change trading session",
        3 => "Implied matching on",
        4 => "Implied matching off",
        _ => anyhow::bail!("Invalid `StatusMsg` trading_event, was '{value}'"),
    };

    Ok(Some(Ustr::from(value_str)))
}

/// Decodes a price from the given value, expressed in units of 1e-9.
#[must_use]
pub fn decode_price(value: i64, precision: u8) -> Price {
    Price::from_raw(decode_raw_price_i64(value), precision)
}

/// Decodes a quantity from the given value, expressed in standard whole-number units.
#[must_use]
pub fn decode_quantity(value: u64) -> Quantity {
    Quantity::from(value)
}

/// Decodes a minimum price increment from the given value, expressed in units of 1e-9.
#[must_use]
pub fn decode_price_increment(value: i64, precision: u8) -> Price {
    match value {
        0 | i64::MAX => Price::new(10f64.powi(-i32::from(precision)), precision),
        _ => decode_price(value, precision),
    }
}

/// Decodes a price from the given optional value, expressed in units of 1e-9.
#[must_use]
pub fn decode_optional_price(value: i64, precision: u8) -> Option<Price> {
    match value {
        i64::MAX => None,
        _ => Some(decode_price(value, precision)),
    }
}

/// Decodes a quantity from the given optional value, where `i64::MAX` indicates missing data.
#[must_use]
pub fn decode_optional_quantity(value: i64) -> Option<Quantity> {
    match value {
        i64::MAX => None,
        _ => Some(Quantity::from(value)),
    }
}

/// Decodes a multiplier from the given value, expressed in units of 1e-9.
/// Uses exact integer arithmetic to avoid precision loss in financial calculations.
///
/// # Errors
///
/// Returns an error if value is negative (invalid multiplier).
pub fn decode_multiplier(value: i64) -> anyhow::Result<Quantity> {
    match value {
        0 | i64::MAX => Ok(Quantity::from(1)),
        v if v < 0 => anyhow::bail!("Invalid negative multiplier: {v}"),
        v => {
            // Work in integers: v is fixed-point with 9 fractional digits.
            // Build a canonical decimal string without floating-point.
            let abs = v as u128;

            const SCALE: u128 = 1_000_000_000;
            let int_part = abs / SCALE;
            let frac_part = abs % SCALE;

            // Format fractional part with exactly 9 digits, then trim trailing zeros
            // to keep a canonical representation.
            if frac_part == 0 {
                // Pure integer
                Ok(Quantity::from(int_part as u64))
            } else {
                let mut frac_str = format!("{frac_part:09}");
                while frac_str.ends_with('0') {
                    frac_str.pop();
                }
                let s = format!("{int_part}.{frac_str}");
                Ok(Quantity::from(s))
            }
        }
    }
}

/// Decodes a lot size from the given value, expressed in standard whole-number units.
#[must_use]
pub fn decode_lot_size(value: i32) -> Quantity {
    match value {
        0 | i32::MAX => Quantity::from(1),
        value => Quantity::from(value),
    }
}

#[must_use]
fn is_trade_msg(action: c_char) -> bool {
    action as u8 as char == 'T'
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
            let price = Price::from_raw(decode_raw_price_i64(msg.price), price_precision);
            let size = Quantity::from(msg.size);
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
    let price = if msg.price == i64::MAX {
        Price::from_raw(PRICE_UNDEF, 0)
    } else {
        Price::from_raw(decode_raw_price_i64(msg.price), price_precision)
    };
    let size = Quantity::from(msg.size);
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
        Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
        Quantity::from(msg.size),
        parse_aggressor_side(msg.side),
        TradeId::new(itoa::Buffer::new().format(msg.sequence)),
        ts_event,
        ts_init,
    );

    Ok(trade)
}

/// Decodes a Databento TBBO (Top of Book with Trade) message into quote and trade ticks.
///
/// # Errors
///
/// Returns an error if decoding the TBBO message fails.
pub fn decode_tbbo_msg(
    msg: &dbn::TbboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<(QuoteTick, TradeTick)> {
    let top_level = &msg.levels[0];
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(top_level.bid_px), price_precision),
        Price::from_raw(decode_raw_price_i64(top_level.ask_px), price_precision),
        Quantity::from(top_level.bid_sz),
        Quantity::from(top_level.ask_sz),
        ts_event,
        ts_init,
    );

    let trade = TradeTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
        Quantity::from(msg.size),
        parse_aggressor_side(msg.side),
        TradeId::new(itoa::Buffer::new().format(msg.sequence)),
        ts_event,
        ts_init,
    );

    Ok((quote, trade))
}

/// Decodes a Databento MBP1 (Market by Price Level 1) message into quote and optional trade ticks.
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
) -> anyhow::Result<(QuoteTick, Option<TradeTick>)> {
    let top_level = &msg.levels[0];
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(top_level.bid_px), price_precision),
        Price::from_raw(decode_raw_price_i64(top_level.ask_px), price_precision),
        Quantity::from(top_level.bid_sz),
        Quantity::from(top_level.ask_sz),
        ts_event,
        ts_init,
    );

    let maybe_trade = if include_trades && msg.action as u8 as char == 'T' {
        Some(TradeTick::new(
            instrument_id,
            Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
            Quantity::from(msg.size),
            parse_aggressor_side(msg.side),
            TradeId::new(itoa::Buffer::new().format(msg.sequence)),
            ts_event,
            ts_init,
        ))
    } else {
        None
    };

    Ok((quote, maybe_trade))
}

/// Decodes a Databento BBO (Best Bid and Offer) message into a `QuoteTick`.
///
/// # Errors
///
/// Returns an error if decoding the BBO message fails.
pub fn decode_bbo_msg(
    msg: &dbn::BboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<QuoteTick> {
    let top_level = &msg.levels[0];
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(top_level.bid_px), price_precision),
        Price::from_raw(decode_raw_price_i64(top_level.ask_px), price_precision),
        Quantity::from(top_level.bid_sz),
        Quantity::from(top_level.ask_sz),
        ts_event,
        ts_init,
    );

    Ok(quote)
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
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from_raw(decode_raw_price_i64(level.bid_px), price_precision),
            Quantity::from(level.bid_sz),
            0,
        );

        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from_raw(decode_raw_price_i64(level.ask_px), price_precision),
            Quantity::from(level.ask_sz),
            0,
        );

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
/// Returns a tuple containing a `QuoteTick` and an optional `TradeTick` based on the message content.
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
) -> anyhow::Result<(QuoteTick, Option<TradeTick>)> {
    let top_level = &msg.levels[0];
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(top_level.bid_px), price_precision),
        Price::from_raw(decode_raw_price_i64(top_level.ask_px), price_precision),
        Quantity::from(top_level.bid_sz),
        Quantity::from(top_level.ask_sz),
        ts_event,
        ts_init,
    );

    let maybe_trade = if include_trades && msg.action as u8 as char == 'T' {
        // Use UUID4 for trade ID as CMBP1 doesn't have a sequence field
        Some(TradeTick::new(
            instrument_id,
            Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
            Quantity::from(msg.size),
            parse_aggressor_side(msg.side),
            TradeId::new(UUID4::new().to_string()),
            ts_event,
            ts_init,
        ))
    } else {
        None
    };

    Ok((quote, maybe_trade))
}

/// Decodes a Databento CBBO (Consolidated Best Bid and Offer) message.
///
/// Returns a `QuoteTick` representing the consolidated best bid and offer.
///
/// # Errors
///
/// Returns an error if decoding the CBBO message fails.
pub fn decode_cbbo_msg(
    msg: &dbn::CbboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<QuoteTick> {
    let top_level = &msg.levels[0];
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(top_level.bid_px), price_precision),
        Price::from_raw(decode_raw_price_i64(top_level.ask_px), price_precision),
        Quantity::from(top_level.bid_sz),
        Quantity::from(top_level.ask_sz),
        ts_event,
        ts_init,
    );

    Ok(quote)
}

/// Decodes a Databento TCBBO (Consolidated Top of Book with Trade) message.
///
/// Returns a tuple containing both a `QuoteTick` and a `TradeTick`.
///
/// # Errors
///
/// Returns an error if decoding the TCBBO message fails.
pub fn decode_tcbbo_msg(
    msg: &dbn::CbboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<(QuoteTick, TradeTick)> {
    let top_level = &msg.levels[0];
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(top_level.bid_px), price_precision),
        Price::from_raw(decode_raw_price_i64(top_level.ask_px), price_precision),
        Quantity::from(top_level.bid_sz),
        Quantity::from(top_level.ask_sz),
        ts_event,
        ts_init,
    );

    // Use UUID4 for trade ID as TCBBO doesn't have a sequence field
    let trade = TradeTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
        Quantity::from(msg.size),
        parse_aggressor_side(msg.side),
        TradeId::new(UUID4::new().to_string()),
        ts_event,
        ts_init,
    );

    Ok((quote, trade))
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
        Price::from_raw(decode_raw_price_i64(msg.open), price_precision),
        Price::from_raw(decode_raw_price_i64(msg.high), price_precision),
        Price::from_raw(decode_raw_price_i64(msg.low), price_precision),
        Price::from_raw(decode_raw_price_i64(msg.close), price_precision),
        Quantity::from(msg.volume),
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
    // Note: TBBO and TCBBO messages provide both quotes and trades.
    // TBBO is handled explicitly below, while TCBBO is handled by
    // the CbboMsg branch based on whether it has trade data.
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
        let result = decode_mbp1_msg(
            msg,
            instrument_id,
            price_precision,
            Some(ts_init),
            include_trades,
        )?;
        match result {
            (quote, None) => (Some(Data::Quote(quote)), None),
            (quote, Some(trade)) => (Some(Data::Quote(quote)), Some(Data::Trade(trade))),
        }
    } else if let Some(msg) = record.get::<dbn::Bbo1SMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let quote = decode_bbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
        (Some(Data::Quote(quote)), None)
    } else if let Some(msg) = record.get::<dbn::Bbo1MMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let quote = decode_bbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
        (Some(Data::Quote(quote)), None)
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
        let result = decode_cmbp1_msg(
            msg,
            instrument_id,
            price_precision,
            Some(ts_init),
            include_trades,
        )?;
        match result {
            (quote, None) => (Some(Data::Quote(quote)), None),
            (quote, Some(trade)) => (Some(Data::Quote(quote)), Some(Data::Trade(trade))),
        }
    } else if let Some(msg) = record.get::<dbn::TbboMsg>() {
        // TBBO always has both quote and trade
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let (quote, trade) = decode_tbbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
        (Some(Data::Quote(quote)), Some(Data::Trade(trade)))
    } else if let Some(msg) = record.get::<dbn::CbboMsg>() {
        // Check if this is a TCBBO or regular CBBO based on whether it has trade data
        if msg.price != i64::MAX && msg.size > 0 {
            // TCBBO - has both quote and trade
            let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
            let (quote, trade) =
                decode_tcbbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
            (Some(Data::Quote(quote)), Some(Data::Trade(trade)))
        } else {
            // Regular CBBO - quote only
            let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
            let quote = decode_cbbo_msg(msg, instrument_id, price_precision, Some(ts_init))?;
            (Some(Data::Quote(quote)), None)
        }
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

/// # Errors
///
/// Returns an error if decoding the `InstrumentDefMsg` fails.
pub fn decode_instrument_def_msg(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<InstrumentAny> {
    match msg.instrument_class as u8 as char {
        'K' => Ok(InstrumentAny::Equity(decode_equity(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'F' => Ok(InstrumentAny::FuturesContract(decode_futures_contract(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'S' => Ok(InstrumentAny::FuturesSpread(decode_futures_spread(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'C' | 'P' => Ok(InstrumentAny::OptionContract(decode_option_contract(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'T' | 'M' => Ok(InstrumentAny::OptionSpread(decode_option_spread(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'B' => anyhow::bail!("Unsupported `instrument_class` 'B' (Bond)"),
        'X' => anyhow::bail!("Unsupported `instrument_class` 'X' (FX spot)"),
        _ => anyhow::bail!(
            "Unsupported `instrument_class` '{}'",
            msg.instrument_class as u8 as char
        ),
    }
}

/// Decodes a Databento instrument definition message into an `Equity` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `Equity` fails.
pub fn decode_equity(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<Equity> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(Equity::new(
        instrument_id,
        instrument_id.symbol,
        None, // No ISIN available yet
        currency,
        price_increment.precision,
        price_increment,
        Some(lot_size),
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        ts_event,
        ts_init,
    ))
}

/// Decodes a Databento instrument definition message into a `FuturesContract` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `FuturesContract` fails.
pub fn decode_futures_contract(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<FuturesContract> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = Ustr::from(msg.asset()?);
    let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?)?;
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty)?;
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    FuturesContract::new_checked(
        instrument_id,
        instrument_id.symbol,
        asset_class.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        msg.activation.into(),
        msg.expiration.into(),
        currency,
        price_increment.precision,
        price_increment,
        multiplier,
        lot_size,
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        ts_event,
        ts_init,
    )
}

/// Decodes a Databento instrument definition message into a `FuturesSpread` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `FuturesSpread` fails.
pub fn decode_futures_spread(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<FuturesSpread> {
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = Ustr::from(msg.asset()?);
    let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?)?;
    let strategy_type = Ustr::from(msg.secsubtype()?);
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty)?;
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    FuturesSpread::new_checked(
        instrument_id,
        instrument_id.symbol,
        asset_class.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        strategy_type,
        msg.activation.into(),
        msg.expiration.into(),
        currency,
        price_increment.precision,
        price_increment,
        multiplier,
        lot_size,
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        ts_event,
        ts_init,
    )
}

/// Decodes a Databento instrument definition message into an `OptionContract` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `OptionContract` fails.
pub fn decode_option_contract(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<OptionContract> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let strike_price_currency = parse_currency_or_usd_default(msg.strike_price_currency());
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = Ustr::from(msg.underlying()?);
    let asset_class_opt = if instrument_id.venue.as_str() == "OPRA" {
        Some(AssetClass::Equity)
    } else {
        let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?)?;
        asset_class
    };
    let option_kind = parse_option_kind(msg.instrument_class)?;
    let strike_price = Price::from_raw(
        decode_raw_price_i64(msg.strike_price),
        strike_price_currency.precision,
    );
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty)?;
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    OptionContract::new_checked(
        instrument_id,
        instrument_id.symbol,
        asset_class_opt.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        option_kind,
        strike_price,
        currency,
        msg.activation.into(),
        msg.expiration.into(),
        price_increment.precision,
        price_increment,
        multiplier,
        lot_size,
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        ts_event,
        ts_init,
    )
}

/// Decodes a Databento instrument definition message into an `OptionSpread` instrument.
///
/// # Errors
///
/// Returns an error if parsing or constructing `OptionSpread` fails.
pub fn decode_option_spread(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<OptionSpread> {
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = Ustr::from(msg.underlying()?);
    let asset_class_opt = if instrument_id.venue.as_str() == "OPRA" {
        Some(AssetClass::Equity)
    } else {
        let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?)?;
        asset_class
    };
    let strategy_type = Ustr::from(msg.secsubtype()?);
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty)?;
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = msg.ts_recv.into(); // More accurate and reliable timestamp
    let ts_init = ts_init.unwrap_or(ts_event);

    OptionSpread::new_checked(
        instrument_id,
        instrument_id.symbol,
        asset_class_opt.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        strategy_type,
        msg.activation.into(),
        msg.expiration.into(),
        currency,
        price_increment.precision,
        price_increment,
        multiplier,
        lot_size,
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        ts_event,
        ts_init,
    )
}

/// Decodes a Databento imbalance message into a `DatabentoImbalance` event.
///
/// # Errors
///
/// Returns an error if constructing `DatabentoImbalance` fails.
pub fn decode_imbalance_msg(
    msg: &dbn::ImbalanceMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<DatabentoImbalance> {
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(DatabentoImbalance::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(msg.ref_price), price_precision),
        Price::from_raw(
            decode_raw_price_i64(msg.cont_book_clr_price),
            price_precision,
        ),
        Price::from_raw(
            decode_raw_price_i64(msg.auct_interest_clr_price),
            price_precision,
        ),
        Quantity::new(f64::from(msg.paired_qty), 0),
        Quantity::new(f64::from(msg.total_imbalance_qty), 0),
        parse_order_side(msg.side),
        msg.significant_imbalance as c_char,
        msg.hd.ts_event.into(),
        ts_event,
        ts_init,
    ))
}

/// Decodes a Databento statistics message into a `DatabentoStatistics` event.
///
/// # Errors
///
/// Returns an error if constructing `DatabentoStatistics` fails or if `msg.stat_type` or
/// `msg.update_action` is not a valid enum variant.
pub fn decode_statistics_msg(
    msg: &dbn::StatMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<DatabentoStatistics> {
    let stat_type = DatabentoStatisticType::from_u8(msg.stat_type as u8)
        .ok_or_else(|| anyhow::anyhow!("Invalid value for `stat_type`: {}", msg.stat_type))?;
    let update_action =
        DatabentoStatisticUpdateAction::from_u8(msg.update_action).ok_or_else(|| {
            anyhow::anyhow!("Invalid value for `update_action`: {}", msg.update_action)
        })?;
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(DatabentoStatistics::new(
        instrument_id,
        stat_type,
        update_action,
        decode_optional_price(msg.price, price_precision),
        decode_optional_quantity(msg.quantity),
        msg.channel_id,
        msg.stat_flags,
        msg.sequence,
        msg.ts_ref.into(),
        msg.ts_in_delta,
        msg.hd.ts_event.into(),
        ts_event,
        ts_init,
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use databento::dbn::decode::{DecodeStream, dbn::Decoder};
    use fallible_streaming_iterator::FallibleStreamingIterator;
    use nautilus_model::instruments::Instrument;
    use rstest::*;

    use super::*;

    fn test_data_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data")
    }

    #[rstest]
    #[case('Y' as c_char, Some(true))]
    #[case('N' as c_char, Some(false))]
    #[case('X' as c_char, None)]
    fn test_parse_optional_bool(#[case] input: c_char, #[case] expected: Option<bool>) {
        assert_eq!(parse_optional_bool(input), expected);
    }

    #[rstest]
    #[case('A' as c_char, OrderSide::Sell)]
    #[case('B' as c_char, OrderSide::Buy)]
    #[case('X' as c_char, OrderSide::NoOrderSide)]
    fn test_parse_order_side(#[case] input: c_char, #[case] expected: OrderSide) {
        assert_eq!(parse_order_side(input), expected);
    }

    #[rstest]
    #[case('A' as c_char, AggressorSide::Seller)]
    #[case('B' as c_char, AggressorSide::Buyer)]
    #[case('X' as c_char, AggressorSide::NoAggressor)]
    fn test_parse_aggressor_side(#[case] input: c_char, #[case] expected: AggressorSide) {
        assert_eq!(parse_aggressor_side(input), expected);
    }

    #[rstest]
    #[case('T' as c_char, true)]
    #[case('A' as c_char, false)]
    #[case('C' as c_char, false)]
    #[case('F' as c_char, false)]
    #[case('M' as c_char, false)]
    #[case('R' as c_char, false)]
    fn test_is_trade_msg(#[case] action: c_char, #[case] expected: bool) {
        assert_eq!(is_trade_msg(action), expected);
    }

    #[rstest]
    #[case('A' as c_char, Ok(BookAction::Add))]
    #[case('C' as c_char, Ok(BookAction::Delete))]
    #[case('F' as c_char, Ok(BookAction::Update))]
    #[case('M' as c_char, Ok(BookAction::Update))]
    #[case('R' as c_char, Ok(BookAction::Clear))]
    #[case('X' as c_char, Err("Invalid `BookAction`, was 'X'"))]
    fn test_parse_book_action(#[case] input: c_char, #[case] expected: Result<BookAction, &str>) {
        match parse_book_action(input) {
            Ok(action) => assert_eq!(Ok(action), expected),
            Err(e) => assert_eq!(Err(e.to_string().as_str()), expected),
        }
    }

    #[rstest]
    #[case('C' as c_char, Ok(OptionKind::Call))]
    #[case('P' as c_char, Ok(OptionKind::Put))]
    #[case('X' as c_char, Err("Invalid `OptionKind`, was 'X'"))]
    fn test_parse_option_kind(#[case] input: c_char, #[case] expected: Result<OptionKind, &str>) {
        match parse_option_kind(input) {
            Ok(kind) => assert_eq!(Ok(kind), expected),
            Err(e) => assert_eq!(Err(e.to_string().as_str()), expected),
        }
    }

    #[rstest]
    #[case(Ok("USD"), Currency::USD())]
    #[case(Ok("EUR"), Currency::try_from_str("EUR").unwrap())]
    #[case(Ok(""), Currency::USD())]
    #[case(Err("Error"), Currency::USD())]
    fn test_parse_currency_or_usd_default(
        #[case] input: Result<&str, &'static str>, // Using `&'static str` for errors
        #[case] expected: Currency,
    ) {
        let actual = parse_currency_or_usd_default(input.map_err(std::io::Error::other));
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case("DII", Ok((Some(AssetClass::Index), Some(InstrumentClass::Future))))]
    #[case("EII", Ok((Some(AssetClass::Index), Some(InstrumentClass::Future))))]
    #[case("EIA", Ok((Some(AssetClass::Equity), Some(InstrumentClass::Future))))]
    #[case("XXX", Ok((None, None)))]
    #[case("D", Err("Value string is too short"))]
    fn test_parse_cfi_iso10926(
        #[case] input: &str,
        #[case] expected: Result<(Option<AssetClass>, Option<InstrumentClass>), &'static str>,
    ) {
        match parse_cfi_iso10926(input) {
            Ok(result) => assert_eq!(Ok(result), expected),
            Err(e) => assert_eq!(Err(e.to_string().as_str()), expected),
        }
    }

    #[rstest]
    #[case(0, 2, Price::new(0.01, 2))] // Default for 0
    #[case(i64::MAX, 2, Price::new(0.01, 2))] // Default for i64::MAX
    #[case(1000000, 2, Price::from_raw(decode_raw_price_i64(1000000), 2))] // Arbitrary valid price
    fn test_decode_price(#[case] value: i64, #[case] precision: u8, #[case] expected: Price) {
        let actual = decode_price_increment(value, precision);
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case(i64::MAX, 2, None)] // None for i64::MAX
    #[case(0, 2, Some(Price::from_raw(0, 2)))] // 0 is valid here
    #[case(1000000, 2, Some(Price::from_raw(decode_raw_price_i64(1000000), 2)))] // Arbitrary valid price
    fn test_decode_optional_price(
        #[case] value: i64,
        #[case] precision: u8,
        #[case] expected: Option<Price>,
    ) {
        let actual = decode_optional_price(value, precision);
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case(i64::MAX, None)] // None for i32::MAX
    #[case(0, Some(Quantity::new(0.0, 0)))] // 0 is valid quantity
    #[case(10, Some(Quantity::new(10.0, 0)))] // Arbitrary valid quantity
    fn test_decode_optional_quantity(#[case] value: i64, #[case] expected: Option<Quantity>) {
        let actual = decode_optional_quantity(value);
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case(0, Quantity::from(1))] // Default fallback for 0
    #[case(i64::MAX, Quantity::from(1))] // Default fallback for i64::MAX
    #[case(50_000_000_000, Quantity::from("50"))] // 50.0 exactly
    #[case(12_500_000_000, Quantity::from("12.5"))] // 12.5 exactly
    #[case(1_000_000_000, Quantity::from("1"))] // 1.0 exactly
    #[case(1, Quantity::from("0.000000001"))] // Smallest positive value
    #[case(1_000_000_001, Quantity::from("1.000000001"))] // Just over 1.0
    #[case(999_999_999, Quantity::from("0.999999999"))] // Just under 1.0
    #[case(123_456_789_000, Quantity::from("123.456789"))] // Trailing zeros trimmed
    fn test_decode_multiplier_precise(#[case] raw: i64, #[case] expected: Quantity) {
        assert_eq!(decode_multiplier(raw).unwrap(), expected);
    }

    #[rstest]
    #[case(-1_500_000_000)] // Large negative value
    #[case(-1)] // Small negative value
    #[case(-999_999_999)] // Another negative value
    fn test_decode_multiplier_negative_error(#[case] raw: i64) {
        let result = decode_multiplier(raw);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid negative multiplier")
        );
    }

    #[rstest]
    #[case(100, Quantity::from(100))]
    #[case(1000, Quantity::from(1000))]
    #[case(5, Quantity::from(5))]
    fn test_decode_quantity(#[case] value: u64, #[case] expected: Quantity) {
        assert_eq!(decode_quantity(value), expected);
    }

    #[rstest]
    #[case(0, 2, Price::new(0.01, 2))] // Default for 0
    #[case(i64::MAX, 2, Price::new(0.01, 2))] // Default for i64::MAX
    #[case(1000000, 2, Price::from_raw(decode_raw_price_i64(1000000), 2))] // Arbitrary valid price
    fn test_decode_price_increment(
        #[case] value: i64,
        #[case] precision: u8,
        #[case] expected: Price,
    ) {
        assert_eq!(decode_price_increment(value, precision), expected);
    }

    #[rstest]
    #[case(0, Quantity::from(1))] // Default for 0
    #[case(i32::MAX, Quantity::from(1))] // Default for MAX
    #[case(100, Quantity::from(100))]
    #[case(1, Quantity::from(1))]
    #[case(1000, Quantity::from(1000))]
    fn test_decode_lot_size(#[case] value: i32, #[case] expected: Quantity) {
        assert_eq!(decode_lot_size(value), expected);
    }

    #[rstest]
    #[case(0, None)] // None for 0
    #[case(1, Some(Ustr::from("Scheduled")))]
    #[case(2, Some(Ustr::from("Surveillance intervention")))]
    #[case(3, Some(Ustr::from("Market event")))]
    #[case(10, Some(Ustr::from("Regulatory")))]
    #[case(30, Some(Ustr::from("News pending")))]
    #[case(40, Some(Ustr::from("Order imbalance")))]
    #[case(50, Some(Ustr::from("LULD pause")))]
    #[case(60, Some(Ustr::from("Operational")))]
    #[case(100, Some(Ustr::from("Corporate action")))]
    #[case(120, Some(Ustr::from("Market wide halt level 1")))]
    fn test_parse_status_reason(#[case] value: u16, #[case] expected: Option<Ustr>) {
        assert_eq!(parse_status_reason(value).unwrap(), expected);
    }

    #[rstest]
    #[case(999)] // Invalid code
    fn test_parse_status_reason_invalid(#[case] value: u16) {
        assert!(parse_status_reason(value).is_err());
    }

    #[rstest]
    #[case(0, None)] // None for 0
    #[case(1, Some(Ustr::from("No cancel")))]
    #[case(2, Some(Ustr::from("Change trading session")))]
    #[case(3, Some(Ustr::from("Implied matching on")))]
    #[case(4, Some(Ustr::from("Implied matching off")))]
    fn test_parse_status_trading_event(#[case] value: u16, #[case] expected: Option<Ustr>) {
        assert_eq!(parse_status_trading_event(value).unwrap(), expected);
    }

    #[rstest]
    #[case(5)] // Invalid code
    #[case(100)] // Invalid code
    fn test_parse_status_trading_event_invalid(#[case] value: u16) {
        assert!(parse_status_trading_event(value).is_err());
    }

    #[rstest]
    fn test_decode_mbo_msg() {
        let path = test_data_path().join("test_data.mbo.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::MboMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let (delta, _) = decode_mbo_msg(msg, instrument_id, 2, Some(0.into()), false).unwrap();
        let delta = delta.unwrap();

        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.action, BookAction::Delete);
        assert_eq!(delta.order.side, OrderSide::Sell);
        assert_eq!(delta.order.price, Price::from("3722.75"));
        assert_eq!(delta.order.size, Quantity::from("1"));
        assert_eq!(delta.order.order_id, 647_784_973_705);
        assert_eq!(delta.flags, 128);
        assert_eq!(delta.sequence, 1_170_352);
        assert_eq!(delta.ts_event, msg.ts_recv);
        assert_eq!(delta.ts_event, 1_609_160_400_000_704_060);
        assert_eq!(delta.ts_init, 0);
    }

    #[rstest]
    fn test_decode_mbo_msg_clear_action() {
        // Create an MBO message with Clear action (action='R', side='N')
        let ts_recv = 1_609_160_400_000_000_000;
        let msg = dbn::MboMsg {
            hd: dbn::RecordHeader::new::<dbn::MboMsg>(1, 1, ts_recv as u32, 0),
            order_id: 0,
            price: i64::MAX,
            size: 0,
            flags: dbn::FlagSet::empty(),
            channel_id: 0,
            action: 'R' as c_char,
            side: 'N' as c_char, // NoOrderSide for Clear
            ts_recv,
            ts_in_delta: 0,
            sequence: 1_000_000,
        };

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let (delta, trade) = decode_mbo_msg(&msg, instrument_id, 2, Some(0.into()), false).unwrap();

        // Clear messages should produce OrderBookDelta, not TradeTick
        assert!(trade.is_none());
        let delta = delta.expect("Clear action should produce OrderBookDelta");

        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.action, BookAction::Clear);
        assert_eq!(delta.order.side, OrderSide::NoOrderSide);
        assert_eq!(delta.order.size, Quantity::from("0"));
        assert_eq!(delta.order.order_id, 0);
        assert_eq!(delta.sequence, 1_000_000);
        assert_eq!(delta.ts_event, ts_recv);
        assert_eq!(delta.ts_init, 0);
    }

    #[rstest]
    fn test_decode_mbo_msg_no_order_side_update() {
        // MBO messages with NoOrderSide are now passed through to the book
        // The book will resolve the side from its cache using the order_id
        let ts_recv = 1_609_160_400_000_000_000;
        let msg = dbn::MboMsg {
            hd: dbn::RecordHeader::new::<dbn::MboMsg>(1, 1, ts_recv as u32, 0),
            order_id: 123_456_789,
            price: 4_800_250_000_000, // $4800.25 with precision 2
            size: 1,
            flags: dbn::FlagSet::empty(),
            channel_id: 1,
            action: 'M' as c_char, // Modify/Update action
            side: 'N' as c_char,   // NoOrderSide
            ts_recv,
            ts_in_delta: 0,
            sequence: 1_000_000,
        };

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let (delta, trade) = decode_mbo_msg(&msg, instrument_id, 2, Some(0.into()), false).unwrap();

        // Delta should be created with NoOrderSide (book will resolve it)
        assert!(delta.is_some());
        assert!(trade.is_none());
        let delta = delta.unwrap();
        assert_eq!(delta.order.side, OrderSide::NoOrderSide);
        assert_eq!(delta.order.order_id, 123_456_789);
        assert_eq!(delta.action, BookAction::Update);
    }

    #[rstest]
    fn test_decode_mbp1_msg() {
        let path = test_data_path().join("test_data.mbp-1.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::Mbp1Msg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let (quote, _) = decode_mbp1_msg(msg, instrument_id, 2, Some(0.into()), false).unwrap();

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("3720.25"));
        assert_eq!(quote.ask_price, Price::from("3720.50"));
        assert_eq!(quote.bid_size, Quantity::from("24"));
        assert_eq!(quote.ask_size, Quantity::from("11"));
        assert_eq!(quote.ts_event, msg.ts_recv);
        assert_eq!(quote.ts_event, 1_609_160_400_006_136_329);
        assert_eq!(quote.ts_init, 0);
    }

    #[rstest]
    fn test_decode_bbo_1s_msg() {
        let path = test_data_path().join("test_data.bbo-1s.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::BboMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let quote = decode_bbo_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("3702.25"));
        assert_eq!(quote.ask_price, Price::from("3702.75"));
        assert_eq!(quote.bid_size, Quantity::from("18"));
        assert_eq!(quote.ask_size, Quantity::from("13"));
        assert_eq!(quote.ts_event, msg.ts_recv);
        assert_eq!(quote.ts_event, 1609113600000000000);
        assert_eq!(quote.ts_init, 0);
    }

    #[rstest]
    fn test_decode_bbo_1m_msg() {
        let path = test_data_path().join("test_data.bbo-1m.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::BboMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let quote = decode_bbo_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("3702.25"));
        assert_eq!(quote.ask_price, Price::from("3702.75"));
        assert_eq!(quote.bid_size, Quantity::from("18"));
        assert_eq!(quote.ask_size, Quantity::from("13"));
        assert_eq!(quote.ts_event, msg.ts_recv);
        assert_eq!(quote.ts_event, 1609113600000000000);
        assert_eq!(quote.ts_init, 0);
    }

    #[rstest]
    fn test_decode_mbp10_msg() {
        let path = test_data_path().join("test_data.mbp-10.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::Mbp10Msg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let depth10 = decode_mbp10_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

        assert_eq!(depth10.instrument_id, instrument_id);
        assert_eq!(depth10.bids.len(), 10);
        assert_eq!(depth10.asks.len(), 10);
        assert_eq!(depth10.bid_counts.len(), 10);
        assert_eq!(depth10.ask_counts.len(), 10);
        assert_eq!(depth10.flags, 128);
        assert_eq!(depth10.sequence, 1_170_352);
        assert_eq!(depth10.ts_event, msg.ts_recv);
        assert_eq!(depth10.ts_event, 1_609_160_400_000_704_060);
        assert_eq!(depth10.ts_init, 0);
    }

    #[rstest]
    fn test_decode_trade_msg() {
        let path = test_data_path().join("test_data.trades.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::TradeMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let trade = decode_trade_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

        assert_eq!(trade.instrument_id, instrument_id);
        assert_eq!(trade.price, Price::from("3720.25"));
        assert_eq!(trade.size, Quantity::from("5"));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(trade.trade_id.to_string(), "1170380");
        assert_eq!(trade.ts_event, msg.ts_recv);
        assert_eq!(trade.ts_event, 1_609_160_400_099_150_057);
        assert_eq!(trade.ts_init, 0);
    }

    #[rstest]
    fn test_decode_tbbo_msg() {
        let path = test_data_path().join("test_data.tbbo.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::Mbp1Msg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let (quote, trade) = decode_tbbo_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("3720.25"));
        assert_eq!(quote.ask_price, Price::from("3720.50"));
        assert_eq!(quote.bid_size, Quantity::from("26"));
        assert_eq!(quote.ask_size, Quantity::from("7"));
        assert_eq!(quote.ts_event, msg.ts_recv);
        assert_eq!(quote.ts_event, 1_609_160_400_099_150_057);
        assert_eq!(quote.ts_init, 0);

        assert_eq!(trade.instrument_id, instrument_id);
        assert_eq!(trade.price, Price::from("3720.25"));
        assert_eq!(trade.size, Quantity::from("5"));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(trade.trade_id.to_string(), "1170380");
        assert_eq!(trade.ts_event, msg.ts_recv);
        assert_eq!(trade.ts_event, 1_609_160_400_099_150_057);
        assert_eq!(trade.ts_init, 0);
    }

    #[rstest]
    fn test_decode_ohlcv_msg() {
        let path = test_data_path().join("test_data.ohlcv-1s.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::OhlcvMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let bar = decode_ohlcv_msg(msg, instrument_id, 2, Some(0.into()), true).unwrap();

        assert_eq!(
            bar.bar_type,
            BarType::from("ESM4.GLBX-1-SECOND-LAST-EXTERNAL")
        );
        assert_eq!(bar.open, Price::from("372025.00"));
        assert_eq!(bar.high, Price::from("372050.00"));
        assert_eq!(bar.low, Price::from("372025.00"));
        assert_eq!(bar.close, Price::from("372050.00"));
        assert_eq!(bar.volume, Quantity::from("57"));
        assert_eq!(bar.ts_event, msg.hd.ts_event + BAR_CLOSE_ADJUSTMENT_1S); // timestamp_on_close=true
        assert_eq!(bar.ts_init, 0); // ts_init was Some(0)
    }

    #[rstest]
    fn test_decode_definition_msg() {
        let path = test_data_path().join("test_data.definition.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::InstrumentDefMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let result = decode_instrument_def_msg(msg, instrument_id, Some(0.into()));

        assert!(result.is_ok());
        assert_eq!(result.unwrap().multiplier(), Quantity::from(1));
    }

    #[rstest]
    fn test_decode_status_msg() {
        let path = test_data_path().join("test_data.status.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::StatusMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let status = decode_status_msg(msg, instrument_id, Some(0.into())).unwrap();

        assert_eq!(status.instrument_id, instrument_id);
        assert_eq!(status.action, MarketStatusAction::Trading);
        assert_eq!(status.ts_event, msg.hd.ts_event);
        assert_eq!(status.ts_init, 0);
        assert_eq!(status.reason, Some(Ustr::from("Scheduled")));
        assert_eq!(status.trading_event, None);
        assert_eq!(status.is_trading, Some(true));
        assert_eq!(status.is_quoting, Some(true));
        assert_eq!(status.is_short_sell_restricted, None);
    }

    #[rstest]
    fn test_decode_imbalance_msg() {
        let path = test_data_path().join("test_data.imbalance.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::ImbalanceMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let imbalance = decode_imbalance_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

        assert_eq!(imbalance.instrument_id, instrument_id);
        assert_eq!(imbalance.ref_price, Price::from("229.43"));
        assert_eq!(imbalance.cont_book_clr_price, Price::from("0.00"));
        assert_eq!(imbalance.auct_interest_clr_price, Price::from("0.00"));
        assert_eq!(imbalance.paired_qty, Quantity::from("0"));
        assert_eq!(imbalance.total_imbalance_qty, Quantity::from("2000"));
        assert_eq!(imbalance.side, OrderSide::Buy);
        assert_eq!(imbalance.significant_imbalance, 126);
        assert_eq!(imbalance.ts_event, msg.hd.ts_event);
        assert_eq!(imbalance.ts_recv, msg.ts_recv);
        assert_eq!(imbalance.ts_init, 0);
    }

    #[rstest]
    fn test_decode_statistics_msg() {
        let path = test_data_path().join("test_data.statistics.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::StatMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let statistics = decode_statistics_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

        assert_eq!(statistics.instrument_id, instrument_id);
        assert_eq!(statistics.stat_type, DatabentoStatisticType::LowestOffer);
        assert_eq!(
            statistics.update_action,
            DatabentoStatisticUpdateAction::Added
        );
        assert_eq!(statistics.price, Some(Price::from("100.00")));
        assert_eq!(statistics.quantity, None);
        assert_eq!(statistics.channel_id, 13);
        assert_eq!(statistics.stat_flags, 255);
        assert_eq!(statistics.sequence, 2);
        assert_eq!(statistics.ts_ref, 18_446_744_073_709_551_615);
        assert_eq!(statistics.ts_in_delta, 26961);
        assert_eq!(statistics.ts_event, msg.hd.ts_event);
        assert_eq!(statistics.ts_recv, msg.ts_recv);
        assert_eq!(statistics.ts_init, 0);
    }

    #[rstest]
    fn test_decode_cmbp1_msg() {
        let path = test_data_path().join("test_data.cmbp-1.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::Cmbp1Msg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let (quote, trade) = decode_cmbp1_msg(msg, instrument_id, 2, Some(0.into()), true).unwrap();

        assert_eq!(quote.instrument_id, instrument_id);
        assert!(quote.bid_price.raw > 0);
        assert!(quote.ask_price.raw > 0);
        assert!(quote.bid_size.raw > 0);
        assert!(quote.ask_size.raw > 0);
        assert_eq!(quote.ts_event, msg.ts_recv);
        assert_eq!(quote.ts_init, 0);

        // Check if trade is present based on action
        if msg.action as u8 as char == 'T' {
            assert!(trade.is_some());
            let trade = trade.unwrap();
            assert_eq!(trade.instrument_id, instrument_id);
        } else {
            assert!(trade.is_none());
        }
    }

    #[rstest]
    fn test_decode_cbbo_1s_msg() {
        let path = test_data_path().join("test_data.cbbo-1s.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::CbboMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let quote = decode_cbbo_msg(msg, instrument_id, 2, Some(0.into())).unwrap();

        assert_eq!(quote.instrument_id, instrument_id);
        assert!(quote.bid_price.raw > 0);
        assert!(quote.ask_price.raw > 0);
        assert!(quote.bid_size.raw > 0);
        assert!(quote.ask_size.raw > 0);
        assert_eq!(quote.ts_event, msg.ts_recv);
        assert_eq!(quote.ts_init, 0);
    }

    #[rstest]
    fn test_decode_mbp10_msg_with_all_levels() {
        let mut msg = dbn::Mbp10Msg::default();
        for i in 0..10 {
            msg.levels[i].bid_px = 100_000_000_000 - i as i64 * 10_000_000;
            msg.levels[i].ask_px = 100_010_000_000 + i as i64 * 10_000_000;
            msg.levels[i].bid_sz = 10 + i as u32;
            msg.levels[i].ask_sz = 10 + i as u32;
            msg.levels[i].bid_ct = 1 + i as u32;
            msg.levels[i].ask_ct = 1 + i as u32;
        }
        msg.ts_recv = 1_609_160_400_000_704_060;

        let instrument_id = InstrumentId::from("TEST.VENUE");
        let result = decode_mbp10_msg(&msg, instrument_id, 2, None);

        assert!(result.is_ok());
        let depth = result.unwrap();
        assert_eq!(depth.bids.len(), 10);
        assert_eq!(depth.asks.len(), 10);
        assert_eq!(depth.bid_counts.len(), 10);
        assert_eq!(depth.ask_counts.len(), 10);
    }

    #[rstest]
    fn test_array_conversion_error_handling() {
        let mut bids = Vec::new();
        let mut asks = Vec::new();

        // Intentionally create fewer than DEPTH10_LEN elements
        for i in 0..5 {
            bids.push(BookOrder::new(
                OrderSide::Buy,
                Price::from(format!("{}.00", 100 - i)),
                Quantity::from(10),
                i as u64,
            ));
            asks.push(BookOrder::new(
                OrderSide::Sell,
                Price::from(format!("{}.00", 101 + i)),
                Quantity::from(10),
                i as u64,
            ));
        }

        let result: Result<[BookOrder; DEPTH10_LEN], _> =
            bids.try_into().map_err(|v: Vec<BookOrder>| {
                anyhow::anyhow!(
                    "Expected exactly {DEPTH10_LEN} bid levels, received {}",
                    v.len()
                )
            });
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Expected exactly 10 bid levels, received 5")
        );
    }

    #[rstest]
    fn test_decode_tcbbo_msg() {
        // Use cbbo-1s as base since cbbo.dbn.zst was invalid
        let path = test_data_path().join("test_data.cbbo-1s.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::CbboMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        // Simulate TCBBO by adding trade data
        let mut tcbbo_msg = msg.clone();
        tcbbo_msg.price = 3702500000000;
        tcbbo_msg.size = 10;

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let (quote, trade) =
            decode_tcbbo_msg(&tcbbo_msg, instrument_id, 2, Some(0.into())).unwrap();

        assert_eq!(quote.instrument_id, instrument_id);
        assert!(quote.bid_price.raw > 0);
        assert!(quote.ask_price.raw > 0);
        assert!(quote.bid_size.raw > 0);
        assert!(quote.ask_size.raw > 0);
        assert_eq!(quote.ts_event, tcbbo_msg.ts_recv);
        assert_eq!(quote.ts_init, 0);

        assert_eq!(trade.instrument_id, instrument_id);
        assert_eq!(trade.price, Price::from("3702.50"));
        assert_eq!(trade.size, Quantity::from(10));
        assert_eq!(trade.ts_event, tcbbo_msg.ts_recv);
        assert_eq!(trade.ts_init, 0);
    }

    #[rstest]
    fn test_decode_bar_type() {
        let mut msg = dbn::OhlcvMsg::default_for_schema(dbn::Schema::Ohlcv1S);
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        // Test 1-second bar
        msg.hd.rtype = 32;
        let bar_type = decode_bar_type(&msg, instrument_id).unwrap();
        assert_eq!(bar_type, BarType::from("ESM4.GLBX-1-SECOND-LAST-EXTERNAL"));

        // Test 1-minute bar
        msg.hd.rtype = 33;
        let bar_type = decode_bar_type(&msg, instrument_id).unwrap();
        assert_eq!(bar_type, BarType::from("ESM4.GLBX-1-MINUTE-LAST-EXTERNAL"));

        // Test 1-hour bar
        msg.hd.rtype = 34;
        let bar_type = decode_bar_type(&msg, instrument_id).unwrap();
        assert_eq!(bar_type, BarType::from("ESM4.GLBX-1-HOUR-LAST-EXTERNAL"));

        // Test 1-day bar
        msg.hd.rtype = 35;
        let bar_type = decode_bar_type(&msg, instrument_id).unwrap();
        assert_eq!(bar_type, BarType::from("ESM4.GLBX-1-DAY-LAST-EXTERNAL"));

        // Test unsupported rtype
        msg.hd.rtype = 99;
        let result = decode_bar_type(&msg, instrument_id);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_decode_ts_event_adjustment() {
        let mut msg = dbn::OhlcvMsg::default_for_schema(dbn::Schema::Ohlcv1S);

        // Test 1-second bar adjustment
        msg.hd.rtype = 32;
        let adjustment = decode_ts_event_adjustment(&msg).unwrap();
        assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1S);

        // Test 1-minute bar adjustment
        msg.hd.rtype = 33;
        let adjustment = decode_ts_event_adjustment(&msg).unwrap();
        assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1M);

        // Test 1-hour bar adjustment
        msg.hd.rtype = 34;
        let adjustment = decode_ts_event_adjustment(&msg).unwrap();
        assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1H);

        // Test 1-day bar adjustment
        msg.hd.rtype = 35;
        let adjustment = decode_ts_event_adjustment(&msg).unwrap();
        assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1D);

        // Test eod bar adjustment (same as 1d)
        msg.hd.rtype = 36;
        let adjustment = decode_ts_event_adjustment(&msg).unwrap();
        assert_eq!(adjustment, BAR_CLOSE_ADJUSTMENT_1D);

        // Test unsupported rtype
        msg.hd.rtype = 99;
        let result = decode_ts_event_adjustment(&msg);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_decode_record() {
        // Test with MBO message
        let path = test_data_path().join("test_data.mbo.dbn.zst");
        let decoder = Decoder::from_zstd_file(path).unwrap();
        let mut dbn_stream = decoder.decode_stream::<dbn::MboMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let record_ref = dbn::RecordRef::from(msg);
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let (data1, data2) =
            decode_record(&record_ref, instrument_id, 2, Some(0.into()), true, false).unwrap();

        assert!(data1.is_some());
        assert!(data2.is_none());

        // Test with Trade message
        let path = test_data_path().join("test_data.trades.dbn.zst");
        let decoder = Decoder::from_zstd_file(path).unwrap();
        let mut dbn_stream = decoder.decode_stream::<dbn::TradeMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let record_ref = dbn::RecordRef::from(msg);

        let (data1, data2) =
            decode_record(&record_ref, instrument_id, 2, Some(0.into()), true, false).unwrap();

        assert!(data1.is_some());
        assert!(data2.is_none());
        assert!(matches!(data1.unwrap(), Data::Trade(_)));
    }
}
