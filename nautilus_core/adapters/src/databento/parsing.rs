// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use anyhow::{anyhow, bail, Result};
use dbn;
use itoa;
use nautilus_core::{datetime::secs_to_nanos, time::UnixNanos};
use nautilus_model::{
    data::{
        bar::{Bar, BarSpecification, BarType},
        delta::OrderBookDelta,
        order::BookOrder,
        quote::QuoteTick,
        trade::TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction, InstrumentClass,
        OptionKind, OrderSide, PriceType,
    },
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    instruments::{
        equity::Equity, futures_contract::FuturesContract, options_contract::OptionsContract,
        Instrument,
    },
    types::{currency::Currency, price::Price, quantity::Quantity},
};
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::{common::nautilus_instrument_id_from_databento, types::DatabentoPublisher};

pub fn parse_order_side(c: c_char) -> OrderSide {
    match c as u8 as char {
        'A' => OrderSide::Sell,
        'B' => OrderSide::Buy,
        _ => OrderSide::NoOrderSide,
    }
}

pub fn parse_aggressor_side(c: c_char) -> AggressorSide {
    match c as u8 as char {
        'A' => AggressorSide::Seller,
        'B' => AggressorSide::Buyer,
        _ => AggressorSide::NoAggressor,
    }
}

pub fn parse_book_action(c: c_char) -> Result<BookAction> {
    match c as u8 as char {
        'A' => Ok(BookAction::Add),
        'C' => Ok(BookAction::Delete),
        'F' => Ok(BookAction::Update),
        'M' => Ok(BookAction::Update),
        'R' => Ok(BookAction::Clear),
        _ => bail!("Invalid `BookAction`, was '{c}'"),
    }
}

pub fn parse_option_kind(c: c_char) -> Result<OptionKind> {
    match c as u8 as char {
        'C' => Ok(OptionKind::Call),
        'P' => Ok(OptionKind::Put),
        _ => bail!("Invalid `OptionKind`, was '{c}'"),
    }
}

