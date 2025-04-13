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

use std::{cmp, ffi::c_char, num::NonZeroUsize};

use databento::dbn::{self};
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
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
    types::{Currency, Price, Quantity, price::decode_raw_price_i64},
};
use ustr::Ustr;

use super::{
    enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction},
    types::{DatabentoImbalance, DatabentoStatistics},
};

const DATABENTO_FIXED_SCALAR: f64 = 1_000_000_000.0;

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

pub fn parse_option_kind(c: c_char) -> anyhow::Result<OptionKind> {
    match c as u8 as char {
        'C' => Ok(OptionKind::Call),
        'P' => Ok(OptionKind::Put),
        invalid => anyhow::bail!("Invalid `OptionKind`, was '{invalid}'"),
    }
}

fn parse_currency_or_usd_default(value: Result<&str, impl std::error::Error>) -> Currency {
    match value {
        Ok(value) if !value.is_empty() => {
            Currency::try_from_str(value).unwrap_or_else(Currency::USD)
        }
        Ok(_) => Currency::USD(),
        Err(e) => {
            log::error!("Error parsing currency: {e}");
            Currency::USD()
        }
    }
}

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

// https://databento.com/docs/schemas-and-data-formats/status#types-of-status-reasons
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

/// Decodes a quantity from the given optional value, expressed in standard whole-number units.
#[must_use]
pub fn decode_optional_quantity(value: i32) -> Option<Quantity> {
    match value {
        i32::MAX => None,
        _ => Some(Quantity::from(value)),
    }
}

/// Decodes a multiplier from the given value, expressed in units of 1e-9.
#[must_use]
pub fn decode_multiplier(value: i64) -> Quantity {
    match value {
        0 | i64::MAX => Quantity::from(1),
        _ => Quantity::from(format!("{}", value as f64 / DATABENTO_FIXED_SCALAR)),
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

pub fn decode_equity_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<Equity> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

    Equity::new_checked(
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
    )
}

pub fn decode_futures_contract_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FuturesContract> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = Ustr::from(msg.asset()?);
    let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?)?;
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

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

pub fn decode_futures_spread_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FuturesSpread> {
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = Ustr::from(msg.asset()?);
    let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?)?;
    let strategy_type = Ustr::from(msg.secsubtype()?);
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

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

pub fn decode_option_contract_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
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
    let multiplier = decode_multiplier(msg.unit_of_measure_qty);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

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

pub fn decode_option_spread_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<OptionSpread> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = Ustr::from(msg.underlying()?);
    let asset_class_opt = if instrument_id.venue.as_str() == "OPRA" {
        Some(AssetClass::Equity)
    } else {
        let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?)?;
        asset_class
    };
    let strategy_type = Ustr::from(msg.secsubtype()?);
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

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

#[must_use]
fn is_trade_msg(order_side: OrderSide, action: c_char) -> bool {
    order_side == OrderSide::NoOrderSide || action as u8 as char == 'T'
}

pub fn decode_mbo_msg(
    msg: &dbn::MboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
    include_trades: bool,
) -> anyhow::Result<(Option<OrderBookDelta>, Option<TradeTick>)> {
    let side = parse_order_side(msg.side);
    if is_trade_msg(side, msg.action) {
        if include_trades {
            let trade = TradeTick::new(
                instrument_id,
                Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
                Quantity::from(msg.size),
                parse_aggressor_side(msg.side),
                TradeId::new(itoa::Buffer::new().format(msg.sequence)),
                msg.ts_recv.into(),
                ts_init,
            );
            return Ok((None, Some(trade)));
        }

        return Ok((None, None));
    }

    let order = BookOrder::new(
        side,
        Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
        Quantity::from(msg.size),
        msg.order_id,
    );

    let delta = OrderBookDelta::new(
        instrument_id,
        parse_book_action(msg.action)?,
        order,
        msg.flags.raw(),
        msg.sequence.into(),
        msg.ts_recv.into(),
        ts_init,
    );

    Ok((Some(delta), None))
}

pub fn decode_trade_msg(
    msg: &dbn::TradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let trade = TradeTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
        Quantity::from(msg.size),
        parse_aggressor_side(msg.side),
        TradeId::new(itoa::Buffer::new().format(msg.sequence)),
        msg.ts_recv.into(),
        ts_init,
    );

    Ok(trade)
}

