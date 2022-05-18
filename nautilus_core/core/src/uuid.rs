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

use crate::impl_identifier_boundary_api;
use pyo3::ffi;
use std::fmt::{Debug, Display, Formatter, Result};
use uuid::Uuid;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
pub struct UUID4 {
    value: Box<String>,
}

impl UUID4 {
    pub fn new() -> UUID4 {
        let uuid = Uuid::new_v4();
        UUID4 {
            value: Box::new(uuid.to_string()),
        }
    }
}

impl From<&str> for UUID4 {
    fn from(s: &str) -> Self {
        let uuid = Uuid::parse_str(s).unwrap();
        UUID4 {
            value: Box::new(uuid.to_string()),
        }
    }
}

impl Default for UUID4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn uuid4_new() -> UUID4 {
    UUID4::new()
}

use nautilus_core::string::IdentifierBoundaryAPI;
use nautilus_core::string::{pystr_to_string, string_to_pystr};
impl IdentifierBoundaryAPI for UUID4 {
    #[export_name = "UUID4_free"]
    extern "C" fn free(self: Self)
    where
        Self: Sized,
    {
        drop(self);
    }

    /// Returns a Nautilus identifier from a valid Python object pointer.
    ///
    /// # Safety
    ///
    /// - `ptr` must be borrowed from a valid Python UTF-8 `str`.
    #[export_name = "UUID4_from_pystr"]
    unsafe extern "C" fn from_pystr(ptr: *mut ffi::PyObject) -> Self {
        Self {
            value: Box::new(pystr_to_string(ptr)),
        }
    }

    /// Returns a pointer to a valid Python UTF-8 string.
    ///
    /// # Safety
    ///
    /// - Assumes that since the data is originating from Rust, the GIL does not need
    /// to be acquired.
    /// - Assumes you are immediately returning this pointer to Python.
    #[export_name = "UUID4_to_pystr"]
    unsafe extern "C" fn to_pystr(&self) -> *mut ffi::PyObject {
        string_to_pystr(self.value.as_str())
    }
}

#[export_name = "UUID4_from_pystr"]
pub unsafe extern "C" fn from_pystr(ptr: *mut ffi::PyObject) -> UUID4 {
    UUID4 {
        value: Box::new(pystr_to_string(ptr)),
    }
}
