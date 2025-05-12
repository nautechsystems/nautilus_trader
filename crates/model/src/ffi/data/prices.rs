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

use nautilus_core::ffi::string::str_to_cstr;

use crate::{data::MarkPriceUpdate, identifiers::InstrumentId, types::Price};

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn mark_price_update_new(
    instrument_id: InstrumentId,
    value: Price,
    ts_event: u64,
    ts_init: u64,
) -> MarkPriceUpdate {
    MarkPriceUpdate::new(instrument_id, value, ts_event.into(), ts_init.into())
}

#[unsafe(no_mangle)]
pub extern "C" fn mark_price_update_eq(lhs: &MarkPriceUpdate, rhs: &MarkPriceUpdate) -> u8 {
    u8::from(lhs == rhs)
}

#[unsafe(no_mangle)]
pub extern "C" fn mark_price_update_hash(value: &MarkPriceUpdate) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[unsafe(no_mangle)]
pub extern "C" fn mark_price_update_to_cstr(value: &MarkPriceUpdate) -> *const c_char {
    str_to_cstr(&value.to_string())
}
