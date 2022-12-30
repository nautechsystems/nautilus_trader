// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
use std::fmt::Debug;
use std::os::raw::c_char;
use std::str::FromStr;

use pyo3::ffi;
use strum::{Display, EnumString, FromRepr};

use crate::string::{pystr_to_string, string_to_cstr};

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, FromRepr, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MessageCategory {
    Command = 1,
    Document = 2,
    Event = 3,
    Request = 4,
    Response = 5,
}

#[no_mangle]
pub extern "C" fn message_category_to_cstr(value: MessageCategory) -> *const c_char {
    string_to_cstr(&value.to_string())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn message_category_from_pystr(ptr: *mut ffi::PyObject) -> MessageCategory {
    let value = &pystr_to_string(ptr);
    MessageCategory::from_str(&pystr_to_string(ptr))
        .unwrap_or_else(|_| panic!("invalid `MessageCategory` enum string value, was '{value}'"))
}
