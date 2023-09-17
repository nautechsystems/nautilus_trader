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
use ustr::Ustr;

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Symbol {
    pub value: Ustr,
}

impl Symbol {
    pub fn new(s: &str) -> Result<Self> {
        check_valid_string(s, "`Symbol` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }
}

impl Default for Symbol {
    fn default() -> Self {
        Self {
            value: Ustr::from("AUD/USD"),
        }
    }
}

impl Debug for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for Symbol {
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
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn symbol_new(ptr: *const c_char) -> Symbol {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    Symbol::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn symbol_hash(id: &Symbol) -> u64 {
    id.value.precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use crate::identifiers::symbol::Symbol;

    #[fixture]
    pub fn eth_perp() -> Symbol {
        Symbol::from("ETH-PERP")
    }

    #[fixture]
    pub fn aud_usd() -> Symbol {
        Symbol::from("AUDUSD")
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{stubs::*, Symbol};

    #[rstest]
    fn test_string_reprs(eth_perp: Symbol) {
        assert_eq!(eth_perp.to_string(), "ETH-PERP");
        assert_eq!(format!("{eth_perp}"), "ETH-PERP");
    }
}
