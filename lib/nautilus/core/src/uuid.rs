// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::text::CStringRaw;
use std::ffi::CString;
use uuid::Uuid;

#[no_mangle]
pub extern "C" fn c_uuid_str_new() -> CStringRaw {
    return CString::new(Uuid::new_v4().to_string()).unwrap().into_raw();
}

#[no_mangle]
pub extern "C" fn c_uuid_str_free(s: CStringRaw) {
    unsafe {
        if s.is_null() {
            return;
        }
        CString::from_raw(s) // Frees memory here
    };
}
