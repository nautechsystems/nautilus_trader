// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    cmp,
    ffi::{c_char, CStr},
    i64,
    str::FromStr,
};

use databento::dbn::{self};
use nautilus_core::{datetime::NANOSECONDS_IN_SECOND, nanos::UnixNanos};
use nautilus_model::{
    data::{
        bar::{Bar, BarSpecification, BarType},
        delta::OrderBookDelta,
        depth::{OrderBookDepth10, DEPTH10_LEN},
        order::BookOrder,
        quote::QuoteTick,
        trade::TradeTick,
        Data,
    },
    enums::{
        AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction, FromU8,
        InstrumentClass, OptionKind, OrderSide, PriceType,
    },
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    instruments::{
        any::InstrumentAny, equity::Equity, futures_contract::FuturesContract,
        futures_spread::FuturesSpread, options_contract::OptionsContract,
        options_spread::OptionsSpread,
    },
    types::{currency::Currency, fixed::FIXED_SCALAR, price::Price, quantity::Quantity},
};
use ustr::Ustr;

use super::{
    enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction},
    types::{DatabentoImbalance, DatabentoStatistics},
};

const BAR_SPEC_1S: BarSpecification = BarSpecification {
    step: 1,
    aggregation: BarAggregation::Second,
    price_type: PriceType::Last,
};
const BAR_SPEC_1M: BarSpecification = BarSpecification {
    step: 1,
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
const BAR_SPEC_1H: BarSpecification = BarSpecification {
    step: 1,
    aggregation: BarAggregation::Hour,
    price_type: PriceType::Last,
};
const BAR_SPEC_1D: BarSpecification = BarSpecification {
    step: 1,
    aggregation: BarAggregation::Day,
    price_type: PriceType::Last,
};

const BAR_CLOSE_ADJUSTMENT_1S: u64 = NANOSECONDS_IN_SECOND;
const BAR_CLOSE_ADJUSTMENT_1M: u64 = NANOSECONDS_IN_SECOND * 60;
const BAR_CLOSE_ADJUSTMENT_1H: u64 = NANOSECONDS_IN_SECOND * 60 * 60;
const BAR_CLOSE_ADJUSTMENT_1D: u64 = NANOSECONDS_IN_SECOND * 60 * 60 * 24;

#[must_use]
pub fn parse_order_side(c: c_char) -> OrderSide {
    match c as u8 as char {
        'A' => OrderSide::Sell,
        'B' => OrderSide::Buy,
        _ => OrderSide::NoOrderSide,
    }
}

#[must_use]
pub fn parse_aggressor_side(c: c_char) -> AggressorSide {
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
        _ => anyhow::bail!("Invalid `BookAction`, was '{c}'"),
    }
}

pub fn parse_option_kind(c: c_char) -> anyhow::Result<OptionKind> {
    match c as u8 as char {
        'C' => Ok(OptionKind::Call),
        'P' => Ok(OptionKind::Put),
        _ => anyhow::bail!("Invalid `OptionKind`, was '{c}'"),
    }
}

