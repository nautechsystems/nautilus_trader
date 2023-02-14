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

use std::ffi::{c_char, CStr};
use std::fmt::{Debug, Display, Formatter, Result};
use std::rc::Rc;

use nautilus_core::correctness;
use nautilus_core::string::string_to_cstr;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
#[allow(clippy::redundant_allocation)] // C ABI compatibility
pub struct StrategyId {
    pub value: Box<Rc<String>>,
}

impl Display for StrategyId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl StrategyId {
    #[must_use]
    pub fn new(s: &str) -> Self {
        correctness::valid_string(s, "`StrategyId` value");
        if s != "EXTERNAL" {
            correctness::string_contains(s, "-", "`StrategyId` value");
        }

        StrategyId {
            value: Box::new(Rc::new(s.to_string())),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn strategy_id_new(ptr: *const c_char) -> StrategyId {
    StrategyId::new(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[no_mangle]
pub extern "C" fn strategy_id_clone(strategy_id: &StrategyId) -> StrategyId {
    strategy_id.clone()
}

/// Frees the memory for the given `strategy_id` by dropping.
#[no_mangle]
pub extern "C" fn strategy_id_free(strategy_id: StrategyId) {
    drop(strategy_id); // Memory freed here
}

/// Returns a [`StrategyId`] as a C string pointer.
#[no_mangle]
pub extern "C" fn strategy_id_to_cstr(strategy_id: &StrategyId) -> *const c_char {
    string_to_cstr(&strategy_id.value)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::StrategyId;
    use crate::identifiers::strategy_id::strategy_id_free;

    #[test]
    fn test_equality() {
        let id1 = StrategyId::new("EMACross-001");
        let id2 = StrategyId::new("EMACross-002");
        assert_eq!(id1, id1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_string_reprs() {
        let id = StrategyId::new("EMACross-001");
        assert_eq!(id.to_string(), "EMACross-001");
        assert_eq!(format!("{id}"), "EMACross-001");
    }

    #[test]
    fn test_strategy_id_free() {
        let id = StrategyId::new("EMACross-001");
        strategy_id_free(id); // No panic
    }
}
