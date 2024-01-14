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

use anyhow::{anyhow, bail, Result};
use databento::dbn;
use itoa;
use nautilus_core::{datetime::NANOSECONDS_IN_SECOND, time::UnixNanos};
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
        AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction, InstrumentClass,
        OptionKind, OrderSide, PriceType,
    },
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    instruments::{
        equity::Equity, futures_contract::FuturesContract, options_contract::OptionsContract,
        Instrument,
    },
    types::{currency::Currency, fixed::FIXED_SCALAR, price::Price, quantity::Quantity},
};
use ustr::Ustr;

use super::{common::nautilus_instrument_id_from_databento, types::DatabentoPublisher};

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
        0 | i64::MAX => Price::new(
            10f64.powi(-i32::from(currency.precision)),
            currency.precision,
        ),
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
    record: &dbn::compat::InstrumentDefMsgV1,
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
    record: &dbn::compat::InstrumentDefMsgV1,
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
    record: &dbn::compat::InstrumentDefMsgV1,
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
        Some(lot_size),
        None,           // TBD
        None,           // TBD
        None,           // TBD
        None,           // TBD
        record.ts_recv, // More accurate and reliable timestamp
        ts_init,
    )
}

#[must_use]
pub fn is_trade_msg(order_side: OrderSide, action: c_char) -> bool {
    order_side == OrderSide::NoOrderSide || action as u8 as char == 'T'
}

