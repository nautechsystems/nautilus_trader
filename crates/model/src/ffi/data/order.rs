// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
    ffi::enums::OrderSideOptional,
    types::{Price, Quantity},
};

/// The stable C representation of a [`BookOrder`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BookOrderFfi {
    /// The order side, including the legacy zero value.
    pub side: OrderSideOptional,
    /// The order price.
    pub price: Price,
    /// The order size.
    pub size: Quantity,
    /// The order ID.
    pub order_id: u64,
}

impl From<BookOrderFfi> for BookOrder {
    fn from(value: BookOrderFfi) -> Self {
        Self {
            side: value.side.as_option(),
            price: value.price,
            size: value.size,
            order_id: value.order_id,
        }
    }
}

impl From<BookOrder> for BookOrderFfi {
    fn from(value: BookOrder) -> Self {
        Self {
            side: value.side.into(),
            price: value.price,
            size: value.size,
            order_id: value.order_id,
        }
    }
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn book_order_new(
    order_side: OrderSideOptional,
    price: Price,
    size: Quantity,
    order_id: u64,
) -> BookOrderFfi {
    BookOrder::new(order_side.as_option(), price, size, order_id).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn book_order_eq(lhs: &BookOrderFfi, rhs: &BookOrderFfi) -> u8 {
    u8::from(BookOrder::from(*lhs) == BookOrder::from(*rhs))
}

#[unsafe(no_mangle)]
pub extern "C" fn book_order_hash(order: &BookOrderFfi) -> u64 {
    let mut hasher = DefaultHasher::new();
    BookOrder::from(*order).hash(&mut hasher);
    hasher.finish()
}

#[unsafe(no_mangle)]
pub extern "C" fn book_order_exposure(order: &BookOrderFfi) -> f64 {
    BookOrder::from(*order).exposure()
}

#[unsafe(no_mangle)]
pub extern "C" fn book_order_signed_size(order: &BookOrderFfi) -> f64 {
    BookOrder::from(*order).signed_size()
}

/// Returns a [`BookOrder`] display string as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn book_order_display_to_cstr(order: &BookOrderFfi) -> *const c_char {
    str_to_cstr(&BookOrder::from(*order).to_string())
}

/// Returns a [`BookOrder`] debug string as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn book_order_debug_to_cstr(order: &BookOrderFfi) -> *const c_char {
    str_to_cstr(&format!("{:?}", BookOrder::from(*order)))
}
