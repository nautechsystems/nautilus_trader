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
use nautilus_core::correctness;
use pyo3::prelude::*;
use ustr::Ustr;

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[pyclass]
pub struct VenueOrderId {
    pub value: Ustr,
}

impl VenueOrderId {
    pub fn new(s: &str) -> Result<Self> {
        correctness::valid_string(s, "`VenueOrderId` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }
}

impl Default for VenueOrderId {
    fn default() -> Self {
        Self {
            value: Ustr::from("001"),
        }
    }
}

impl Debug for VenueOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for VenueOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
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
#[no_mangle]
pub unsafe extern "C" fn venue_order_id_new(ptr: *const c_char) -> VenueOrderId {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    VenueOrderId::new(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed")).unwrap()
}

#[no_mangle]
pub extern "C" fn venue_order_id_hash(id: &VenueOrderId) -> u64 {
    id.value.precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::VenueOrderId;

    #[test]
    fn test_string_reprs() {
        let id = VenueOrderId::new("001").unwrap();
        assert_eq!(id.to_string(), "001");
        assert_eq!(format!("{id}"), "001");
    }
}
