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
use std::os::raw::c_char;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use pyo3::ffi;

use nautilus_core::correctness;
use nautilus_core::string::{pystr_to_string, string_to_pystr, string_to_cstr};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
#[allow(clippy::redundant_allocation)] // C ABI compatibility
pub struct Venue {
    pub value: Box<Rc<String>>,
}

impl Display for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl Venue {
    pub fn new(s: &str) -> Venue {
        correctness::valid_string(s, "`Venue` value");

        Venue {
            value: Box::new(Rc::new(s.to_string())),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////

/// Returns a Nautilus identifier from a valid Python object pointer.
///
/// # Safety
/// - Assumes `ptr` is borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn venue_new(ptr: *mut ffi::PyObject) -> Venue {
    Venue::new(pystr_to_string(ptr).as_str())
}

#[no_mangle]
pub extern "C" fn venue_clone(venue: &Venue) -> Venue {
    venue.clone()
}

/// Frees the memory for the given `venue` by dropping.
#[no_mangle]
pub extern "C" fn venue_free(venue: Venue) {
    drop(venue); // Memory freed here
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
pub unsafe extern "C" fn venue_to_cstr(venue: &Venue) -> *const c_char {
    string_to_cstr(venue.value.as_str())
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
        let venue1 = Venue::new("BINANCE");
        let venue2 = Venue::new("IDEALPRO");

        assert_eq!(venue1, venue1);
        assert_ne!(venue1, venue2);
    }

    #[test]
    fn test_string_reprs() {
        let venue = Venue::new("BINANCE");

        assert_eq!(venue.to_string(), "BINANCE");
        assert_eq!(format!("{venue}"), "BINANCE");
    }

    #[test]
    fn test_venue_free() {
        let id = Venue::new("BINANCE");

        venue_free(id); // No panic
    }
}
