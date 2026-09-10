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
    hash::{Hash, Hasher},
};

use nautilus_core::{UnixNanos, ffi::abort_on_panic};

use crate::{
    data::depth::{DEPTH10_LEN, OrderBookDepth10},
    ffi::data::order::BookOrderFfi,
    identifiers::InstrumentId,
};

/// The stable C representation of an [`OrderBookDepth10`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OrderBookDepth10Ffi {
    pub instrument_id: InstrumentId,
    pub bids: [BookOrderFfi; DEPTH10_LEN],
    pub asks: [BookOrderFfi; DEPTH10_LEN],
    pub bid_counts: [u32; DEPTH10_LEN],
    pub ask_counts: [u32; DEPTH10_LEN],
    pub flags: u8,
    pub sequence: u64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl From<OrderBookDepth10Ffi> for OrderBookDepth10 {
    fn from(value: OrderBookDepth10Ffi) -> Self {
        Self {
            instrument_id: value.instrument_id,
            bids: value.bids.map(Into::into).into(),
            asks: value.asks.map(Into::into).into(),
            bid_counts: value.bid_counts.into(),
            ask_counts: value.ask_counts.into(),
            flags: value.flags,
            sequence: value.sequence,
            ts_event: value.ts_event,
            ts_init: value.ts_init,
        }
    }
}

impl TryFrom<OrderBookDepth10> for OrderBookDepth10Ffi {
    type Error = anyhow::Error;

    fn try_from(value: OrderBookDepth10) -> Result<Self, Self::Error> {
        anyhow::ensure!(
            [
                value.bids.len(),
                value.asks.len(),
                value.bid_counts.len(),
                value.ask_counts.len()
            ]
            .into_iter()
            .all(|len| len == DEPTH10_LEN),
            "The legacy depth FFI requires exactly ten levels per side"
        );
        Ok(Self {
            instrument_id: value.instrument_id,
            bids: std::array::from_fn(|i| value.bids[i].into()),
            asks: std::array::from_fn(|i| value.asks[i].into()),
            bid_counts: std::array::from_fn(|i| value.bid_counts[i]),
            ask_counts: std::array::from_fn(|i| value.ask_counts[i]),
            flags: value.flags,
            sequence: value.sequence,
            ts_event: value.ts_event,
            ts_init: value.ts_init,
        })
    }
}

