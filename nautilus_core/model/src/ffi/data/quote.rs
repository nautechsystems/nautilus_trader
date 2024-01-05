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

use std::{
    collections::hash_map::DefaultHasher,
    ffi::c_char,
    hash::{Hash, Hasher},
};

use nautilus_core::{ffi::string::str_to_cstr, time::UnixNanos};

use crate::{
    data::quote::QuoteTick,
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

#[no_mangle]
pub extern "C" fn quote_tick_new(
    instrument_id: InstrumentId,
    bid_price_raw: i64,
    ask_price_raw: i64,
    bid_price_prec: u8,
    ask_price_prec: u8,
    bid_size_raw: u64,
    ask_size_raw: u64,
    bid_size_prec: u8,
    ask_size_prec: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from_raw(bid_price_raw, bid_price_prec).unwrap(),
        Price::from_raw(ask_price_raw, ask_price_prec).unwrap(),
        Quantity::from_raw(bid_size_raw, bid_size_prec).unwrap(),
        Quantity::from_raw(ask_size_raw, ask_size_prec).unwrap(),
        ts_event,
        ts_init,
    )
    .unwrap()
}

#[no_mangle]
pub extern "C" fn quote_tick_eq(lhs: &QuoteTick, rhs: &QuoteTick) -> u8 {
    assert_eq!(lhs.ask_price, rhs.ask_price);
    assert_eq!(lhs.ask_size, rhs.ask_size);
    assert_eq!(lhs.bid_price, rhs.bid_price);
    assert_eq!(lhs.bid_size, rhs.bid_size);
    assert_eq!(lhs.ts_event, rhs.ts_event);
    assert_eq!(lhs.ts_init, rhs.ts_init);
    assert_eq!(
        lhs.instrument_id.symbol.value,
        rhs.instrument_id.symbol.value
    );
    assert_eq!(lhs.instrument_id.venue.value, rhs.instrument_id.venue.value);
    u8::from(lhs == rhs)
}

#[no_mangle]
pub extern "C" fn quote_tick_hash(delta: &QuoteTick) -> u64 {
    let mut hasher = DefaultHasher::new();
    delta.hash(&mut hasher);
    hasher.finish()
}

/// Returns a [`QuoteTick`] as a C string pointer.
#[no_mangle]
pub extern "C" fn quote_tick_to_cstr(tick: &QuoteTick) -> *const c_char {
    str_to_cstr(&tick.to_string())
}
