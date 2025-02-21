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

use std::ffi::c_char;

use nautilus_core::{
    UUID4,
    ffi::string::{cstr_to_ustr, str_to_cstr},
};

use crate::timer::{TimeEvent, TimeEventCallback, TimeEventHandlerV2};

#[repr(C)]
#[derive(Clone, Debug)]
/// Legacy time event handler for Cython/FFI inter-operatbility
///
/// TODO: Remove once Cython is deprecated
///
/// `TimeEventHandler` associates a `TimeEvent` with a callback function that is triggered
/// when the event's timestamp is reached.
pub struct TimeEventHandler {
    /// The time event.
    pub event: TimeEvent,
    /// The callable raw pointer.
    pub callback_ptr: *mut c_char,
}

impl From<TimeEventHandlerV2> for TimeEventHandler {
    fn from(value: TimeEventHandlerV2) -> Self {
        Self {
            event: value.event,
            callback_ptr: match value.callback {
                TimeEventCallback::Python(callback) => callback.as_ptr().cast::<c_char>(),
                TimeEventCallback::Rust(_) => {
                    panic!("Legacy time event handler is not supported for Rust callback")
                }
            },
        }
    }
}

/// # Safety
///
/// - Assumes `name_ptr` is borrowed from a valid Python UTF-8 `str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn time_event_new(
    name_ptr: *const c_char,
    event_id: UUID4,
    ts_event: u64,
    ts_init: u64,
) -> TimeEvent {
    TimeEvent::new(
        unsafe { cstr_to_ustr(name_ptr) },
        event_id,
        ts_event.into(),
        ts_init.into(),
    )
}

/// Returns a [`TimeEvent`] as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn time_event_to_cstr(event: &TimeEvent) -> *const c_char {
    str_to_cstr(&event.to_string())
}

// This function only exists so that `TimeEventHandler` is included in the definitions
#[unsafe(no_mangle)]
pub const extern "C" fn dummy(v: TimeEventHandler) -> TimeEventHandler {
    v
}
