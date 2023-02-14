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

use std::collections::hash_map::DefaultHasher;
use std::ffi::{c_char, CStr};
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use nautilus_core::correctness;
use nautilus_core::string::string_to_cstr;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
#[allow(clippy::redundant_allocation)] // C ABI compatibility
pub struct TradeId {
    pub value: Box<Rc<String>>,
}

impl Display for TradeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

impl TradeId {
    #[must_use]
    pub fn new(s: &str) -> Self {
        correctness::valid_string(s, "`TradeId` value");

        TradeId {
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
pub unsafe extern "C" fn trade_id_new(ptr: *const c_char) -> TradeId {
    TradeId::new(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[no_mangle]
pub extern "C" fn trade_id_clone(trade_id: &TradeId) -> TradeId {
    trade_id.clone()
}

/// Frees the memory for the given `trade_id` by dropping.
#[no_mangle]
pub extern "C" fn trade_id_free(trade_id: TradeId) {
    drop(trade_id); // Memory freed here
}

/// Returns [TradeId] as a C string pointer.
#[no_mangle]
pub extern "C" fn trade_id_to_cstr(trade_id: &TradeId) -> *const c_char {
    string_to_cstr(&trade_id.value)
}

#[no_mangle]
pub extern "C" fn trade_id_eq(lhs: &TradeId, rhs: &TradeId) -> u8 {
    u8::from(lhs == rhs)
}

#[no_mangle]
pub extern "C" fn trade_id_hash(trade_id: &TradeId) -> u64 {
    let mut h = DefaultHasher::new();
    trade_id.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::TradeId;
    use crate::identifiers::trade_id::trade_id_free;

    #[test]
    fn test_equality() {
        let trade_id1 = TradeId::new("123456789");
        let trade_id2 = TradeId::new("234567890");
        assert_eq!(trade_id1, trade_id1);
        assert_ne!(trade_id1, trade_id2);
    }

    #[test]
    fn test_string_reprs() {
        let trade_id = TradeId::new("1234567890");
        assert_eq!(trade_id.to_string(), "1234567890");
        assert_eq!(format!("{trade_id}"), "1234567890");
    }

    #[test]
    fn test_trade_id_free() {
        let id = TradeId::new("123456789");
        trade_id_free(id); // No panic
    }
}
