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

use nautilus_core::ffi::string::cstr_to_str;

use crate::identifiers::client_id::ClientId;

/// Returns a Nautilus identifier from C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn client_id_new(ptr: *const c_char) -> ClientId {
    ClientId::from(cstr_to_str(ptr))
}

#[no_mangle]
pub extern "C" fn client_id_hash(id: &ClientId) -> u64 {
    id.value.precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use rstest::rstest;

    use super::*;
    use crate::identifiers::stubs::*;

    #[rstest]
    fn test_client_id_to_cstr_c() {
        let id = ClientId::from("BINANCE");
        let c_string = id.value.as_char_ptr();
        let rust_string = unsafe { CStr::from_ptr(c_string) }.to_str().unwrap();
        assert_eq!(rust_string, "BINANCE");
    }

    #[rstest]
    fn test_client_id_hash_c() {
        let id1 = client_id_binance();
        let id2 = client_id_binance();
        let id3 = client_id_dydx();
        assert_eq!(client_id_hash(&id1), client_id_hash(&id2));
        assert_ne!(client_id_hash(&id1), client_id_hash(&id3));
    }
}
