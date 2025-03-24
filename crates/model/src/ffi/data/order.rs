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

use std::{
    collections::hash_map::DefaultHasher,
    ffi::c_char,
    hash::{Hash, Hasher},
};

use nautilus_core::ffi::string::str_to_cstr;

use crate::{
    data::BookOrder,
    enums::OrderSide,
    types::{Price, Quantity},
};

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn book_order_new(
    order_side: OrderSide,
    price: Price,
    size: Quantity,
    order_id: u64,
) -> BookOrder {
    BookOrder::new(order_side, price, size, order_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn book_order_eq(lhs: &BookOrder, rhs: &BookOrder) -> u8 {
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
pub extern "C" fn book_order_hash(order: &BookOrder) -> u64 {
    let mut hasher = DefaultHasher::new();
    order.hash(&mut hasher);
    hasher.finish()
}

#[unsafe(no_mangle)]
pub extern "C" fn book_order_exposure(order: &BookOrder) -> f64 {
    order.exposure()
}

#[unsafe(no_mangle)]
pub extern "C" fn book_order_signed_size(order: &BookOrder) -> f64 {
    order.signed_size()
}

/// Returns a [`BookOrder`] display string as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn book_order_display_to_cstr(order: &BookOrder) -> *const c_char {
    str_to_cstr(&format!("{order}"))
}

/// Returns a [`BookOrder`] debug string as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn book_order_debug_to_cstr(order: &BookOrder) -> *const c_char {
    str_to_cstr(&format!("{order:?}"))
}
