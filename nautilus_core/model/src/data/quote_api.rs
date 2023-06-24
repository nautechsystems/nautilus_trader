// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::ffi::c_char;

use nautilus_core::{string::str_to_cstr, time::UnixNanos};

use super::quote::QuoteTick;
use crate::{
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
        Price::from_raw(bid_price_raw, bid_price_prec),
        Price::from_raw(ask_price_raw, ask_price_prec),
        Quantity::from_raw(bid_size_raw, bid_size_prec),
        Quantity::from_raw(ask_size_raw, ask_size_prec),
        ts_event,
        ts_init,
    )
}

#[no_mangle]
pub extern "C" fn quote_tick_drop(tick: QuoteTick) {
    drop(tick); // Memory freed here
}

#[no_mangle]
pub extern "C" fn quote_tick_clone(tick: &QuoteTick) -> QuoteTick {
    tick.clone()
}

/// Returns a [`QuoteTick`] as a C string pointer.
#[no_mangle]
pub extern "C" fn quote_tick_to_cstr(tick: &QuoteTick) -> *const c_char {
    str_to_cstr(&tick.to_string())
}
