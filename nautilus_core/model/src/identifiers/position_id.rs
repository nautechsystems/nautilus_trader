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
pub struct PositionId {
    value: Box<String>,
}

impl From<&str> for PositionId {
    fn from(s: &str) -> PositionId {
        PositionId {
            value: Box::new(s.to_string()),
        }
    }
}

impl Display for PositionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn position_id_free(position_id: PositionId) {
    drop(position_id); // Memory freed here
}

/// Returns a Nautilus identifier from a valid Python object pointer.
///
/// # Safety
///
/// - `ptr` must be borrowed from a valid Python UTF-8 `str`.
#[no_mangle]
pub unsafe extern "C" fn position_id_from_pystr(ptr: *mut ffi::PyObject) -> PositionId {
    PositionId {
        value: Box::new(pystr_to_string(ptr)),
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::PositionId;

    #[test]
    fn test_position_id_from_str() {
        let position_id1 = PositionId::from("ETHUSDT.BINANCE-EMACross-001");
        let position_id2 = PositionId::from("BTCUSDT.BINANCE-EMACross-002");

        assert_eq!(position_id1, position_id1);
        assert_ne!(position_id1, position_id2);
        assert_eq!(position_id1.to_string(), "ETHUSDT.BINANCE-EMACross-001");
    }

    #[test]
    fn test_position_id_as_str() {
        let position_id = PositionId::from("ETHUSDT.BINANCE-EMACross-001");

        assert_eq!(position_id.to_string(), "ETHUSDT.BINANCE-EMACross-001");
    }
}
