// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::{Debug, Display, Formatter};

use nautilus_core::correctness::{check_string_contains, check_valid_string};
use ustr::Ustr;

/// Represents a valid trader ID.
///
/// Must be correctly formatted with two valid strings either side of a hyphen.
/// It is expected a trader ID is the abbreviated name of the trader
/// with an order ID tag number separated by a hyphen.
///
/// Example: "TESTER-001".

/// The reason for the numerical component of the ID is so that order and position IDs
/// do not collide with those from another node instance.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct TraderId {
    /// The trader ID value.
    pub value: Ustr,
}

impl TraderId {
    pub fn new(value: &str) -> anyhow::Result<Self> {
        check_valid_string(value, stringify!(value))?;
        check_string_contains(value, "-", stringify!(value))?;

        Ok(Self {
            value: Ustr::from(value),
        })
    }

    #[must_use]
    pub fn get_tag(&self) -> &str {
        // SAFETY: Unwrap safe as value previously validated
        self.value.split('-').last().unwrap()
    }
}

impl Default for TraderId {
    fn default() -> Self {
        Self {
            value: Ustr::from("TRADER-000"),
        }
    }
}

impl Debug for TraderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for TraderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for TraderId {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::identifiers::{stubs::*, trader_id::TraderId};

    #[rstest]
    fn test_string_reprs(trader_id: TraderId) {
        assert_eq!(trader_id.to_string(), "TRADER-001");
        assert_eq!(format!("{trader_id}"), "TRADER-001");
    }

    #[rstest]
    fn test_get_tag(trader_id: TraderId) {
        assert_eq!(trader_id.get_tag(), "001");
    }
}
