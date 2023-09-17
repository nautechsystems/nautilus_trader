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

use std::{
    ffi::{c_char, CStr},
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use anyhow::Result;
use nautilus_core::correctness::check_valid_string;
use ustr::Ustr;

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct ClientId {
    pub value: Ustr,
}

impl ClientId {
    pub fn new(s: &str) -> Result<Self> {
        check_valid_string(s, "`ClientId` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }
}

impl Debug for ClientId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for ClientId {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a Nautilus identifier from C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn client_id_new(ptr: *const c_char) -> ClientId {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    ClientId::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn client_id_hash(id: &ClientId) -> u64 {
    id.value.precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use crate::identifiers::client_id::ClientId;

    #[fixture]
    pub fn client_binance() -> ClientId {
        ClientId::from("BINANCE")
    }

    #[fixture]
    pub fn client_dydx() -> ClientId {
        ClientId::from("COINBASE")
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{stubs::*, *};

    #[rstest]
    fn test_string_reprs(client_binance: ClientId) {
        assert_eq!(client_binance.to_string(), "BINANCE");
        assert_eq!(format!("{client_binance}"), "BINANCE");
    }

    #[rstest]
    fn test_client_id_to_cstr_c() {
        let id = ClientId::from("BINANCE");
        let c_string = id.value.as_char_ptr();
        let rust_string = unsafe { CStr::from_ptr(c_string) }.to_str().unwrap();
        assert_eq!(rust_string, "BINANCE");
    }

    #[rstest]
    fn test_client_id_hash_c() {
        let id1 = client_binance();
        let id2 = client_binance();
        let id3 = client_dydx();
        assert_eq!(client_id_hash(&id1), client_id_hash(&id2));
        assert_ne!(client_id_hash(&id1), client_id_hash(&id3));
    }
}
