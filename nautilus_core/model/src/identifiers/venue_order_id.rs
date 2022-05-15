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

use nautilus_core::string::pystr_to_string;
use pyo3::ffi;
use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
pub struct VenueOrderId {
    value: Box<String>,
}

impl From<&str> for VenueOrderId {
    fn from(s: &str) -> VenueOrderId {
        VenueOrderId {
            value: Box::new(s.to_string()),
        }
    }
}

impl Display for VenueOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn venue_order_id_free(venue_order_id: VenueOrderId) {
    drop(venue_order_id); // Memory freed here
}

/// Returns a Nautilus identifier from a valid Python object pointer.
///
/// # Safety
///
/// - `ptr` must be borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn venue_order_id_from_pystr(ptr: *mut ffi::PyObject) -> VenueOrderId {
    VenueOrderId {
        value: Box::new(pystr_to_string(ptr)),
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::VenueOrderId;

    #[test]
    fn test_venue_order_id_from_str() {
        let venue_order_id1 = VenueOrderId::from("001");
        let venue_order_id2 = VenueOrderId::from("002");

        assert_eq!(venue_order_id1, venue_order_id1);
        assert_ne!(venue_order_id1, venue_order_id2);
        assert_eq!(venue_order_id1.to_string(), "001")
    }

    #[test]
    fn test_venue_order_id_as_str() {
        let venue_order_id = VenueOrderId::from("001");

        assert_eq!(venue_order_id.to_string(), "001")
    }
}
