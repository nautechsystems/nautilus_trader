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

use std::ops::{Deref, DerefMut};

use nautilus_core::ffi::cvec::CVec;

use crate::{
    data::order::BookOrder,
    enums::OrderSide,
    orderbook::{ladder::BookPrice, level::Level},
    types::price::Price,
};

/// Provides a C compatible Foreign Function Interface (FFI) for an underlying order book[`Level`].
///
/// This struct wraps `Level` in a way that makes it compatible with C function
/// calls, enabling interaction with `Level` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `Level_API` to be
/// dereferenced to `Level`, providing access to `Level`'s methods without
/// having to manually acce wss the underlying `Level` instance.
#[repr(C)]
#[derive(Clone)]
#[allow(non_camel_case_types)]
pub struct Level_API(Box<Level>);

impl Level_API {
    #[must_use]
    pub fn new(level: Level) -> Self {
        Self(Box::new(level))
    }
}

impl Deref for Level_API {
    type Target = Level;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Level_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[no_mangle]
pub extern "C" fn level_new(order_side: OrderSide, price: Price, orders: CVec) -> Level_API {
    let CVec { ptr, len, cap } = orders;
    let orders: Vec<BookOrder> = unsafe { Vec::from_raw_parts(ptr.cast::<BookOrder>(), len, cap) };
    let price = BookPrice {
        value: price,
        side: order_side,
    };
    let mut level = Level::new(price);
    level.add_bulk(orders);
    Level_API::new(level)
}

#[no_mangle]
pub extern "C" fn level_drop(level: Level_API) {
    drop(level); // Memory freed here
}

#[no_mangle]
pub extern "C" fn level_clone(level: &Level_API) -> Level_API {
    level.clone()
}

#[no_mangle]
pub extern "C" fn level_price(level: &Level_API) -> Price {
    level.price.value
}

#[no_mangle]
pub extern "C" fn level_orders(level: &Level_API) -> CVec {
    let orders_vec: Vec<BookOrder> = level.orders.values().copied().collect();
    orders_vec.into()
}

#[no_mangle]
pub extern "C" fn level_size(level: &Level_API) -> f64 {
    level.size()
}

#[no_mangle]
pub extern "C" fn level_exposure(level: &Level_API) -> f64 {
    level.exposure()
}

#[allow(clippy::drop_non_drop)]
#[no_mangle]
pub extern "C" fn vec_levels_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let data: Vec<Level_API> = unsafe { Vec::from_raw_parts(ptr.cast::<Level_API>(), len, cap) };
    drop(data); // Memory freed here
}

#[allow(clippy::drop_non_drop)]
#[no_mangle]
pub extern "C" fn vec_orders_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let orders: Vec<BookOrder> = unsafe { Vec::from_raw_parts(ptr.cast::<BookOrder>(), len, cap) };
    drop(orders); // Memory freed here
}
