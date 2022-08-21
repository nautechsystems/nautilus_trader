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

use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};

use pyo3::ffi;

use nautilus_core::string::{pystr_to_string, string_to_pystr};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
pub struct Venue {
    value: Box<String>,
}

impl From<&str> for Venue {
    fn from(s: &str) -> Venue {
        Venue {
            value: Box::new(s.to_string()),
        }
    }
}

impl Display for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn venue_free(venue: Venue) {
    drop(venue); // Memory freed here
}

/// Returns a Nautilus identifier from a valid Python object pointer.
///
/// # Safety
/// - Assumes `ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn venue_from_pystr(ptr: *mut ffi::PyObject) -> Venue {
    Venue {
        value: Box::new(pystr_to_string(ptr)),
    }
}

/// Returns a pointer to a valid Python UTF-8 string.
///
/// # Safety
/// - Assumes that since the data is originating from Rust, the GIL does not need
/// to be acquired.
/// - Assumes you are immediately returning this pointer to Python.
#[no_mangle]
pub unsafe extern "C" fn venue_to_pystr(venue: &Venue) -> *mut ffi::PyObject {
    string_to_pystr(venue.value.as_str())
}

#[no_mangle]
pub extern "C" fn venue_eq(lhs: &Venue, rhs: &Venue) -> u8 {
    (lhs == rhs) as u8
}

#[no_mangle]
pub extern "C" fn venue_hash(venue: &Venue) -> u64 {
    let mut h = DefaultHasher::new();
    venue.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::Venue;
    use crate::identifiers::venue::venue_free;

    #[test]
    fn test_equality() {
        let venue1 = Venue::from("FTX");
        let venue2 = Venue::from("IDEALPRO");

        assert_eq!(venue1, venue1);
        assert_ne!(venue1, venue2);
    }

    #[test]
    fn test_string_reprs() {
        let venue = Venue::from("FTX");

        assert_eq!(venue.to_string(), "FTX");
        assert_eq!(format!("{venue}"), "FTX");
    }

    #[test]
    fn test_venue_free() {
        let id = Venue::from("FTX");

        venue_free(id); // No panic
    }
}
