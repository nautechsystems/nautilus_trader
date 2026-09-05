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

use nautilus_core::{
    UnixNanos,
    ffi::{abort_on_panic, cvec::CVec},
};

use crate::{
    data::OrderBookDeltas, ffi::data::delta::OrderBookDeltaFfi, identifiers::InstrumentId,
};

/// Creates a new `OrderBookDeltas` instance from a `CVec` of `OrderBookDeltaFfi`.
///
/// The data is cloned into Rust-managed memory and remains owned by the caller.
///
/// Returns an owning pointer to the heap-allocated `OrderBookDeltas` which the caller
/// must eventually pass to [`orderbook_deltas_drop`].
///
/// # Safety
///
/// `deltas` must describe initialized `OrderBookDeltaFfi` values that remain valid and immutable for
/// the duration of this call. The caller remains responsible for deallocating its buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orderbook_deltas_new(
    instrument_id: InstrumentId,
    deltas: &CVec,
) -> *mut OrderBookDeltas {
    let cloned_deltas = unsafe { deltas.as_slice::<OrderBookDeltaFfi>() }
        .iter()
        .copied()
        .map(Into::into)
        .collect();
    Box::into_raw(Box::new(OrderBookDeltas::new(instrument_id, cloned_deltas)))
}

/// # Safety
///
/// `deltas` must be a live owning pointer returned by [`orderbook_deltas_new`],
/// [`orderbook_deltas_clone`], or
/// [`orderbook_to_snapshot_deltas`](crate::ffi::orderbook::book::orderbook_to_snapshot_deltas), and
/// must not be used after this call.
///
/// # Panics
///
/// Panics if `deltas` is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orderbook_deltas_drop(deltas: *mut OrderBookDeltas) {
    abort_on_panic(|| {
        assert!(!deltas.is_null(), "`deltas` was NULL");
        // SAFETY: Caller guarantees exclusive ownership of a live `Box` allocation
        drop(unsafe { Box::from_raw(deltas) }); // Memory freed here
    });
}

/// Returns an owning pointer to a deep copy of `deltas` which the caller must
/// eventually pass to [`orderbook_deltas_drop`].
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_clone(deltas: &OrderBookDeltas) -> *mut OrderBookDeltas {
    Box::into_raw(Box::new(deltas.clone()))
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_instrument_id(deltas: &OrderBookDeltas) -> InstrumentId {
    deltas.instrument_id
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_vec_deltas(deltas: &OrderBookDeltas) -> CVec {
    deltas
        .deltas
        .iter()
        .copied()
        .map(OrderBookDeltaFfi::from)
        .collect::<Vec<_>>()
        .into()
}

/// Returns `1` if `deltas` has the `F_SNAPSHOT` record flag, `0` otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_is_snapshot(deltas: &OrderBookDeltas) -> u8 {
    u8::from(deltas.is_snapshot())
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_flags(deltas: &OrderBookDeltas) -> u8 {
    deltas.flags
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_sequence(deltas: &OrderBookDeltas) -> u64 {
    deltas.sequence
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_ts_event(deltas: &OrderBookDeltas) -> UnixNanos {
    deltas.ts_event
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_deltas_ts_init(deltas: &OrderBookDeltas) -> UnixNanos {
    deltas.ts_init
}

/// Drops a `CVec` of `OrderBookDeltaFfi` values.
///
/// # Safety
///
/// `v` must uniquely own a valid `Vec<OrderBookDeltaFfi>` allocation transferred from Rust.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orderbook_deltas_vec_drop(v: CVec) {
    let deltas = unsafe { v.into_vec::<OrderBookDeltaFfi>() };
    drop(deltas); // Memory freed here
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        data::stubs::stub_delta,
        enums::{BookAction, RecordFlag},
    };

    #[rstest]
    fn test_empty_delta_drop_returns_without_panic() {
        unsafe { orderbook_deltas_vec_drop(CVec::empty()) };
    }

    #[rstest]
    fn test_orderbook_deltas_new_clones_borrowed_buffer() {
        let delta = stub_delta();
        let mut caller_owned = vec![OrderBookDeltaFfi::from(delta)];
        let cvec = CVec {
            ptr: caller_owned.as_mut_ptr().cast(),
            len: caller_owned.len(),
            cap: caller_owned.capacity(),
        };

        let deltas_ptr = unsafe { orderbook_deltas_new(delta.instrument_id, &cvec) };

        // SAFETY: `deltas_ptr` was just returned by `orderbook_deltas_new`
        let deltas = unsafe { &*deltas_ptr };
        assert_eq!(deltas.deltas, vec![delta]);
        caller_owned[0].sequence += 1;
        assert_ne!(deltas.deltas[0].sequence, caller_owned[0].sequence);

        unsafe { orderbook_deltas_drop(deltas_ptr) };
    }

    #[rstest]
    #[case::snapshot(
        BookAction::Add,
        RecordFlag::F_SNAPSHOT as u8,
        1
    )]
    #[case::combined_flags(
        BookAction::Add,
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
        1
    )]
    #[case::not_snapshot(BookAction::Add, RecordFlag::F_MBP as u8, 0)]
    #[case::clear_without_snapshot(BookAction::Clear, 0, 0)]
    fn test_orderbook_deltas_is_snapshot(
        #[case] action: BookAction,
        #[case] flags: u8,
        #[case] expected: u8,
    ) {
        let mut delta = stub_delta();
        delta.action = action;
        delta.flags = flags;
        let deltas = OrderBookDeltas::new(delta.instrument_id, vec![delta]);

        assert_eq!(orderbook_deltas_is_snapshot(&deltas), expected);
    }
}
