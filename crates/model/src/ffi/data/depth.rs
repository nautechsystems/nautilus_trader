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
    hash::{Hash, Hasher},
};

use nautilus_core::UnixNanos;

use crate::{
    data::{
        depth::{DEPTH10_LEN, OrderBookDepth10},
        order::BookOrder,
    },
    identifiers::InstrumentId,
};

/// # Safety
///
/// - Assumes `bids` and `asks` are valid pointers to arrays of `BookOrder` of length 10.
/// - Assumes `bid_counts` and `ask_counts` are valid pointers to arrays of `u32` of length 10.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub unsafe extern "C" fn orderbook_depth10_new(
    instrument_id: InstrumentId,
    bids_ptr: *const BookOrder,
    asks_ptr: *const BookOrder,
    bid_counts_ptr: *const u32,
    ask_counts_ptr: *const u32,
    flags: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDepth10 {
    // Safety: Ensure that `bids_ptr` and `asks_ptr` are valid pointers.
    // The caller must guarantee that they point to arrays of `BookOrder` of at least `DEPTH10_LEN` length.
    assert!(!bids_ptr.is_null());
    assert!(!asks_ptr.is_null());
    assert!(!bid_counts_ptr.is_null());
    assert!(!ask_counts_ptr.is_null());

    let bids_slice = unsafe { std::slice::from_raw_parts(bids_ptr, DEPTH10_LEN) };
    let asks_slice = unsafe { std::slice::from_raw_parts(asks_ptr, DEPTH10_LEN) };
    let bids: [BookOrder; DEPTH10_LEN] = bids_slice.try_into().expect("Slice length != 10");
    let asks: [BookOrder; DEPTH10_LEN] = asks_slice.try_into().expect("Slice length != 10");

    let bid_counts_slice = unsafe { std::slice::from_raw_parts(bid_counts_ptr, DEPTH10_LEN) };
    let ask_counts_slice = unsafe { std::slice::from_raw_parts(ask_counts_ptr, DEPTH10_LEN) };
    let bid_counts: [u32; DEPTH10_LEN] = bid_counts_slice.try_into().expect("Slice length != 10");
    let ask_counts: [u32; DEPTH10_LEN] = ask_counts_slice.try_into().expect("Slice length != 10");

    OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        bid_counts,
        ask_counts,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_depth10_clone(depth: &OrderBookDepth10) -> OrderBookDepth10 {
    *depth
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_eq(lhs: &OrderBookDepth10, rhs: &OrderBookDepth10) -> u8 {
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_hash(delta: &OrderBookDepth10) -> u64 {
    let mut hasher = DefaultHasher::new();
    delta.hash(&mut hasher);
    hasher.finish()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_bids_array(depth: &OrderBookDepth10) -> *const BookOrder {
    depth.bids.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_asks_array(depth: &OrderBookDepth10) -> *const BookOrder {
    depth.asks.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_bid_counts_array(depth: &OrderBookDepth10) -> *const u32 {
    depth.bid_counts.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_depth10_ask_counts_array(depth: &OrderBookDepth10) -> *const u32 {
    depth.ask_counts.as_ptr()
}
