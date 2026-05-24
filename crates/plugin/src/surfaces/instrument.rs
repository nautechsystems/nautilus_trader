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

//! Boundary-owned handle for [`InstrumentAny`].
//!
//! [`InstrumentAny`] is a Rust enum whose variant payloads (e.g.
//! `FuturesContract`, `CurrencyPair`) own heap-allocated fields and
//! cannot be `#[repr(C)]`, so the host wraps it in a `#[repr(C)]` handle
//! that owns the boxed value and passes a borrowed pointer to the
//! plug-in. The plug-in's thunk dereferences the handle once and hands
//! an `&InstrumentAny` to the trait method. Mirrors the ownership
//! contract that
//! [`OrderBookDeltasHandle`](crate::surfaces::book::OrderBookDeltasHandle)
//! uses for [`OrderBookDeltas`](nautilus_model::data::OrderBookDeltas).

#![allow(unsafe_code)]

use std::ops::Deref;

use nautilus_model::instruments::InstrumentAny;

/// Boundary-owned wrapper that lets [`InstrumentAny`] cross the cdylib
/// FFI boundary by reference.
///
/// The host constructs an instance, hands a
/// `*const InstrumentAnyHandle` to the plug-in for the duration of the
/// callback, and drops the handle when the call returns. The plug-in
/// only borrows the handle and never owns it.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct InstrumentAnyHandle(Box<InstrumentAny>);

impl InstrumentAnyHandle {
    /// Wraps `instrument` in a boundary-owned handle.
    #[must_use]
    pub fn new(instrument: InstrumentAny) -> Self {
        Self(Box::new(instrument))
    }

    /// Returns a reference to the wrapped instrument.
    #[must_use]
    pub fn instrument(&self) -> &InstrumentAny {
        &self.0
    }

    /// Consumes the wrapper and returns the inner instrument.
    #[must_use]
    pub fn into_inner(self) -> InstrumentAny {
        *self.0
    }
}

impl Deref for InstrumentAnyHandle {
    type Target = InstrumentAny;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
