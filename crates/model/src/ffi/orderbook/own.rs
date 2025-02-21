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
    ops::{Deref, DerefMut},
};

use nautilus_core::ffi::string::str_to_cstr;

use crate::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId},
    orderbook::{OwnBookOrder, own::OwnOrderBook},
    types::{Price, Quantity},
};

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn own_book_order_new(
    client_order_id: ClientOrderId,
    side: OrderSide,
    price: Price,
    size: Quantity,
    order_type: OrderType,
    time_in_force: TimeInForce,
    status: OrderStatus,
    ts_last: u64,
    ts_init: u64,
) -> OwnBookOrder {
    OwnBookOrder::new(
        client_order_id,
        side.as_specified(),
        price,
        size,
        order_type,
        time_in_force,
        status,
        ts_last.into(),
        ts_init.into(),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn own_book_order_eq(lhs: &OwnBookOrder, rhs: &OwnBookOrder) -> u8 {
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
pub extern "C" fn own_book_order_hash(order: &OwnBookOrder) -> u64 {
    let mut hasher = DefaultHasher::new();
    order.hash(&mut hasher);
    hasher.finish()
}

/// Returns a [`OwnBookOrder`] display string as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn own_book_order_display_to_cstr(order: &OwnBookOrder) -> *const c_char {
    str_to_cstr(&format!("{order}"))
}

/// Returns a [`OwnBookOrder`] debug string as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn own_book_order_debug_to_cstr(order: &OwnBookOrder) -> *const c_char {
    str_to_cstr(&format!("{order:?}"))
}

/// C compatible Foreign Function Interface (FFI) for an underlying `OwnOrderBook`.
///
/// This struct wraps `OwnOrderBook` in a way that makes it compatible with C function
/// calls, enabling interaction with `OrderBook` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `OwnOrderBook_API` to be
/// dereferenced to `OwnOrderBook`, providing access to `OwnOrderBook`'s methods without
/// having to manually access the underlying `OrderBook` instance.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct OwnOrderBook_API(Box<OwnOrderBook>);

impl Deref for OwnOrderBook_API {
    type Target = OwnOrderBook;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for OwnOrderBook_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn own_orderbook_new(instrument_id: InstrumentId) -> OwnOrderBook_API {
    OwnOrderBook_API(Box::new(OwnOrderBook::new(instrument_id)))
}

#[unsafe(no_mangle)]
pub extern "C" fn own_orderbook_drop(book: OwnOrderBook_API) {
    drop(book); // Memory freed here
}

#[unsafe(no_mangle)]
pub extern "C" fn own_orderbook_reset(book: &mut OwnOrderBook_API) {
    book.reset();
}

#[unsafe(no_mangle)]
pub extern "C" fn own_orderbook_instrument_id(book: &OwnOrderBook_API) -> InstrumentId {
    book.instrument_id
}

#[unsafe(no_mangle)]
pub extern "C" fn own_orderbook_ts_last(book: &OwnOrderBook_API) -> u64 {
    book.ts_last.into()
}

#[unsafe(no_mangle)]
pub extern "C" fn own_orderbook_count(book: &OwnOrderBook_API) -> u64 {
    book.count
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn own_orderbook_add(book: &mut OwnOrderBook_API, order: OwnBookOrder) {
    book.add(order);
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn own_orderbook_update(book: &mut OwnOrderBook_API, order: OwnBookOrder) {
    book.update(order);
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn own_orderbook_delete(book: &mut OwnOrderBook_API, order: OwnBookOrder) {
    book.delete(order);
}

#[unsafe(no_mangle)]
pub extern "C" fn own_orderbook_clear(book: &mut OwnOrderBook_API) {
    book.clear();
}