pub fn parse_cfi_iso10926(value: &str) -> Result<(Option<AssetClass>, Option<InstrumentClass>)> {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 3 {
        bail!("Value string is too short");
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

pub fn parse_min_price_increment(value: i64, currency: Currency) -> Result<Price> {
    match value {
        0 | i64::MAX => Price::new(10f64.powi(-(currency.precision as i32)), currency.precision),
        _ => Price::from_raw(value, currency.precision),
    }
}

/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
pub unsafe fn parse_raw_ptr_to_string(ptr: *const c_char) -> Result<String> {
    let c_str: &CStr = unsafe { CStr::from_ptr(ptr) };
    let str_slice: &str = c_str.to_str().map_err(|e| anyhow!(e))?;
    Ok(str_slice.to_owned())
}

/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
pub unsafe fn parse_raw_ptr_to_ustr(ptr: *const c_char) -> Result<Ustr> {
    let c_str: &CStr = unsafe { CStr::from_ptr(ptr) };
    let str_slice: &str = c_str.to_str().map_err(|e| anyhow!(e))?;
    Ok(Ustr::from(str_slice))
}

pub fn parse_equity(
    record: dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> Result<Equity> {
    let currency = Currency::USD(); // TODO: Temporary hard coding of US equities for now

    Equity::new(
        instrument_id,
        instrument_id.symbol,
        None, // No ISIN available yet
        currency,
        currency.precision,
        parse_min_price_increment(record.min_price_increment, currency)?,
        dec!(0), // TBD
        dec!(0), // TBD
        dec!(0), // TBD
        dec!(0), // TBD
        Some(Quantity::new(record.min_lot_size_round_lot.into(), 0)?),
        None,           // TBD
        None,           // TBD
        None,           // TBD
        None,           // TBD
        record.ts_recv, // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn parse_futures_contract(
    record: dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> Result<FuturesContract> {
    let currency = Currency::USD(); // TODO: Temporary hard coding of US futures for now
    let cfi_str = unsafe { parse_raw_ptr_to_string(record.cfi.as_ptr())? };
    let asset = unsafe { parse_raw_ptr_to_ustr(record.asset.as_ptr())? };
    let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;

    FuturesContract::new(
        instrument_id,
        instrument_id.symbol,
        asset_class.unwrap_or(AssetClass::Commodity),
        asset,
        record.activation,
        record.expiration,
        currency,
        currency.precision,
        parse_min_price_increment(record.min_price_increment, currency)?,
        dec!(0), // TBD
        dec!(0), // TBD
        dec!(0), // TBD
        dec!(0), // TBD
        Quantity::new(record.contract_multiplier.into(), 0)?,
        None,           // TBD
        None,           // TBD
        None,           // TBD
        None,           // TBD
        None,           // TBD
        record.ts_recv, // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn parse_options_contract(
    record: dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> Result<OptionsContract> {
    let currency_str = unsafe { parse_raw_ptr_to_string(record.currency.as_ptr())? };
    let cfi_str = unsafe { parse_raw_ptr_to_string(record.cfi.as_ptr())? };
    let currency = Currency::from_str(&currency_str)?;
    let (asset_class, _) = parse_cfi_iso10926(&cfi_str)?;
    let lot_size = Quantity::new(1.0, 0)?;

    OptionsContract::new(
        instrument_id,
        instrument_id.symbol,
        asset_class.unwrap_or(AssetClass::Commodity),
        unsafe { parse_raw_ptr_to_ustr(record.asset.as_ptr())? },
        parse_option_kind(record.instrument_class)?,
        record.activation,
        record.expiration,
        Price::from_raw(record.strike_price, currency.precision)?,
        currency,
        currency.precision,
        parse_min_price_increment(record.min_price_increment, currency)?,
        dec!(0), // TBD
        dec!(0), // TBD
        dec!(0), // TBD
        dec!(0), // TBD
        Some(lot_size),
        None,           // TBD
        None,           // TBD
        None,           // TBD
        None,           // TBD
        record.ts_recv, // More accurate and reliable timestamp
        ts_init,
    )
}

pub fn is_trade_msg(order_side: OrderSide, action: c_char) -> bool {
    order_side == OrderSide::NoOrderSide || action as u8 as char == 'T'
}

pub fn parse_mbo_msg(
    record: dbn::MboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<Option<OrderBookDelta>> {
    let side = parse_order_side(record.side);
    if is_trade_msg(side, record.action) {
        return Ok(None);
    }

    let order = BookOrder::new(
        side,
        Price::from_raw(record.price, price_precision)?,
        Quantity::from_raw(record.size.into(), 0)?,
        record.order_id,
    );

    let delta = OrderBookDelta::new(
        instrument_id,
        parse_book_action(record.action)?,
        order,
        record.flags,
        record.sequence.into(),
        record.ts_recv,
        ts_init,
    );

    Ok(Some(delta))
}

pub fn parse_mbo_msg_trades(
    record: dbn::MboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<Option<TradeTick>> {
    if !is_trade_msg(parse_order_side(record.side), record.action) {
        return Ok(None);
    }

    let trade = TradeTick::new(
        instrument_id,
        Price::from_raw(record.price, price_precision)?,
        Quantity::from_raw(record.size.into(), 0)?,
        parse_aggressor_side(record.side),
        TradeId::new(itoa::Buffer::new().format(record.sequence))?,
        record.ts_recv,
        ts_init,
    );

    Ok(Some(trade))
}

pub fn parse_trade_msg(
    record: dbn::TradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<TradeTick> {
    let trade = TradeTick::new(
        instrument_id,
        Price::from_raw(record.price, price_precision)?,
        Quantity::from_raw(record.size.into(), 0)?,
        parse_aggressor_side(record.side),
        TradeId::new(itoa::Buffer::new().format(record.sequence))?,
        record.ts_recv,
        ts_init,
    );

    Ok(trade)
}

pub fn parse_mbp1_msg(
    record: dbn::Mbp1Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<Option<QuoteTick>> {
    let top_level = &record.levels[0];
    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(top_level.bid_px, price_precision)?,
        Price::from_raw(top_level.ask_px, price_precision)?,
        Quantity::from_raw(top_level.bid_sz.into(), 0)?,
        Quantity::from_raw(top_level.ask_sz.into(), 0)?,
        record.ts_recv,
        ts_init,
    )?;

    Ok(Some(quote))
}

pub fn parse_mbp1_msg_trades(
    record: dbn::Mbp1Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<Option<TradeTick>> {
    if record.action as u8 as char != 'T' {
        return Ok(None);
    }

    let trade = TradeTick::new(
        instrument_id,
        Price::from_raw(record.price, price_precision)?,
        Quantity::from_raw(record.size.into(), 0)?,
        parse_aggressor_side(record.side),
        TradeId::new(itoa::Buffer::new().format(record.sequence))?,
        record.ts_recv,
        ts_init,
    );

    Ok(Some(trade))
}

pub fn parse_mbp10_msg(
    record: dbn::Mbp1Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<Vec<OrderBookDelta>> {
    let mut deltas = Vec::with_capacity(21);
    let clear = OrderBookDelta::clear(
        instrument_id,
        record.sequence.into(),
        record.ts_recv,
        ts_init,
    );
    deltas.push(clear);

    for level in record.levels {
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from_raw(level.bid_px, price_precision)?,
            Quantity::from_raw(level.bid_sz.into(), 0)?,
            0,
        );
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            bid_order,
            record.flags,
            record.sequence.into(),
            record.ts_recv,
            ts_init,
        );

        deltas.push(delta);

        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from_raw(level.ask_px, price_precision)?,
            Quantity::from_raw(level.ask_sz.into(), 0)?,
            0,
        );
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            ask_order,
            record.flags,
            record.sequence.into(),
            record.ts_recv,
            ts_init,
        );

        deltas.push(delta);
    }

    Ok(deltas)
}

pub fn parse_bar_type(record: dbn::OhlcvMsg, instrument_id: InstrumentId) -> Result<BarType> {
    match record.hd.rtype {
        32 => {
            // ohlcv-1s
            let bar_spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
            let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);
            Ok(bar_type)
        }
        33 => {
            //  ohlcv-1m
            let bar_spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
            let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);
            Ok(bar_type)
        }
        34 => {
            // ohlcv-1h
            let bar_spec = BarSpecification::new(1, BarAggregation::Hour, PriceType::Last);
            let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);
            Ok(bar_type)
        }
        35 => {
            // ohlcv-1d
            let bar_spec = BarSpecification::new(1, BarAggregation::Day, PriceType::Last);
            let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);
            Ok(bar_type)
        }
        _ => bail!(
            "`rtype` is not a supported bar aggregation, was {}",
            record.hd.rtype
        ),
    }
}

pub fn parse_ts_event_adjustment(record: dbn::OhlcvMsg) -> Result<UnixNanos> {
    match record.hd.rtype {
        32 => {
            // ohlcv-1s
            Ok(secs_to_nanos(1.0))
        }
        33 => {
            //  ohlcv-1m
            Ok(secs_to_nanos(60.0))
        }
        34 => {
            //  ohlcv-1h
            Ok(secs_to_nanos(60.0 * 60.0))
        }
        35 => {
            // ohlcv-1d
            Ok(secs_to_nanos(60.0 * 60.0 * 24.0))
        }
        _ => bail!(
            "`rtype` is not a supported bar aggregation, was {}",
            record.hd.rtype
        ),
    }
}

pub fn parse_ohlcv_msg(
    record: dbn::OhlcvMsg,
    bar_type: BarType,
    price_precision: u8,
    ts_event_adjustment: UnixNanos,
    ts_init: UnixNanos,
) -> Result<Bar> {
    // Adjust `ts_event` from open to close of bar
    let ts_event = record.hd.ts_event + ts_event_adjustment;
    let ts_init = cmp::max(ts_init, ts_event);

    let bar = Bar::new(
        bar_type,
        Price::from_raw(record.open / 100, price_precision)?, // TODO(adjust for display factor)
        Price::from_raw(record.high / 100, price_precision)?, // TODO(adjust for display factor)
        Price::from_raw(record.low / 100, price_precision)?,  // TODO(adjust for display factor)
        Price::from_raw(record.close / 100, price_precision)?, // TODO(adjust for display factor)
        Quantity::from_raw(record.volume, 0)?,                // TODO(adjust for display factor)
        ts_event,
        ts_init,
    );

    Ok(bar)
}

// pub fn parse_record_with_metadata<T>(
//     record: T,
//     publishers: IndexMap<PublisherId, DatabentoPublisher>,
//     ts_init: UnixNanos,
// ) -> Result<Data>
// where
//     T: dbn::Record,
// {
//     let publisher_id: PublisherId = record.header().publisher_id;
//     let publisher = publishers
//         .get(&record.header().publisher_id)
//         .ok_or_else(|| anyhow!("Publisher ID {publisher_id} not found in map"))?;
//     match record.rtype() {
//         dbn::RType::InstrumentDef => parse_instrument_def_msg(record, publisher, ts_init)?,
//         _ => bail!("OOPS!"),
//     }
// }

pub fn parse_instrument_def_msg(
    record: dbn::InstrumentDefMsg,
    publisher: &DatabentoPublisher,
    ts_init: UnixNanos,
) -> Result<Box<dyn Instrument>> {
    let raw_symbol = unsafe { parse_raw_ptr_to_ustr(record.raw_symbol.as_ptr())? };
    let instrument_id = nautilus_instrument_id_from_databento(raw_symbol, publisher);

    match record.instrument_class as u8 as char {
        'C' | 'P' => Ok(Box::new(parse_options_contract(
            record,
            instrument_id,
            ts_init,
        )?)),
        'K' => Ok(Box::new(parse_equity(record, instrument_id, ts_init)?)),
        'F' => Ok(Box::new(parse_futures_contract(
            record,
            instrument_id,
            ts_init,
        )?)),
        'X' => bail!("Unsupported `instrument_class` 'X' (FX_SPOT)"),
        _ => bail!(
            "Invalid `instrument_class`, was {}",
            record.instrument_class
        ),
    }
}
