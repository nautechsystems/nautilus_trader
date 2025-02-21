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

use nautilus_core::ffi::string::{cstr_as_str, str_to_cstr};

use crate::identifiers::Symbol;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn symbol_new(ptr: *const c_char) -> Symbol {
    let value = unsafe { cstr_as_str(ptr) };
    Symbol::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn symbol_hash(id: &Symbol) -> u64 {
    id.inner().precomputed_hash()
}

#[unsafe(no_mangle)]
pub extern "C" fn symbol_is_composite(id: &Symbol) -> u8 {
    u8::from(id.is_composite())
}

#[unsafe(no_mangle)]
pub extern "C" fn symbol_root(id: &Symbol) -> *const c_char {
    str_to_cstr(id.root())
}

#[unsafe(no_mangle)]
pub extern "C" fn symbol_topic(id: &Symbol) -> *const c_char {
    str_to_cstr(&id.topic())
}
