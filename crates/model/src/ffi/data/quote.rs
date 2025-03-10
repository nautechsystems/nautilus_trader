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
};

use nautilus_core::{UnixNanos, ffi::string::str_to_cstr};

use crate::{
    data::QuoteTick,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn quote_tick_new(
    instrument_id: InstrumentId,
    bid_price: Price,
    ask_price: Price,
    bid_size: Quantity,
    ask_size: Quantity,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn quote_tick_eq(lhs: &QuoteTick, rhs: &QuoteTick) -> u8 {
    assert_eq!(lhs.ask_price, rhs.ask_price);
    assert_eq!(lhs.ask_size, rhs.ask_size);
    assert_eq!(lhs.bid_price, rhs.bid_price);
    assert_eq!(lhs.bid_size, rhs.bid_size);
    assert_eq!(lhs.ts_event, rhs.ts_event);
    assert_eq!(lhs.ts_init, rhs.ts_init);
    assert_eq!(lhs.instrument_id.symbol, rhs.instrument_id.symbol);
    assert_eq!(lhs.instrument_id.venue, rhs.instrument_id.venue);
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
pub extern "C" fn quote_tick_hash(delta: &QuoteTick) -> u64 {
    let mut hasher = DefaultHasher::new();
    delta.hash(&mut hasher);
    hasher.finish()
}

/// Returns a [`QuoteTick`] as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn quote_tick_to_cstr(quote: &QuoteTick) -> *const c_char {
    str_to_cstr(&quote.to_string())
}
