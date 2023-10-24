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
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use anyhow::Result;
use nautilus_core::correctness::check_valid_string;
use ustr::Ustr;

pub const SYNTHETIC_VENUE: &str = "SYNTH";

/// Represents a valid trading venue ID.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Venue {
    /// The venue ID value.
    pub value: Ustr,
}

impl Venue {
    pub fn new(s: &str) -> Result<Self> {
        check_valid_string(s, "`Venue` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }

    #[must_use]
    pub fn synthetic() -> Self {
        // SAFETY: using synethtic venue constant
        Self::new(SYNTHETIC_VENUE).unwrap()
    }

    pub fn is_synthetic(&self) -> bool {
        self.value.as_str() == SYNTHETIC_VENUE
    }
}

impl Default for Venue {
    fn default() -> Self {
        Self {
            value: Ustr::from("SIM"),
        }
    }
}

impl Debug for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for Venue {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use crate::identifiers::venue::Venue;

    #[fixture]
    pub fn binance() -> Venue {
        Venue::from("BINANCE")
    }
    #[fixture]
    pub fn sim() -> Venue {
        Venue::from("SIM")
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{stubs::*, Venue};

    #[rstest]
    fn test_string_reprs(binance: Venue) {
        assert_eq!(binance.to_string(), "BINANCE");
        assert_eq!(format!("{binance}"), "BINANCE");
    }
}
