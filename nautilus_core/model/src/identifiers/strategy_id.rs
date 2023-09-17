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
};

use anyhow::Result;
use nautilus_core::correctness::{check_string_contains, check_valid_string};
use ustr::Ustr;

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct StrategyId {
    pub value: Ustr,
}

impl StrategyId {
    pub fn new(s: &str) -> Result<Self> {
        check_valid_string(s, "`StrategyId` value")?;
        if s != "EXTERNAL" {
            check_string_contains(s, "-", "`StrategyId` value")?;
        }

        Ok(Self {
            value: Ustr::from(s),
        })
    }
}

impl Default for StrategyId {
    fn default() -> Self {
        Self {
            value: Ustr::from("S-001"),
        }
    }
}

impl Debug for StrategyId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for StrategyId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for StrategyId {
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
pub unsafe extern "C" fn strategy_id_new(ptr: *const c_char) -> StrategyId {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    StrategyId::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn strategy_id_hash(id: &StrategyId) -> u64 {
    id.value.precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use crate::identifiers::strategy_id::StrategyId;

    #[fixture]
    pub fn strategy_id_ema_cross() -> StrategyId {
        StrategyId::from("EMACross-001")
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::StrategyId;
    use crate::identifiers::strategy_id::stubs::strategy_id_ema_cross;

    #[rstest]
    fn test_string_reprs(strategy_id_ema_cross: StrategyId) {
        assert_eq!(strategy_id_ema_cross.to_string(), "EMACross-001");
        assert_eq!(format!("{strategy_id_ema_cross}"), "EMACross-001");
    }
}
