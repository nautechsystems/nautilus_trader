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
};

use nautilus_core::ffi::string::str_to_cstr;

use crate::{
    data::order::BookOrder,
    enums::OrderSide,
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
        Price::from_raw(price_raw, price_prec).unwrap(),
        Quantity::from_raw(size_raw, size_prec).unwrap(),
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
    str_to_cstr(&format!("{order}"))
}

/// Returns a [`BookOrder`] debug string as a C string pointer.
#[no_mangle]
pub extern "C" fn book_order_debug_to_cstr(order: &BookOrder) -> *const c_char {
    str_to_cstr(&format!("{order:?}"))
}
