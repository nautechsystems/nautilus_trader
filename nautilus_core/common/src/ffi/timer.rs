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

use std::ffi::c_char;

use nautilus_core::{
    ffi::string::{cstr_to_ustr, str_to_cstr},
    uuid::UUID4,
};

use crate::timer::{TimeEvent, TimeEventHandler};

/// # Safety
///
/// - Assumes `name_ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn time_event_new(
    name_ptr: *const c_char,
    event_id: UUID4,
    ts_event: u64,
    ts_init: u64,
) -> TimeEvent {
    TimeEvent::new(cstr_to_ustr(name_ptr), event_id, ts_event, ts_init)
}

/// Returns a [`TimeEvent`] as a C string pointer.
#[no_mangle]
pub extern "C" fn time_event_to_cstr(event: &TimeEvent) -> *const c_char {
    str_to_cstr(&event.to_string())
}

// This function only exists so that `TimeEventHandler` is included in the definitions
#[no_mangle]
pub extern "C" fn dummy(v: TimeEventHandler) -> TimeEventHandler {
    v
}
