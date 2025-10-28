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
    ffi::{CStr, CString, c_char},
    hash::{Hash, Hasher},
};

use crate::identifiers::trade_id::TradeId;

/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trade_id_new(ptr: *const c_char) -> TradeId {
    let value = unsafe { CStr::from_ptr(ptr).to_owned() };
    TradeId::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn trade_id_hash(id: &TradeId) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.value.hash(&mut hasher);
    hasher.finish()
}

#[unsafe(no_mangle)]
pub extern "C" fn trade_id_to_cstr(trade_id: &TradeId) -> *const c_char {
    trade_id.as_cstr().as_ptr()
}

impl From<CString> for TradeId {
    fn from(value: CString) -> Self {
        Self::from_bytes(value.as_bytes_with_nul()).unwrap()
    }
}
