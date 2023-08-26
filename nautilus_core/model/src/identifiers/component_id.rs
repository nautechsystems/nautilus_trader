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
pub struct ComponentId {
    pub value: Ustr,
}

impl ComponentId {
    pub fn new(s: &str) -> Result<Self> {
        correctness::valid_string(s, "`ComponentId` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }
}

impl Debug for ComponentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for ComponentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for ComponentId {
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
pub unsafe extern "C" fn component_id_new(ptr: *const c_char) -> ComponentId {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    ComponentId::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[no_mangle]
pub extern "C" fn component_id_hash(id: &ComponentId) -> u64 {
    id.value.precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::ComponentId;

    #[test]
    fn test_string_reprs() {
        let id = ComponentId::new("RiskEngine").unwrap();
        assert_eq!(id.to_string(), "RiskEngine");
        assert_eq!(format!("{id}"), "RiskEngine");
    }
}
