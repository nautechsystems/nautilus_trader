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
    instruments::equity::Equity,
    types::{currency::Currency, price::Price, quantity::Quantity},
};
use rust_decimal_macros::dec;

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

pub fn parse_raw_symbol_to_string(raw_symbol: [c_char; dbn::SYMBOL_CSTR_LEN]) -> Result<String> {
    let c_str: &CStr = unsafe { CStr::from_ptr(raw_symbol.as_ptr()) };
    let str_slice: &str = c_str.to_str().map_err(|e| anyhow!(e))?;
    Ok(str_slice.to_owned())
}

pub fn parse_equity(
    record: dbn::InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> Result<Equity> {
    // Use USD for all US equities venues for now
    let currency = Currency::USD();

    Equity::new(
        instrument_id,
        instrument_id.symbol,
        // Symbol::from_str(&parse_raw_symbol_to_string(record.raw_symbol)?)?,
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