pub fn decode_tbbo_msg(
    msg: &dbn::TbboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<(QuoteTick, TradeTick)> {
    let top_level = &msg.levels[0];
    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(top_level.bid_px), price_precision),
        Price::from_raw(decode_raw_price_i64(top_level.ask_px), price_precision),
        Quantity::from(top_level.bid_sz),
        Quantity::from(top_level.ask_sz),
        msg.ts_recv.into(),
        ts_init,
    );

    let trade = TradeTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
        Quantity::from(msg.size),
        parse_aggressor_side(msg.side),
        TradeId::new(itoa::Buffer::new().format(msg.sequence)),
        msg.ts_recv.into(),
        ts_init,
    );

    Ok((quote, trade))
}

pub fn decode_mbp1_msg(
    msg: &dbn::Mbp1Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
    include_trades: bool,
) -> anyhow::Result<(QuoteTick, Option<TradeTick>)> {
    let top_level = &msg.levels[0];
    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(top_level.bid_px), price_precision),
        Price::from_raw(decode_raw_price_i64(top_level.ask_px), price_precision),
        Quantity::from(top_level.bid_sz),
        Quantity::from(top_level.ask_sz),
        msg.ts_recv.into(),
        ts_init,
    );

    let maybe_trade = if include_trades && msg.action as u8 as char == 'T' {
        Some(TradeTick::new(
            instrument_id,
            Price::from_raw(decode_raw_price_i64(msg.price), price_precision),
            Quantity::from(msg.size),
            parse_aggressor_side(msg.side),
            TradeId::new(itoa::Buffer::new().format(msg.sequence)),
            msg.ts_recv.into(),
            ts_init,
        ))
    } else {
        None
    };

    Ok((quote, maybe_trade))
}

pub fn decode_bbo_msg(
    msg: &dbn::BboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let top_level = &msg.levels[0];
    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(decode_raw_price_i64(top_level.bid_px), price_precision),
        Price::from_raw(decode_raw_price_i64(top_level.ask_px), price_precision),
        Quantity::from(top_level.bid_sz),
        Quantity::from(top_level.ask_sz),
        msg.ts_recv.into(),
        ts_init,
    );

    Ok(quote)
}

pub fn decode_mbp10_msg(
    msg: &dbn::Mbp10Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
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

    let bids: [BookOrder; DEPTH10_LEN] = bids.try_into().expect("`bids` length != 10");
    let asks: [BookOrder; DEPTH10_LEN] = asks.try_into().expect("`asks` length != 10");
    let bid_counts: [u32; DEPTH10_LEN] = bid_counts.try_into().expect("`bid_counts` length != 10");
    let ask_counts: [u32; DEPTH10_LEN] = ask_counts.try_into().expect("`ask_counts` length != 10");

    let depth = OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        bid_counts,
        ask_counts,
        msg.flags.raw(),
        msg.sequence.into(),
        msg.ts_recv.into(),
        ts_init,
    );

    Ok(depth)
}

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
            //  ohlcv-1m
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
        _ => anyhow::bail!(
            "`rtype` is not a supported bar aggregation, was {}",
            msg.hd.rtype
        ),
    };

    Ok(bar_type)
}

pub fn decode_ts_event_adjustment(msg: &dbn::OhlcvMsg) -> anyhow::Result<UnixNanos> {
    let adjustment = match msg.hd.rtype {
        32 => {
            // ohlcv-1s
            BAR_CLOSE_ADJUSTMENT_1S
        }
        33 => {
            //  ohlcv-1m
            BAR_CLOSE_ADJUSTMENT_1M
        }
        34 => {
            //  ohlcv-1h
            BAR_CLOSE_ADJUSTMENT_1H
        }
        35 => {
            // ohlcv-1d
            BAR_CLOSE_ADJUSTMENT_1D
        }
        _ => anyhow::bail!(
            "`rtype` is not a supported bar aggregation, was {}",
            msg.hd.rtype
        ),
    };

    Ok(adjustment.into())
}

