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
    collections::hash_map::DefaultHasher,
    ffi::c_char,
    hash::{Hash, Hasher},
};

use nautilus_core::{string::str_to_cstr, time::UnixNanos};

use super::book::{BookOrder, OrderBookDelta};
use crate::{
    enums::{BookAction, OrderSide},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

#[no_mangle]
pub extern "C" fn book_order_from_raw(
    order_side: OrderSide,
    price_raw: i64,
    price_prec: u8,
    size_raw: u64,
    size_prec: u8,
    order_id: u64,
) -> BookOrder {
    BookOrder::new(
        order_side,
        Price::from_raw(price_raw, price_prec),
        Quantity::from_raw(size_raw, size_prec),
        order_id,
    )
}

#[no_mangle]
pub extern "C" fn book_order_eq(lhs: &BookOrder, rhs: &BookOrder) -> u8 {
    u8::from(lhs == rhs)
}

#[no_mangle]
pub extern "C" fn book_order_hash(order: &BookOrder) -> u64 {
    let mut hasher = DefaultHasher::new();
    order.hash(&mut hasher);
    hasher.finish()
}

#[no_mangle]
pub extern "C" fn book_order_exposure(order: &BookOrder) -> f64 {
    order.exposure()
}

#[no_mangle]
pub extern "C" fn book_order_signed_size(order: &BookOrder) -> f64 {
    order.signed_size()
}

/// Returns a [`BookOrder`] display string as a C string pointer.
#[no_mangle]
pub extern "C" fn book_order_display_to_cstr(order: &BookOrder) -> *const c_char {
    str_to_cstr(&format!("{}", order))
}

/// Returns a [`BookOrder`] debug string as a C string pointer.
#[no_mangle]
pub extern "C" fn book_order_debug_to_cstr(order: &BookOrder) -> *const c_char {
    str_to_cstr(&format!("{:?}", order))
}

#[no_mangle]
pub extern "C" fn orderbook_delta_drop(delta: OrderBookDelta) {
    drop(delta); // Memory freed here
}

#[no_mangle]
pub extern "C" fn orderbook_delta_clone(delta: &OrderBookDelta) -> OrderBookDelta {
    delta.clone()
}

#[no_mangle]
pub extern "C" fn orderbook_delta_new(
    instrument_id: InstrumentId,
    action: BookAction,
    order: BookOrder,
    flags: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
}

#[no_mangle]
pub extern "C" fn orderbook_delta_eq(lhs: &OrderBookDelta, rhs: &OrderBookDelta) -> u8 {
    u8::from(lhs == rhs)
}

#[no_mangle]
pub extern "C" fn orderbook_delta_hash(delta: &OrderBookDelta) -> u64 {
    let mut hasher = DefaultHasher::new();
    delta.hash(&mut hasher);
    hasher.finish()
}
