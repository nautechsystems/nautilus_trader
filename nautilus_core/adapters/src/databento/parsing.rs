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
    ffi::{c_char, CStr},
    i64,
};

use anyhow::{anyhow, bail, Result};
use dbn::{InstrumentDefMsg, SYMBOL_CSTR_LEN};
use nautilus_core::time::UnixNanos;
use nautilus_model::{
    enums::{AggressorSide, AssetClass, AssetType, BookAction, OptionKind, OrderSide},
    identifiers::instrument_id::InstrumentId,
    instruments::equity::Equity,
    types::{currency::Currency, price::Price, quantity::Quantity},
};
use rust_decimal_macros::dec;

pub fn parse_order_side(c: char) -> OrderSide {
    match c {
        'A' => OrderSide::Sell,
        'B' => OrderSide::Buy,
        _ => OrderSide::NoOrderSide,
    }
}

pub fn parse_aggressor_side(c: char) -> AggressorSide {
    match c {
        'A' => AggressorSide::Seller,
        'B' => AggressorSide::Buyer,
        _ => AggressorSide::NoAggressor,
    }
}

pub fn parse_book_action(c: char) -> Result<BookAction> {
    match c {
        'A' => Ok(BookAction::Add),
        'C' => Ok(BookAction::Delete),
        'F' => Ok(BookAction::Update),
        'M' => Ok(BookAction::Update),
        'R' => Ok(BookAction::Clear),
        _ => bail!("Invalid `BookAction`, was '{c}'"),
    }
}

pub fn parse_option_kind(c: char) -> Result<OptionKind> {
    match c {
        'C' => Ok(OptionKind::Call),
        'P' => Ok(OptionKind::Put),
        _ => bail!("Invalid `OptionKind`, was '{c}'"),
    }
}

pub fn parse_cfi_iso10926(value: &str) -> Result<(Option<AssetClass>, Option<AssetType>)> {
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
        'D' => Some(AssetClass::Bond),
        'E' => Some(AssetClass::Equity),
        'S' => None,
        _ => None,
    };

    let asset_type = match cfi_group {
        'I' => Some(AssetType::Future),
        _ => None,
    };

    if cfi_attribute1 == 'I' {
        asset_class = Some(AssetClass::Index);
    }

    Ok((asset_class, asset_type))
}

pub fn parse_min_price_increment(value: i64, currency: Currency) -> Result<Price> {
    match value {
        0 | i64::MAX => Price::new(10f64.powi(-(currency.precision as i32)), currency.precision),
        _ => Price::from_raw(value, currency.precision),
    }
}

pub fn parse_raw_symbol_to_string(raw_symbol: [c_char; SYMBOL_CSTR_LEN]) -> Result<String> {
    let c_str: &CStr = unsafe { CStr::from_ptr(raw_symbol.as_ptr()) };
    let str_slice: &str = c_str.to_str().map_err(|e| anyhow!(e))?;
    Ok(str_slice.to_owned())
}

pub fn parse_equity(
    record: InstrumentDefMsg,
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
        Quantity::new(1.0, 0)?,
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
