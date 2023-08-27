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
pub struct PositionId {
    pub value: Ustr,
}

impl PositionId {
    pub fn new(s: &str) -> Result<Self> {
        check_valid_string(s, "`PositionId` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }
}

impl Default for PositionId {
    fn default() -> Self {
        Self {
            value: Ustr::from("P-001"),
        }
    }
}

impl Debug for PositionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}
impl Display for PositionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for PositionId {
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
#[no_mangle]
pub unsafe extern "C" fn position_id_new(ptr: *const c_char) -> PositionId {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    PositionId::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[no_mangle]
pub extern "C" fn position_id_hash(id: &PositionId) -> u64 {
    id.value.precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::PositionId;

    #[test]
    fn test_string_reprs() {
        let id = PositionId::from("P-123456789");
        assert_eq!(id.to_string(), "P-123456789");
        assert_eq!(format!("{id}"), "P-123456789");
    }
}