pub fn parse_cfi_iso10926(
    value: &str,
) -> anyhow::Result<(Option<AssetClass>, Option<InstrumentClass>)> {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 3 {
        anyhow::bail!("Value string is too short");
    }

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

pub fn decode_price(value: i64, precision: u8) -> anyhow::Result<Price> {
    match value {
        0 | i64::MAX => Price::new(10f64.powi(-i32::from(precision)), precision),
        _ => Price::from_raw(value, precision),
    }
}

pub fn decode_optional_price(value: i64, precision: u8) -> anyhow::Result<Option<Price>> {
    match value {
        i64::MAX => Ok(None),
        _ => Ok(Some(Price::from_raw(value, precision)?)),
    }
}

pub fn decode_optional_quantity_i32(value: i32) -> anyhow::Result<Option<Quantity>> {
    match value {
        i32::MAX => Ok(None),
        _ => Ok(Some(Quantity::new(f64::from(value), 0)?)),
    }
}

/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
pub unsafe fn raw_ptr_to_string(ptr: *const c_char) -> anyhow::Result<String> {
    let c_str: &CStr = unsafe { CStr::from_ptr(ptr) };
    let str_slice: &str = c_str.to_str().map_err(|e| anyhow::anyhow!(e))?;
    Ok(str_slice.to_owned())
}

/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
pub unsafe fn raw_ptr_to_ustr(ptr: *const c_char) -> anyhow::Result<Ustr> {
    let c_str: &CStr = unsafe { CStr::from_ptr(ptr) };
    let str_slice: &str = c_str.to_str().map_err(|e| anyhow::anyhow!(e))?;
    Ok(Ustr::from(str_slice))
}

pub fn decode_equity_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<Equity> {
    let currency = Currency::USD(); // TODO: Temporary hard coding of US equities for now

    Equity::new(
        instrument_id,
        instrument_id.symbol,
        None, // No ISIN available yet
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        Some(Quantity::new(msg.min_lot_size_round_lot.into(), 0)?),
        None,               // TBD
        None,               // TBD
        None,               // TBD
        None,               // TBD
        msg.ts_recv.into(), // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn decode_futures_contract_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FuturesContract> {
    let currency = Currency::USD(); // TODO: Temporary hard coding of US futures for now
    let cfi_str = unsafe { raw_ptr_to_string(msg.cfi.as_ptr())? };
    let exchange = unsafe { raw_ptr_to_ustr(msg.exchange.as_ptr())? };
    let underlying = unsafe { raw_ptr_to_ustr(msg.asset.as_ptr())? };
    let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;

    FuturesContract::new(
        instrument_id,
        instrument_id.symbol,
        asset_class.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        msg.activation.into(),
        msg.expiration.into(),
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        Quantity::new(1.0, 0)?, // TBD
        Quantity::new(1.0, 0)?, // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        msg.ts_recv.into(),     // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn decode_futures_spread_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FuturesSpread> {
    let currency = Currency::USD(); // TODO: Temporary hard coding of US futures for now
    let cfi_str = unsafe { raw_ptr_to_string(msg.cfi.as_ptr())? };
    let exchange = unsafe { raw_ptr_to_ustr(msg.exchange.as_ptr())? };
    let underlying = unsafe { raw_ptr_to_ustr(msg.asset.as_ptr())? };
    let strategy_type = unsafe { raw_ptr_to_ustr(msg.secsubtype.as_ptr())? };
    let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;

    FuturesSpread::new(
        instrument_id,
        instrument_id.symbol,
        asset_class.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        strategy_type,
        msg.activation.into(),
        msg.expiration.into(),
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        Quantity::new(1.0, 0)?, // TBD
        Quantity::new(1.0, 0)?, // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        msg.ts_recv.into(),     // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn decode_options_contract_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<OptionsContract> {
    let currency_str = unsafe { raw_ptr_to_string(msg.currency.as_ptr())? };
    let cfi_str = unsafe { raw_ptr_to_string(msg.cfi.as_ptr())? };
    let exchange = unsafe { raw_ptr_to_ustr(msg.exchange.as_ptr())? };
    let asset_class_opt = if instrument_id.venue.as_str() == "OPRA" {
        Some(AssetClass::Equity)
    } else {
        let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;
        asset_class
    };
    let underlying = unsafe { raw_ptr_to_ustr(msg.underlying.as_ptr())? };
    let currency = Currency::from_str(&currency_str)?;

    OptionsContract::new(
        instrument_id,
        instrument_id.symbol,
        asset_class_opt.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        parse_option_kind(msg.instrument_class)?,
        msg.activation.into(),
        msg.expiration.into(),
        Price::from_raw(msg.strike_price, currency.precision)?,
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        Quantity::new(1.0, 0)?, // TBD
        Quantity::new(1.0, 0)?, // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,
        None,
        msg.ts_recv.into(), // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn decode_options_spread_v1(
    msg: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<OptionsSpread> {
    let currency_str = unsafe { raw_ptr_to_string(msg.currency.as_ptr())? };
    let cfi_str = unsafe { raw_ptr_to_string(msg.cfi.as_ptr())? };
    let exchange = unsafe { raw_ptr_to_ustr(msg.exchange.as_ptr())? };
    let asset_class_opt = if instrument_id.venue.as_str() == "OPRA" {
        Some(AssetClass::Equity)
    } else {
        let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;
        asset_class
    };
    let underlying = unsafe { raw_ptr_to_ustr(msg.underlying.as_ptr())? };
    let strategy_type = unsafe { raw_ptr_to_ustr(msg.secsubtype.as_ptr())? };
    let currency = Currency::from_str(&currency_str)?;

    OptionsSpread::new(
        instrument_id,
        instrument_id.symbol,
        asset_class_opt.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        strategy_type,
        msg.activation.into(),
        msg.expiration.into(),
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        Quantity::new(1.0, 0)?, // TBD
        Quantity::new(1.0, 0)?, // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        msg.ts_recv.into(),     // More accurate and reliable timestamp
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
                Price::from_raw(msg.price, price_precision)?,
                Quantity::from_raw(u64::from(msg.size) * FIXED_SCALAR as u64, 0)?,
                parse_aggressor_side(msg.side),
                TradeId::new(itoa::Buffer::new().format(msg.sequence))?,
                msg.ts_recv.into(),
                ts_init,
            );
            return Ok((None, Some(trade)));
        }

        return Ok((None, None));
    };

    let order = BookOrder::new(
        side,
        Price::from_raw(msg.price, price_precision)?,
        Quantity::from_raw(u64::from(msg.size) * FIXED_SCALAR as u64, 0)?,
        msg.order_id,
    );

    let delta = OrderBookDelta::new(
        instrument_id,
        parse_book_action(msg.action)?,
        order,
        msg.flags,
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
        Price::from_raw(msg.price, price_precision)?,
        Quantity::from_raw(u64::from(msg.size) * FIXED_SCALAR as u64, 0)?,
        parse_aggressor_side(msg.side),
        TradeId::new(itoa::Buffer::new().format(msg.sequence))?,
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
        Price::from_raw(top_level.bid_px, price_precision)?,
        Price::from_raw(top_level.ask_px, price_precision)?,
        Quantity::from_raw(u64::from(top_level.bid_sz) * FIXED_SCALAR as u64, 0)?,
        Quantity::from_raw(u64::from(top_level.ask_sz) * FIXED_SCALAR as u64, 0)?,
        msg.ts_recv.into(),
        ts_init,
    )?;

    let trade = TradeTick::new(
        instrument_id,
        Price::from_raw(msg.price, price_precision)?,
        Quantity::from_raw(u64::from(msg.size) * FIXED_SCALAR as u64, 0)?,
        parse_aggressor_side(msg.side),
        TradeId::new(itoa::Buffer::new().format(msg.sequence))?,
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
        Price::from_raw(top_level.bid_px, price_precision)?,
        Price::from_raw(top_level.ask_px, price_precision)?,
        Quantity::from_raw(u64::from(top_level.bid_sz) * FIXED_SCALAR as u64, 0)?,
        Quantity::from_raw(u64::from(top_level.ask_sz) * FIXED_SCALAR as u64, 0)?,
        msg.ts_recv.into(),
        ts_init,
    )?;

    let maybe_trade = if include_trades && msg.action as u8 as char == 'T' {
        Some(TradeTick::new(
            instrument_id,
            Price::from_raw(msg.price, price_precision)?,
            Quantity::from_raw(u64::from(msg.size) * FIXED_SCALAR as u64, 0)?,
            parse_aggressor_side(msg.side),
            TradeId::new(itoa::Buffer::new().format(msg.sequence))?,
            msg.ts_recv.into(),
            ts_init,
        ))
    } else {
        None
    };

    Ok((quote, maybe_trade))
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
            Price::from_raw(level.bid_px, price_precision)?,
            Quantity::from_raw(u64::from(level.bid_sz) * FIXED_SCALAR as u64, 0)?,
            0,
        );

        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from_raw(level.ask_px, price_precision)?,
            Quantity::from_raw(u64::from(level.ask_sz) * FIXED_SCALAR as u64, 0)?,
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
        msg.flags,
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
    let ts_event = msg.hd.ts_event.into();
    let ts_init = cmp::max(ts_init, ts_event) + ts_event_adjustment;

    // Adjust raw prices by a display factor
    let mut display_factor = 1;
    if instrument_id.venue.as_str() == "GLBX" {
        display_factor = 100;
    };

    let bar = Bar::new(
        bar_type,
        Price::from_raw(msg.open / display_factor, price_precision)?,
        Price::from_raw(msg.high / display_factor, price_precision)?,
        Price::from_raw(msg.low / display_factor, price_precision)?,
        Price::from_raw(msg.close / display_factor, price_precision)?,
        Quantity::from_raw(msg.volume * FIXED_SCALAR as u64, 0)?,
        ts_event,
        ts_init,
    );

    Ok(bar)
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
    } else if let Some(msg) = record.get::<dbn::Mbp10Msg>() {
        let ts_init = determine_timestamp(ts_init, msg.ts_recv.into());
        let depth = decode_mbp10_msg(msg, instrument_id, price_precision, ts_init)?;
        (Some(Data::Depth10(depth)), None)
    } else if let Some(msg) = record.get::<dbn::OhlcvMsg>() {
        let ts_init = determine_timestamp(ts_init, msg.hd.ts_event.into());
        let bar = decode_ohlcv_msg(msg, instrument_id, price_precision, ts_init)?;
        (Some(Data::Bar(bar)), None)
    } else {
        anyhow::bail!("DBN message type is not currently supported")
    };

    Ok(result)
}

fn determine_timestamp(ts_init: Option<UnixNanos>, msg_timestamp: UnixNanos) -> UnixNanos {
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
        'C' | 'P' => Ok(InstrumentAny::OptionsContract(decode_options_contract_v1(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'T' | 'M' => Ok(InstrumentAny::OptionsSpread(decode_options_spread_v1(
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
        'C' | 'P' => Ok(InstrumentAny::OptionsContract(decode_options_contract(
            msg,
            instrument_id,
            ts_init,
        )?)),
        'T' | 'M' => Ok(InstrumentAny::OptionsSpread(decode_options_spread(
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
    let currency = Currency::USD(); // TODO: Temporary hard coding of US equities for now

    Equity::new(
        instrument_id,
        instrument_id.symbol,
        None, // No ISIN available yet
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        None, // TBD
        None, // TBD
        None, // TBD
        None, // TBD
        Some(Quantity::new(msg.min_lot_size_round_lot.into(), 0)?),
        None,               // TBD
        None,               // TBD
        None,               // TBD
        None,               // TBD
        msg.ts_recv.into(), // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn decode_futures_contract(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FuturesContract> {
    let currency = Currency::USD(); // TODO: Temporary hard coding of US futures for now
    let cfi_str = unsafe { raw_ptr_to_string(msg.cfi.as_ptr())? };
    let exchange = unsafe { raw_ptr_to_ustr(msg.exchange.as_ptr())? };
    let underlying = unsafe { raw_ptr_to_ustr(msg.asset.as_ptr())? };
    let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;

    FuturesContract::new(
        instrument_id,
        instrument_id.symbol,
        asset_class.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        msg.activation.into(),
        msg.expiration.into(),
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        Quantity::new(1.0, 0)?, // TBD
        Quantity::new(1.0, 0)?, // TBD
        None,
        None,               // TBD
        None,               // TBD
        None,               // TBD
        None,               // TBD
        None,               // TBD
        msg.ts_recv.into(), // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn decode_futures_spread(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FuturesSpread> {
    let currency = Currency::USD(); // TODO: Temporary hard coding of US futures for now
    let cfi_str = unsafe { raw_ptr_to_string(msg.cfi.as_ptr())? };
    let exchange = unsafe { raw_ptr_to_ustr(msg.exchange.as_ptr())? };
    let underlying = unsafe { raw_ptr_to_ustr(msg.asset.as_ptr())? };
    let strategy_type = unsafe { raw_ptr_to_ustr(msg.secsubtype.as_ptr())? };
    let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;

    FuturesSpread::new(
        instrument_id,
        instrument_id.symbol,
        asset_class.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        strategy_type,
        msg.activation.into(),
        msg.expiration.into(),
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        Quantity::new(1.0, 0)?, // TBD
        Quantity::new(1.0, 0)?, // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        msg.ts_recv.into(),     // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn decode_options_contract(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<OptionsContract> {
    let currency_str = unsafe { raw_ptr_to_string(msg.currency.as_ptr())? };
    let cfi_str = unsafe { raw_ptr_to_string(msg.cfi.as_ptr())? };
    let exchange = unsafe { raw_ptr_to_ustr(msg.exchange.as_ptr())? };
    let asset_class_opt = if instrument_id.venue.as_str() == "OPRA" {
        Some(AssetClass::Equity)
    } else {
        let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;
        asset_class
    };
    let underlying = unsafe { raw_ptr_to_ustr(msg.underlying.as_ptr())? };
    let currency = Currency::from_str(&currency_str)?;

    OptionsContract::new(
        instrument_id,
        instrument_id.symbol,
        asset_class_opt.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        parse_option_kind(msg.instrument_class)?,
        msg.activation.into(),
        msg.expiration.into(),
        Price::from_raw(msg.strike_price, currency.precision)?,
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        Quantity::new(1.0, 0)?, // TBD
        Quantity::new(1.0, 0)?, // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        msg.ts_recv.into(),     // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn decode_options_spread(
    msg: &dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<OptionsSpread> {
    let currency_str = unsafe { raw_ptr_to_string(msg.currency.as_ptr())? };
    let cfi_str = unsafe { raw_ptr_to_string(msg.cfi.as_ptr())? };
    let asset_class_opt = if instrument_id.venue.as_str() == "OPRA" {
        Some(AssetClass::Equity)
    } else {
        let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;
        asset_class
    };
    let exchange = unsafe { raw_ptr_to_ustr(msg.exchange.as_ptr())? };
    let underlying = unsafe { raw_ptr_to_ustr(msg.underlying.as_ptr())? };
    let strategy_type = unsafe { raw_ptr_to_ustr(msg.secsubtype.as_ptr())? };
    let currency = Currency::from_str(&currency_str)?;

    OptionsSpread::new(
        instrument_id,
        instrument_id.symbol,
        asset_class_opt.unwrap_or(AssetClass::Commodity),
        Some(exchange),
        underlying,
        strategy_type,
        msg.activation.into(),
        msg.expiration.into(),
        currency,
        currency.precision,
        decode_price(msg.min_price_increment, currency.precision)?,
        Quantity::new(1.0, 0)?, // TBD
        Quantity::new(1.0, 0)?, // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        None,                   // TBD
        msg.ts_recv.into(),     // More accurate and reliable timestamp
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
        Price::from_raw(msg.ref_price, price_precision)?,
        Price::from_raw(msg.cont_book_clr_price, price_precision)?,
        Price::from_raw(msg.auct_interest_clr_price, price_precision)?,
        Quantity::new(f64::from(msg.paired_qty), 0)?,
        Quantity::new(f64::from(msg.total_imbalance_qty), 0)?,
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
        decode_optional_price(msg.price, price_precision)?,
        decode_optional_quantity_i32(msg.quantity)?,
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
    use std::path::PathBuf;

    use databento::dbn::decode::{dbn::Decoder, DecodeStream};
    use rstest::*;
    use streaming_iterator::StreamingIterator;

    use super::*;

    pub const TEST_DATA_PATH: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/databento/test_data");

    #[rstest]
    fn test_decode_mbo_msg() {
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.mbo.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::MboMsg>();
        let msg = dbn_stream.next().unwrap();

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
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.mbp-1.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::Mbp1Msg>();
        let msg = dbn_stream.next().unwrap();

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
    fn test_decode_mbp10_msg() {
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.mbp-10.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::Mbp10Msg>();
        let msg = dbn_stream.next().unwrap();

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
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.trades.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::TradeMsg>();
        let msg = dbn_stream.next().unwrap();

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
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.tbbo.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::Mbp1Msg>();
        let msg = dbn_stream.next().unwrap();

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

    #[rstest]
    fn test_decode_ohlcv_msg() {
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.ohlcv-1s.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::OhlcvMsg>();
        let msg = dbn_stream.next().unwrap();

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
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.definition.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::InstrumentDefMsg>();
        let msg = dbn_stream.next().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let result = decode_instrument_def_msg(msg, instrument_id, 0.into());

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_decode_definition_v1_msg() {
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.definition.v1.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::compat::InstrumentDefMsgV1>();
        let msg = dbn_stream.next().unwrap();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let result = decode_instrument_def_msg_v1(msg, instrument_id, 0.into());

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_decode_imbalance_msg() {
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.imbalance.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::ImbalanceMsg>();
        let msg = dbn_stream.next().unwrap();

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
        let path = PathBuf::from(format!("{TEST_DATA_PATH}/test_data.statistics.dbn.zst"));
        let mut dbn_stream = Decoder::from_zstd_file(path)
            .unwrap()
            .decode_stream::<dbn::StatMsg>();
        let msg = dbn_stream.next().unwrap();

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
