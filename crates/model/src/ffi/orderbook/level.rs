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

use nautilus_core::ffi::{abort_on_panic, cvec::CVec};

use crate::{
    data::order::BookOrder,
    enums::OrderSide,
    orderbook::{BookLevel, BookPrice},
    types::{Price, quantity::QuantityRaw},
};

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
/// # Safety
///
/// `orders` must uniquely own a valid `Vec<BookOrder>` allocation transferred from Rust.
///
/// Returns an owning pointer to the heap-allocated `BookLevel` which the caller must
/// eventually pass to [`level_drop`].
pub unsafe extern "C" fn level_new(
    order_side: OrderSide,
    price: Price,
    orders: CVec,
) -> *mut BookLevel {
    let orders = unsafe { orders.into_vec::<BookOrder>() };
    let price = BookPrice {
        value: price,
        side: order_side.as_specified(),
    };
    let mut level = BookLevel::new(price);
    level.add_bulk(&orders);
    Box::into_raw(Box::new(level))
}

/// # Safety
///
/// `level` must be a live owning pointer returned by [`level_new`] or [`level_clone`],
/// and must not be used after this call.
///
/// # Panics
///
/// Panics if `level` is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn level_drop(level: *mut BookLevel) {
    abort_on_panic(|| {
        assert!(!level.is_null(), "`level` was NULL");
        // SAFETY: Caller guarantees `level` was allocated by `level_new` or `level_clone`
        drop(unsafe { Box::from_raw(level) }); // Memory freed here
    });
}

/// Returns an owning pointer to a deep copy of `level` which the caller must
/// eventually pass to [`level_drop`].
#[unsafe(no_mangle)]
pub extern "C" fn level_clone(level: &BookLevel) -> *mut BookLevel {
    Box::into_raw(Box::new(level.clone()))
}

#[unsafe(no_mangle)]
pub extern "C" fn level_side(level: &BookLevel) -> OrderSide {
    level.price.side.as_order_side()
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn level_price(level: &BookLevel) -> Price {
    level.price.value
}

#[unsafe(no_mangle)]
pub extern "C" fn level_orders(level: &BookLevel) -> CVec {
    let orders_vec: Vec<BookOrder> = level.orders.values().copied().collect();
    orders_vec.into()
}

#[unsafe(no_mangle)]
pub extern "C" fn level_size(level: &BookLevel) -> f64 {
    level.size()
}

#[unsafe(no_mangle)]
pub extern "C" fn level_size_raw(level: &BookLevel) -> QuantityRaw {
    level.size_raw()
}

#[unsafe(no_mangle)]
pub extern "C" fn level_exposure(level: &BookLevel) -> f64 {
    level.exposure()
}

/// Drops a `CVec` of owning `BookLevel` pointers, freeing each level.
///
/// # Safety
///
/// `v` must uniquely own a valid `Vec<*mut BookLevel>` allocation transferred from Rust,
/// where each element is a live owning pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vec_drop_book_levels(v: CVec) {
    let levels = unsafe { v.into_vec::<*mut BookLevel>() };
    for level in levels {
        if !level.is_null() {
            // SAFETY: Caller guarantees each element is a live owning pointer
            drop(unsafe { Box::from_raw(level) }); // Memory freed here
        }
    }
}

/// Drops a `CVec` of `BookOrder` values.
///
/// # Safety
///
/// `v` must uniquely own a valid `Vec<BookOrder>` allocation transferred from Rust.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vec_drop_book_orders(v: CVec) {
    let orders = unsafe { v.into_vec::<BookOrder>() };
    drop(orders); // Memory freed here
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::data::stubs::stub_book_order;

    #[rstest]
    fn test_empty_typed_drops_return_without_panic() {
        unsafe { vec_drop_book_levels(CVec::empty()) };
        unsafe { vec_drop_book_orders(CVec::empty()) };
    }

    #[rstest]
    fn test_level_new_preserves_valid_behavior() {
        let order = stub_book_order();
        let price = order.price;
        let level_ptr = unsafe { level_new(order.side, price, vec![order].into()) };

        // SAFETY: `level_ptr` was just returned by `level_new`
        let level = unsafe { &*level_ptr };
        assert_eq!(level.price.value, price);
        assert_eq!(level.len(), 1);
        assert_eq!(level.first(), Some(&order));

        unsafe { level_drop(level_ptr) };
    }
}