pub fn decode_ohlcv_msg(
    msg: &dbn::OhlcvMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let bar_type = decode_bar_type(msg, instrument_id)?;
    let ts_event_adjustment = decode_ts_event_adjustment(msg)?;

    // Adjust `ts_event` from open to close of bar
    let ts_event = UnixNanos::from(msg.hd.ts_event);
    let ts_init = cmp::max(ts_init, ts_event) + ts_event_adjustment;

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

pub fn decode_status_msg(
    msg: &dbn::StatusMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentStatus> {
    let status = InstrumentStatus::new(
        instrument_id,
        MarketStatusAction::from_u16(msg.action).expect("Invalid `MarketStatusAction`"),
        msg.hd.ts_event.into(),
        ts_init,
        parse_status_reason(msg.reason)?,
        parse_status_trading_event(msg.trading_event)?,
        parse_optional_bool(msg.is_trading),
        parse_optional_bool(msg.is_quoting),
        parse_optional_bool(msg.is_short_sell_restricted),
    );

    Ok(status)
}

pub fn decode_record(
    record: &dbn::RecordRef,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
    include_trades: bool,
) -> anyhow::Result<(Option<Data>, Option<Data>)> {
    // We don't handle `TbboMsg` here as Nautilus separates this schema
    // into quotes and trades when loading, and the live client will
    // never subscribe to `tbbo`.
    let result = if let Some(msg) = record.get::<dbn::MboMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let result = decode_mbo_msg(msg, instrument_id, price_precision, ts_init, include_trades)?;
        match result {
            (Some(delta), None) => (Some(Data::Delta(delta)), None),
            (None, Some(trade)) => (Some(Data::Trade(trade)), None),
            (None, None) => (None, None),
            _ => anyhow::bail!("Invalid `MboMsg` parsing combination"),
        }
    } else if let Some(msg) = record.get::<dbn::TradeMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let trade = decode_trade_msg(msg, instrument_id, price_precision, ts_init)?;
        (Some(Data::Trade(trade)), None)
    } else if let Some(msg) = record.get::<dbn::Mbp1Msg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let result = decode_mbp1_msg(msg, instrument_id, price_precision, ts_init, include_trades)?;
        match result {
            (quote, None) => (Some(Data::Quote(quote)), None),
            (quote, Some(trade)) => (Some(Data::Quote(quote)), Some(Data::Trade(trade))),
        }
    } else if let Some(msg) = record.get::<dbn::Bbo1SMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let quote = decode_bbo_msg(msg, instrument_id, price_precision, ts_init)?;
        (Some(Data::Quote(quote)), None)
    } else if let Some(msg) = record.get::<dbn::Bbo1MMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let quote = decode_bbo_msg(msg, instrument_id, price_precision, ts_init)?;
        (Some(Data::Quote(quote)), None)
    } else if let Some(msg) = record.get::<dbn::Mbp10Msg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let depth = decode_mbp10_msg(msg, instrument_id, price_precision, ts_init)?;
        (Some(Data::from(depth)), None)
    } else if let Some(msg) = record.get::<dbn::OhlcvMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.hd.ts_event.into());
        let bar = decode_ohlcv_msg(msg, instrument_id, price_precision, ts_init)?;
        (Some(Data::Bar(bar)), None)
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

