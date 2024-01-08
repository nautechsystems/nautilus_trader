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
    collections::hash_map::DefaultHasher,
    ffi::c_char,
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{
    ffi::string::{cstr_to_str, str_to_cstr},
    time::UnixNanos,
};

use crate::{
    data::bar::{Bar, BarSpecification, BarType},
    enums::{AggregationSource, BarAggregation, PriceType},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

#[no_mangle]
pub extern "C" fn bar_specification_new(
    step: usize,
    aggregation: u8,
    price_type: u8,
) -> BarSpecification {
    let aggregation =
        BarAggregation::from_repr(aggregation as usize).expect("cannot parse enum value");
    let price_type = PriceType::from_repr(price_type as usize).expect("cannot parse enum value");
    BarSpecification {
        step,
        aggregation,
        price_type,
    }
}

/// Returns a [`BarSpecification`] as a C string pointer.
#[no_mangle]
pub extern "C" fn bar_specification_to_cstr(bar_spec: &BarSpecification) -> *const c_char {
    str_to_cstr(&bar_spec.to_string())
}

#[no_mangle]
pub extern "C" fn bar_specification_hash(bar_spec: &BarSpecification) -> u64 {
    let mut h = DefaultHasher::new();
    bar_spec.hash(&mut h);
    h.finish()
}

#[no_mangle]
pub extern "C" fn bar_specification_eq(lhs: &BarSpecification, rhs: &BarSpecification) -> u8 {
    u8::from(lhs == rhs)
}

#[no_mangle]
pub extern "C" fn bar_specification_lt(lhs: &BarSpecification, rhs: &BarSpecification) -> u8 {
    u8::from(lhs < rhs)
}

#[no_mangle]
pub extern "C" fn bar_specification_le(lhs: &BarSpecification, rhs: &BarSpecification) -> u8 {
    u8::from(lhs <= rhs)
}

#[no_mangle]
pub extern "C" fn bar_specification_gt(lhs: &BarSpecification, rhs: &BarSpecification) -> u8 {
    u8::from(lhs > rhs)
}

#[no_mangle]
pub extern "C" fn bar_specification_ge(lhs: &BarSpecification, rhs: &BarSpecification) -> u8 {
    u8::from(lhs >= rhs)
}

#[no_mangle]
pub extern "C" fn bar_type_new(
    instrument_id: InstrumentId,
    spec: BarSpecification,
    aggregation_source: u8,
) -> BarType {
    let aggregation_source = AggregationSource::from_repr(aggregation_source as usize)
        .expect("Error converting enum from integer");
    BarType {
        instrument_id,
        spec,
        aggregation_source,
    }
}

/// Returns any [`BarType`] parsing error from the provided C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn bar_type_check_parsing(ptr: *const c_char) -> *const c_char {
    match BarType::from_str(cstr_to_str(ptr)) {
        Ok(_) => str_to_cstr(""),
        Err(e) => str_to_cstr(&e.to_string()),
    }
}

/// Returns a [`BarType`] from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn bar_type_from_cstr(ptr: *const c_char) -> BarType {
    BarType::from(cstr_to_str(ptr))
}

#[no_mangle]
pub extern "C" fn bar_type_eq(lhs: &BarType, rhs: &BarType) -> u8 {
    u8::from(lhs == rhs)
}

#[no_mangle]
pub extern "C" fn bar_type_lt(lhs: &BarType, rhs: &BarType) -> u8 {
    u8::from(lhs < rhs)
}

#[no_mangle]
pub extern "C" fn bar_type_le(lhs: &BarType, rhs: &BarType) -> u8 {
    u8::from(lhs <= rhs)
}

#[no_mangle]
pub extern "C" fn bar_type_gt(lhs: &BarType, rhs: &BarType) -> u8 {
    u8::from(lhs > rhs)
}

#[no_mangle]
pub extern "C" fn bar_type_ge(lhs: &BarType, rhs: &BarType) -> u8 {
    u8::from(lhs >= rhs)
}

#[no_mangle]
pub extern "C" fn bar_type_hash(bar_type: &BarType) -> u64 {
    let mut h = DefaultHasher::new();
    bar_type.hash(&mut h);
    h.finish()
}

/// Returns a [`BarType`] as a C string pointer.
#[no_mangle]
pub extern "C" fn bar_type_to_cstr(bar_type: &BarType) -> *const c_char {
    str_to_cstr(&bar_type.to_string())
}

#[no_mangle]
pub extern "C" fn bar_new(
    bar_type: BarType,
    open: Price,
    high: Price,
    low: Price,
    close: Price,
    volume: Quantity,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Bar {
    Bar {
        bar_type,
        open,
        high,
        low,
        close,
        volume,
        ts_event,
        ts_init,
    }
}

#[no_mangle]
pub extern "C" fn bar_new_from_raw(
    bar_type: BarType,
    open: i64,
    high: i64,
    low: i64,
    close: i64,
    price_prec: u8,
    volume: u64,
    size_prec: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Bar {
    Bar {
        bar_type,
        open: Price::from_raw(open, price_prec).unwrap(),
        high: Price::from_raw(high, price_prec).unwrap(),
        low: Price::from_raw(low, price_prec).unwrap(),
        close: Price::from_raw(close, price_prec).unwrap(),
        volume: Quantity::from_raw(volume, size_prec).unwrap(),
        ts_event,
        ts_init,
    }
}

#[no_mangle]
pub extern "C" fn bar_eq(lhs: &Bar, rhs: &Bar) -> u8 {
    u8::from(lhs == rhs)
}

#[no_mangle]
pub extern "C" fn bar_hash(bar: &Bar) -> u64 {
    let mut h = DefaultHasher::new();
    bar.hash(&mut h);
    h.finish()
}

/// Returns a [`Bar`] as a C string.
#[no_mangle]
pub extern "C" fn bar_to_cstr(bar: &Bar) -> *const c_char {
    str_to_cstr(&bar.to_string())
}
