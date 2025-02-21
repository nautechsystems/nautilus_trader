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

use nautilus_core::{UnixNanos, ffi::cvec::CVec};

use crate::{
    data::{OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API},
    enums::BookAction,
    identifiers::InstrumentId,
};

/// Creates a new `OrderBookDeltas` instance from a `CVec` of `OrderBookDelta`.
///
/// # Safety
/// - The `deltas` must be a valid pointer to a `CVec` containing `OrderBookDelta` objects
/// - This function clones the data pointed to by `deltas` into Rust-managed memory, then forgets the original `Vec` to prevent Rust from auto-deallocating it
/// - The caller is responsible for managing the memory of `deltas` (including its deallocation) to avoid memory leaks
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_new(
    instrument_id: InstrumentId,
    deltas: &CVec,
) -> OrderBookDeltas_API {
    let CVec { ptr, len, cap } = *deltas;
    let deltas: Vec<OrderBookDelta> =
        unsafe { Vec::from_raw_parts(ptr.cast::<OrderBookDelta>(), len, cap) };
    let cloned_deltas = deltas.clone();
    std::mem::forget(deltas); // Prevents Rust from dropping `deltas`
    OrderBookDeltas_API::new(OrderBookDeltas::new(instrument_id, cloned_deltas))
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_drop(deltas: OrderBookDeltas_API) {
    drop(deltas); // Memory freed here
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_clone(deltas: &OrderBookDeltas_API) -> OrderBookDeltas_API {
    deltas.clone()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_instrument_id(deltas: &OrderBookDeltas_API) -> InstrumentId {
    deltas.instrument_id
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_vec_deltas(deltas: &OrderBookDeltas_API) -> CVec {
    deltas.deltas.clone().into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_is_snapshot(deltas: &OrderBookDeltas_API) -> u8 {
    u8::from(deltas.deltas[0].action == BookAction::Clear)
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_flags(deltas: &OrderBookDeltas_API) -> u8 {
    deltas.flags
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_sequence(deltas: &OrderBookDeltas_API) -> u64 {
    deltas.sequence
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_ts_event(deltas: &OrderBookDeltas_API) -> UnixNanos {
    deltas.ts_event
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_ts_init(deltas: &OrderBookDeltas_API) -> UnixNanos {
    deltas.ts_init
}

#[allow(clippy::drop_non_drop)]
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_vec_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let deltas: Vec<OrderBookDelta> =
        unsafe { Vec::from_raw_parts(ptr.cast::<OrderBookDelta>(), len, cap) };
    drop(deltas); // Memory freed here
}