/// # Safety
///
/// This function assumes:
/// - `bids` and `asks` are valid pointers to arrays of `BookOrderFfi` of length 10.
/// - `bid_counts` and `ask_counts` are valid pointers to arrays of `u32` of length 10.
///
/// # Panics
///
/// Panics if any input pointer is null or if slice conversion for bids or asks fails.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub unsafe extern "C" fn orderbook_depth10_new(
    instrument_id: InstrumentId,
    bids_ptr: *const BookOrderFfi,
    asks_ptr: *const BookOrderFfi,
    bid_counts_ptr: *const u32,
    ask_counts_ptr: *const u32,
    flags: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDepth10Ffi {
    abort_on_panic(|| {
        // SAFETY: Null checks run before slice construction. The caller still
        // guarantees each pointer refers to `DEPTH10_LEN` initialized elements.
        assert!(!bids_ptr.is_null());
        assert!(!asks_ptr.is_null());
        assert!(!bid_counts_ptr.is_null());
        assert!(!ask_counts_ptr.is_null());

        let bids_slice = unsafe { std::slice::from_raw_parts(bids_ptr, DEPTH10_LEN) };
        let asks_slice = unsafe { std::slice::from_raw_parts(asks_ptr, DEPTH10_LEN) };
        let bids: [BookOrderFfi; DEPTH10_LEN] = bids_slice.try_into().expect("Slice length != 10");
        let asks: [BookOrderFfi; DEPTH10_LEN] = asks_slice.try_into().expect("Slice length != 10");

        let bid_counts_slice = unsafe { std::slice::from_raw_parts(bid_counts_ptr, DEPTH10_LEN) };
        let ask_counts_slice = unsafe { std::slice::from_raw_parts(ask_counts_ptr, DEPTH10_LEN) };
        let bid_counts: [u32; DEPTH10_LEN] =
            bid_counts_slice.try_into().expect("Slice length != 10");
        let ask_counts: [u32; DEPTH10_LEN] =
            ask_counts_slice.try_into().expect("Slice length != 10");

        OrderBookDepth10Ffi {
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        }
    })
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_depth10_clone(depth: &OrderBookDepth10Ffi) -> OrderBookDepth10Ffi {
    *depth
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_eq(lhs: &OrderBookDepth10Ffi, rhs: &OrderBookDepth10Ffi) -> u8 {
    u8::from(OrderBookDepth10::from(*lhs) == OrderBookDepth10::from(*rhs))
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_hash(delta: &OrderBookDepth10Ffi) -> u64 {
    let mut hasher = DefaultHasher::new();
    OrderBookDepth10::from(*delta).hash(&mut hasher);
    hasher.finish()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_bids_array(depth: &OrderBookDepth10Ffi) -> *const BookOrderFfi {
    depth.bids.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_asks_array(depth: &OrderBookDepth10Ffi) -> *const BookOrderFfi {
    depth.asks.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_bid_counts_array(depth: &OrderBookDepth10Ffi) -> *const u32 {
    depth.bid_counts.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_ask_counts_array(depth: &OrderBookDepth10Ffi) -> *const u32 {
    depth.ask_counts.as_ptr()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::data::{BookOrder, stubs::stub_depth10};

    #[rstest]
    #[case::populated(10, 10)]
    #[case::padded(3, 6)]
    #[case::empty(0, 0)]
    fn legacy_constructor_preserves_slots_and_metadata(
        #[case] bid_levels: usize,
        #[case] ask_levels: usize,
    ) {
        let depth = stub_depth10();
        let bids: [BookOrderFfi; DEPTH10_LEN] = std::array::from_fn(|i| {
            if i < bid_levels {
                depth.bids[i].into()
            } else {
                BookOrder::default().into()
            }
        });
        let asks: [BookOrderFfi; DEPTH10_LEN] = std::array::from_fn(|i| {
            if i < ask_levels {
                depth.asks[i].into()
            } else {
                BookOrder::default().into()
            }
        });
        let bid_counts = std::array::from_fn::<_, DEPTH10_LEN, _>(|i| i as u32 + 11);
        let ask_counts = std::array::from_fn::<_, DEPTH10_LEN, _>(|i| i as u32 + 31);

        // SAFETY: All pointers refer to live arrays with exactly DEPTH10_LEN initialized slots
        let actual = unsafe {
            orderbook_depth10_new(
                depth.instrument_id,
                bids.as_ptr(),
                asks.as_ptr(),
                bid_counts.as_ptr(),
                ask_counts.as_ptr(),
                17,
                23,
                UnixNanos::from(41),
                UnixNanos::from(43),
            )
        };

        assert_eq!(actual.instrument_id, depth.instrument_id);
        for (actual, expected) in actual
            .bids
            .into_iter()
            .chain(actual.asks)
            .zip(bids.into_iter().chain(asks))
        {
            assert_eq!(
                (
                    actual.side,
                    actual.price.raw,
                    actual.price.precision,
                    actual.size.raw,
                    actual.size.precision,
                    actual.order_id
                ),
                (
                    expected.side,
                    expected.price.raw,
                    expected.price.precision,
                    expected.size.raw,
                    expected.size.precision,
                    expected.order_id
                ),
            );
        }
        assert_eq!(actual.bid_counts, bid_counts);
        assert_eq!(actual.ask_counts, ask_counts);
        assert_eq!(
            (
                actual.flags,
                actual.sequence,
                actual.ts_event,
                actual.ts_init
            ),
            (17, 23, UnixNanos::from(41), UnixNanos::from(43))
        );
    }

    #[rstest]
    fn legacy_depth_conversion_preserves_all_fields() {
        let mut depth = stub_depth10();
        depth.flags = 31;
        depth.sequence = 23;
        depth.ts_event = UnixNanos::from(41);
        depth.ts_init = UnixNanos::from(43);
        depth.bid_counts = (1..=10).collect();
        depth.ask_counts = (11..=20).collect();
        let ffi = OrderBookDepth10Ffi::try_from(depth.clone()).unwrap();
        assert_eq!(ffi.instrument_id, depth.instrument_id);
        assert_eq!(
            ffi.bids.map(BookOrder::from).as_slice(),
            depth.bids.as_slice()
        );
        assert_eq!(
            ffi.asks.map(BookOrder::from).as_slice(),
            depth.asks.as_slice()
        );
        assert_eq!(ffi.bid_counts.as_slice(), depth.bid_counts.as_slice());
        assert_eq!(ffi.ask_counts.as_slice(), depth.ask_counts.as_slice());
        assert_eq!(
            (ffi.flags, ffi.sequence, ffi.ts_event, ffi.ts_init),
            (31, 23, UnixNanos::from(41), UnixNanos::from(43))
        );
        assert_eq!(OrderBookDepth10::from(ffi), depth);
    }

    #[rstest]
    #[case(0)]
    #[case(9)]
    #[case(11)]
    fn legacy_depth_conversion_rejects_other_depths(#[case] levels: usize) {
        let mut depth = stub_depth10();
        depth.bids.resize(levels, depth.bids[0]);
        depth.bid_counts.resize(levels, 1);
        let error = OrderBookDepth10Ffi::try_from(depth).unwrap_err();
        assert_eq!(
            error.to_string(),
            "The legacy depth FFI requires exactly ten levels per side"
        );
    }
}
