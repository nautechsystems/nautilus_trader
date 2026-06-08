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

//! Boundary-owned handles for order book payloads.
//!
//! [`OrderBook`] and [`OrderBookDeltas`] own Rust collection state and
//! cannot be `#[repr(C)]`, so the host wraps each value in a `#[repr(C)]`
//! handle that owns the boxed value and passes a borrowed pointer to the
//! plug-in. The plug-in's thunk dereferences the handle once and hands an
//! `&OrderBook` or `&OrderBookDeltas` to the trait method. The deltas
//! handle mirrors the ownership contract that
//! [`OrderBookDeltas_API`](nautilus_model::data::OrderBookDeltas_API)
//! uses for the Cython FFI surface.

#![allow(unsafe_code)]

use std::ops::Deref;

use nautilus_model::{data::OrderBookDeltas, orderbook::OrderBook};

/// Boundary-owned wrapper that lets [`OrderBook`] cross the cdylib FFI
/// boundary by reference.
///
/// The host constructs an instance from a cloned book, hands a
/// `*const OrderBookHandle` to the plug-in for the duration of the
/// callback, and drops the handle when the call returns. The plug-in
/// only borrows the handle and never owns it.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct OrderBookHandle(Box<OrderBook>);

impl OrderBookHandle {
    /// Wraps `book` in a boundary-owned handle.
    #[must_use]
    pub fn new(book: OrderBook) -> Self {
        Self(Box::new(book))
    }

    /// Returns a reference to the wrapped book.
    #[must_use]
    pub fn book(&self) -> &OrderBook {
        &self.0
    }

    /// Consumes the wrapper and returns the inner book.
    #[must_use]
    pub fn into_inner(self) -> OrderBook {
        *self.0
    }
}

impl Deref for OrderBookHandle {
    type Target = OrderBook;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Boundary-owned wrapper that lets [`OrderBookDeltas`] cross the cdylib
/// FFI boundary by reference.
///
/// The host constructs an instance, hands a
/// `*const OrderBookDeltasHandle` to the plug-in for the duration of the
/// callback, and drops the handle when the call returns. The plug-in
/// only borrows the handle and never owns it.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct OrderBookDeltasHandle(Box<OrderBookDeltas>);

impl OrderBookDeltasHandle {
    /// Wraps `deltas` in a boundary-owned handle.
    #[must_use]
    pub fn new(deltas: OrderBookDeltas) -> Self {
        Self(Box::new(deltas))
    }

    /// Returns a reference to the wrapped deltas.
    #[must_use]
    pub fn deltas(&self) -> &OrderBookDeltas {
        &self.0
    }

    /// Consumes the wrapper and returns the inner deltas.
    #[must_use]
    pub fn into_inner(self) -> OrderBookDeltas {
        *self.0
    }
}

impl Deref for OrderBookDeltasHandle {
    type Target = OrderBookDeltas;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