pub fn decode_instrument_def_msg_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    match msg.instrument_class as u8 as char {
        'K' => Ok(InstrumentAny::Equity(decode_equity_v1(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'F' => Ok(InstrumentAny::FuturesContract(decode_futures_contract_v1(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'S' => Ok(InstrumentAny::FuturesSpread(decode_futures_spread_v1(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'C' | 'P' => Ok(InstrumentAny::OptionContract(decode_option_contract_v1(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'T' | 'M' => Ok(InstrumentAny::OptionSpread(decode_option_spread_v1(
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

pub fn decode_instrument_def_msg(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
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

pub fn decode_equity(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<Equity> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

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

pub fn decode_futures_contract(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FuturesContract> {
    let currency = parse_currency_or_usd_default(msg.currency());
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = Ustr::from(msg.asset()?);
    let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?)?;
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

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

pub fn decode_futures_spread(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FuturesSpread> {
    let exchange = Ustr::from(msg.exchange()?);
    let underlying = Ustr::from(msg.asset()?);
    let (asset_class, _) = parse_cfi_iso10926(msg.cfi()?)?;
    let strategy_type = Ustr::from(msg.secsubtype()?);
    let currency = parse_currency_or_usd_default(msg.currency());
    let price_increment = decode_price_increment(msg.min_price_increment, currency.precision);
    let multiplier = decode_multiplier(msg.unit_of_measure_qty);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

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

pub fn decode_option_contract(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
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
    let multiplier = decode_multiplier(msg.unit_of_measure_qty);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

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

pub fn decode_option_spread(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
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
    let multiplier = decode_multiplier(msg.unit_of_measure_qty);
    let lot_size = decode_lot_size(msg.min_lot_size_round_lot);
    let ts_event = UnixNanos::from(msg.ts_recv); // More accurate and reliable timestamp

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

pub fn decode_imbalance_msg(
    msg: &dbn::ImbalanceMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<DatabentoImbalance> {
    DatabentoImbalance::new(
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
        msg.ts_recv.into(),
        ts_init,
    )
}

pub fn decode_statistics_msg(
    msg: &dbn::StatMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<DatabentoStatistics> {
    let stat_type = DatabentoStatisticType::from_u8(msg.stat_type as u8)
        .expect("Invalid value for `stat_type`");
    let update_action = DatabentoStatisticUpdateAction::from_u8(msg.update_action)
        .expect("Invalid value for `update_action`");

    DatabentoStatistics::new(
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
        msg.ts_recv.into(),
        ts_init,
    )
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
    #[case(i32::MAX, None)] // None for i32::MAX
    #[case(0, Some(Quantity::new(0.0, 0)))] // 0 is valid quantity
    #[case(10, Some(Quantity::new(10.0, 0)))] // Arbitrary valid quantity
    fn test_decode_optional_quantity(#[case] value: i32, #[case] expected: Option<Quantity>) {
        let actual = decode_optional_quantity(value);
        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_decode_mbo_msg() {
        let path = test_data_path().join("test_data.mbo.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::MboMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let (delta, _) = decode_mbo_msg(msg, instrument_id, 2, 0.into(), false).unwrap();
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
    fn test_decode_mbp1_msg() {
        let path = test_data_path().join("test_data.mbp-1.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::Mbp1Msg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let (quote, _) = decode_mbp1_msg(msg, instrument_id, 2, 0.into(), false).unwrap();

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
        let quote = decode_bbo_msg(msg, instrument_id, 2, 0.into()).unwrap();

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("5199.50"));
        assert_eq!(quote.ask_price, Price::from("5199.75"));
        assert_eq!(quote.bid_size, Quantity::from("26"));
        assert_eq!(quote.ask_size, Quantity::from("23"));
        assert_eq!(quote.ts_event, msg.ts_recv);
        assert_eq!(quote.ts_event, 1715248801000000000);
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
        let quote = decode_bbo_msg(msg, instrument_id, 2, 0.into()).unwrap();

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("5199.50"));
        assert_eq!(quote.ask_price, Price::from("5199.75"));
        assert_eq!(quote.bid_size, Quantity::from("33"));
        assert_eq!(quote.ask_size, Quantity::from("17"));
        assert_eq!(quote.ts_event, msg.ts_recv);
        assert_eq!(quote.ts_event, 1715248800000000000);
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
        let depth10 = decode_mbp10_msg(msg, instrument_id, 2, 0.into()).unwrap();

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
        let trade = decode_trade_msg(msg, instrument_id, 2, 0.into()).unwrap();

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
        let (quote, trade) = decode_tbbo_msg(msg, instrument_id, 2, 0.into()).unwrap();

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

    #[ignore = "Requires updated test data"]
    #[rstest]
    fn test_decode_ohlcv_msg() {
        let path = test_data_path().join("test_data.ohlcv-1s.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::OhlcvMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let bar = decode_ohlcv_msg(msg, instrument_id, 2, 0.into()).unwrap();

        assert_eq!(
            bar.bar_type,
            BarType::from("ESM4.GLBX-1-SECOND-LAST-EXTERNAL")
        );
        assert_eq!(bar.open, Price::from("3720.25"));
        assert_eq!(bar.high, Price::from("3720.50"));
        assert_eq!(bar.low, Price::from("3720.25"));
        assert_eq!(bar.close, Price::from("3720.50"));
        assert_eq!(bar.ts_event, 1_609_160_400_000_000_000);
        assert_eq!(bar.ts_init, 1_609_160_401_000_000_000); // Adjusted to open + interval
    }

    #[rstest]
    fn test_decode_definition_msg() {
        let path = test_data_path().join("test_data.definition.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::InstrumentDefMsg>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let result = decode_instrument_def_msg(msg, instrument_id, 0.into());

        assert!(result.is_ok());
        assert_eq!(result.unwrap().multiplier(), Quantity::from(1));
    }

    #[rstest]
    fn test_decode_definition_v1_msg() {
        let path = test_data_path().join("test_data.definition.v1.dbn.zst");
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::compat::InstrumentDefMsgV1>();
        let msg = dbn_stream.next().unwrap().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let result = decode_instrument_def_msg_v1(msg, instrument_id, 0.into());

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
        let status = decode_status_msg(msg, instrument_id, 0.into()).unwrap();

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
        let imbalance = decode_imbalance_msg(msg, instrument_id, 2, 0.into()).unwrap();

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
        let statistics = decode_statistics_msg(msg, instrument_id, 2, 0.into()).unwrap();

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
}
