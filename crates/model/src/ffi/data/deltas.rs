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

use nautilus_core::{UnixNanos, ffi::cvec::CVec};

use crate::{
    data::{OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API},
    enums::BookAction,
    identifiers::InstrumentId,
};

/// Creates a new [`OrderBookDeltas_API`] instance from a `CVec` of `OrderBookDelta`.
///
/// The data is cloned into Rust-managed memory and remains owned by the caller.
///
/// # Safety
///
/// `deltas` must describe initialized `OrderBookDelta` values that remain valid and immutable for
/// the duration of this call. The caller remains responsible for deallocating its buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orderbook_deltas_new(
    instrument_id: InstrumentId,
    deltas: &CVec,
) -> OrderBookDeltas_API {
    let cloned_deltas = unsafe { deltas.as_slice::<OrderBookDelta>() }.to_vec();
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

/// Returns `1` if the first delta is a `Clear` action (snapshot), `0` otherwise.
///
/// Returns `0` for empty delta vectors to avoid panicking on malformed FFI input.
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_is_snapshot(deltas: &OrderBookDeltas_API) -> u8 {
    deltas
        .deltas
        .first()
        .map_or(0, |first| u8::from(first.action == BookAction::Clear))
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

/// Drops a `CVec` of `OrderBookDelta` values.
///
/// # Safety
///
/// `v` must uniquely own a valid `Vec<OrderBookDelta>` allocation transferred from Rust.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orderbook_deltas_vec_drop(v: CVec) {
    let deltas = unsafe { v.into_vec::<OrderBookDelta>() };
    drop(deltas); // Memory freed here
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::data::stubs::stub_delta;

    #[rstest]
    fn test_empty_delta_drop_returns_without_panic() {
        unsafe { orderbook_deltas_vec_drop(CVec::empty()) };
    }

    #[rstest]
    fn test_orderbook_deltas_new_clones_borrowed_buffer() {
        let delta = stub_delta();
        let mut caller_owned = vec![delta];
        let cvec = CVec {
            ptr: caller_owned.as_mut_ptr().cast(),
            len: caller_owned.len(),
            cap: caller_owned.capacity(),
        };

        let deltas = unsafe { orderbook_deltas_new(delta.instrument_id, &cvec) };

        assert_eq!(deltas.deltas, caller_owned);
        caller_owned[0].sequence += 1;
        assert_ne!(deltas.deltas, caller_owned);
    }
}
