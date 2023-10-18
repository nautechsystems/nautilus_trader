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

use nautilus_core::{ffi::string::str_to_cstr, time::UnixNanos};

use super::ticker::Ticker;
use crate::identifiers::instrument_id::InstrumentId;

#[no_mangle]
pub extern "C" fn ticker_new(
    instrument_id: InstrumentId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Ticker {
    Ticker::new(instrument_id, ts_event, ts_init)
}

/// Returns a [`Ticker`] as a C string pointer.
#[no_mangle]
pub extern "C" fn ticker_to_cstr(ticker: &Ticker) -> *const c_char {
    str_to_cstr(&ticker.to_string())
}