pub fn parse_mbo_msg(
    record: &dbn::MboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<(Option<OrderBookDelta>, Option<TradeTick>)> {
    let side = parse_order_side(record.side);
    if is_trade_msg(side, record.action) {
        let trade = TradeTick::new(
            instrument_id,
            Price::from_raw(record.price, price_precision)?,
            Quantity::from_raw(record.size as u64 * FIXED_SCALAR as u64, 0)?,
            parse_aggressor_side(record.side),
            TradeId::new(itoa::Buffer::new().format(record.sequence))?,
            record.ts_recv,
            ts_init,
        );
        return Ok((None, Some(trade)));
    };

    let order = BookOrder::new(
        side,
        Price::from_raw(record.price, price_precision)?,
        Quantity::from_raw(record.size as u64 * FIXED_SCALAR as u64, 0)?,
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

    Ok((Some(delta), None))
}

pub fn parse_trade_msg(
    record: &dbn::TradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<TradeTick> {
    let trade = TradeTick::new(
        instrument_id,
        Price::from_raw(record.price, price_precision)?,
        Quantity::from_raw(record.size as u64 * FIXED_SCALAR as u64, 0)?,
        parse_aggressor_side(record.side),
        TradeId::new(itoa::Buffer::new().format(record.sequence))?,
        record.ts_recv,
        ts_init,
    );

    Ok(trade)
}

pub fn parse_mbp1_msg(
    record: &dbn::Mbp1Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<(QuoteTick, Option<TradeTick>)> {
    let top_level = &record.levels[0];
    let quote = QuoteTick::new(
        instrument_id,
        Price::from_raw(top_level.bid_px, price_precision)?,
        Price::from_raw(top_level.ask_px, price_precision)?,
        Quantity::from_raw(top_level.bid_sz as u64 * FIXED_SCALAR as u64, 0)?,
        Quantity::from_raw(top_level.ask_sz as u64 * FIXED_SCALAR as u64, 0)?,
        record.ts_recv,
        ts_init,
    )?;

    let trade = match record.action as u8 as char {
        'T' => Some(TradeTick::new(
            instrument_id,
            Price::from_raw(record.price, price_precision)?,
            Quantity::from_raw(record.size as u64 * FIXED_SCALAR as u64, 0)?,
            parse_aggressor_side(record.side),
            TradeId::new(itoa::Buffer::new().format(record.sequence))?,
            record.ts_recv,
            ts_init,
        )),
        _ => None,
    };

    Ok((quote, trade))
}

pub fn parse_mbp10_msg(
    record: &dbn::Mbp10Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<OrderBookDepth10> {
    let mut bids = Vec::with_capacity(DEPTH10_LEN);
    let mut asks = Vec::with_capacity(DEPTH10_LEN);
    let mut bid_counts = Vec::with_capacity(DEPTH10_LEN);
    let mut ask_counts = Vec::with_capacity(DEPTH10_LEN);

    for level in &record.levels {
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from_raw(level.bid_px, price_precision)?,
            Quantity::from_raw(level.bid_sz as u64 * FIXED_SCALAR as u64, 0)?,
            0,
        );

        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from_raw(level.ask_px, price_precision)?,
            Quantity::from_raw(level.ask_sz as u64 * FIXED_SCALAR as u64, 0)?,
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
        record.flags,
        record.sequence.into(),
        record.ts_recv,
        ts_init,
    );

    Ok(depth)
}

pub fn parse_bar_type(record: &dbn::OhlcvMsg, instrument_id: InstrumentId) -> Result<BarType> {
    let bar_type = match record.hd.rtype {
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
        _ => bail!(
            "`rtype` is not a supported bar aggregation, was {}",
            record.hd.rtype
        ),
    };

    Ok(bar_type)
}

pub fn parse_ts_event_adjustment(record: &dbn::OhlcvMsg) -> Result<UnixNanos> {
    let adjustment = match record.hd.rtype {
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
        _ => bail!(
            "`rtype` is not a supported bar aggregation, was {}",
            record.hd.rtype
        ),
    };

    Ok(adjustment)
}

pub fn parse_ohlcv_msg(
    record: &dbn::OhlcvMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<Bar> {
    let bar_type = parse_bar_type(record, instrument_id)?;
    let ts_event_adjustment = parse_ts_event_adjustment(record)?;

    // Adjust `ts_event` from open to close of bar
    let ts_event = record.hd.ts_event;
    let ts_init = cmp::max(ts_init, ts_event) + ts_event_adjustment;

    let bar = Bar::new(
        bar_type,
        Price::from_raw(record.open / 100, price_precision)?, // TODO(adjust for display factor)
        Price::from_raw(record.high / 100, price_precision)?, // TODO(adjust for display factor)
        Price::from_raw(record.low / 100, price_precision)?,  // TODO(adjust for display factor)
        Price::from_raw(record.close / 100, price_precision)?, // TODO(adjust for display factor)
        Quantity::from_raw(record.volume * FIXED_SCALAR as u64, 0)?, // TODO(adjust for display factor)
        ts_event,
        ts_init,
    );

    Ok(bar)
}

pub fn parse_record(
    record: &dbn::RecordRef,
    rtype: dbn::RType,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> Result<(Data, Option<Data>)> {
    let result = match rtype {
        dbn::RType::Mbo => {
            let msg = record.get::<dbn::MboMsg>().unwrap(); // SAFETY: RType known
            let ts_init = match ts_init {
                Some(ts_init) => ts_init,
                None => msg.ts_recv,
            };
            let result = parse_mbo_msg(msg, instrument_id, price_precision, ts_init)?;
            match result {
                (Some(delta), None) => (Data::Delta(delta), None),
                (None, Some(trade)) => (Data::Trade(trade), None),
                _ => bail!("Invalid `MboMsg` parsing combination"),
            }
        }
        dbn::RType::Mbp0 => {
            let msg = record.get::<dbn::TradeMsg>().unwrap(); // SAFETY: RType known
            let ts_init = match ts_init {
                Some(ts_init) => ts_init,
                None => msg.ts_recv,
            };
            let trade = parse_trade_msg(msg, instrument_id, price_precision, ts_init)?;
            (Data::Trade(trade), None)
        }
        dbn::RType::Mbp1 => {
            let msg = record.get::<dbn::Mbp1Msg>().unwrap(); // SAFETY: RType known
            let ts_init = match ts_init {
                Some(ts_init) => ts_init,
                None => msg.ts_recv,
            };
            let result = parse_mbp1_msg(msg, instrument_id, price_precision, ts_init)?;
            match result {
                (quote, None) => (Data::Quote(quote), None),
                (quote, Some(trade)) => (Data::Quote(quote), Some(Data::Trade(trade))),
            }
        }
        dbn::RType::Mbp10 => {
            let msg = record.get::<dbn::Mbp10Msg>().unwrap(); // SAFETY: RType known
            let ts_init = match ts_init {
                Some(ts_init) => ts_init,
                None => msg.ts_recv,
            };
            let depth = parse_mbp10_msg(msg, instrument_id, price_precision, ts_init)?;
            (Data::Depth10(depth), None)
        }
        dbn::RType::Ohlcv1S
        | dbn::RType::Ohlcv1M
        | dbn::RType::Ohlcv1H
        | dbn::RType::Ohlcv1D
        | dbn::RType::OhlcvEod => {
            let msg = record.get::<dbn::OhlcvMsg>().unwrap(); // SAFETY: RType known
            let ts_init = match ts_init {
                Some(ts_init) => ts_init,
                None => msg.hd.ts_event,
            };
            let bar = parse_ohlcv_msg(msg, instrument_id, price_precision, ts_init)?;
            (Data::Bar(bar), None)
        }
        _ => bail!("RType {:?} is not currently supported", rtype),
    };

    Ok(result)
}

pub fn parse_instrument_def_msg(
    record: &dbn::compat::InstrumentDefMsgV1,
    publisher: &DatabentoPublisher,
    ts_init: UnixNanos,
) -> Result<Box<dyn Instrument>> {
    let raw_symbol = unsafe { parse_raw_ptr_to_ustr(record.raw_symbol.as_ptr())? };
    let instrument_id = nautilus_instrument_id_from_databento(raw_symbol, publisher);

    match record.instrument_class as u8 as char {
        'K' => Ok(Box::new(parse_equity(record, instrument_id, ts_init)?)),
        'F' => Ok(Box::new(parse_futures_contract(
            record,
            instrument_id,
            ts_init,
        )?)),
        'C' | 'P' => Ok(Box::new(parse_options_contract(
            record,
            instrument_id,
            ts_init,
        )?)),
        'B' => bail!("Unsupported `instrument_class` 'B' (BOND)"),
        'M' => bail!("Unsupported `instrument_class` 'M' (MIXEDSPREAD)"),
        'S' => bail!("Unsupported `instrument_class` 'S' (FUTURESPREAD)"),
        'T' => bail!("Unsupported `instrument_class` 'T' (OPTIONSPREAD)"),
        'X' => bail!("Unsupported `instrument_class` 'X' (FX_SPOT)"),
        _ => bail!(
            "Unsupported `instrument_class` '{}'",
            record.instrument_class as u8 as char
        ),
    }
}
