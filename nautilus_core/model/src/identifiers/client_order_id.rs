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
use pyo3::prelude::*;
use ustr::Ustr;

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[pyclass]
pub struct ClientOrderId {
    pub value: Ustr,
}

impl ClientOrderId {
    pub fn new(s: &str) -> Result<Self> {
        check_valid_string(s, "`ClientOrderId` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }
}

impl Default for ClientOrderId {
    fn default() -> Self {
        Self {
            value: Ustr::from("O-123456789"),
        }
    }
}

impl Debug for ClientOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for ClientOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

pub fn optional_ustr_to_vec_client_order_ids(s: Option<Ustr>) -> Option<Vec<ClientOrderId>> {
    s.map(|ustr| {
        let s_str = ustr.to_string();
        s_str
            .split(',')
            .map(|s| ClientOrderId::new(s).unwrap())
            .collect::<Vec<ClientOrderId>>()
    })
}

pub fn optional_vec_client_order_ids_to_ustr(vec: Option<Vec<ClientOrderId>>) -> Option<Ustr> {
    vec.map(|client_order_ids| {
        let s: String = client_order_ids
            .into_iter()
            .map(|id| id.value.to_string())
            .collect::<Vec<String>>()
            .join(",");
        Ustr::from(&s)
    })
}

impl From<&str> for ClientOrderId {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn client_order_id_new(ptr: *const c_char) -> ClientOrderId {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    ClientOrderId::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn client_order_id_hash(id: &ClientOrderId) -> u64 {
    id.value.precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;
    use ustr::Ustr;

    use super::ClientOrderId;
    use crate::identifiers::client_order_id::{
        optional_ustr_to_vec_client_order_ids, optional_vec_client_order_ids_to_ustr,
    };

    #[rstest]
    fn test_string_reprs() {
        let id = ClientOrderId::from("O-20200814-102234-001-001-1");
        assert_eq!(id.to_string(), "O-20200814-102234-001-001-1");
        assert_eq!(format!("{id}"), "O-20200814-102234-001-001-1");
    }

    #[rstest]
    fn test_optional_ustr_to_vec_client_order_ids() {
        // Test with None
        assert_eq!(optional_ustr_to_vec_client_order_ids(None), None);

        // Test with Some
        let ustr = Ustr::from("id1,id2,id3");
        let client_order_ids = optional_ustr_to_vec_client_order_ids(Some(ustr)).unwrap();
        assert_eq!(client_order_ids[0].value.to_string(), "id1");
        assert_eq!(client_order_ids[1].value.to_string(), "id2");
        assert_eq!(client_order_ids[2].value.to_string(), "id3");
    }

    #[rstest]
    fn test_optional_vec_client_order_ids_to_ustr() {
        // Test with None
        assert_eq!(optional_vec_client_order_ids_to_ustr(None), None);

        // Test with Some
        let client_order_ids = vec![
            ClientOrderId::from("id1"),
            ClientOrderId::from("id2"),
            ClientOrderId::from("id3"),
        ];
        let ustr = optional_vec_client_order_ids_to_ustr(Some(client_order_ids.into())).unwrap();
        assert_eq!(ustr.to_string(), "id1,id2,id3");
    }
}
