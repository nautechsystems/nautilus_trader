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

use std::{
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use nautilus_core::correctness::check_valid_string;
use ustr::Ustr;

use crate::venues::VENUE_MAP;

pub const SYNTHETIC_VENUE: &str = "SYNTH";

/// Represents a valid trading venue ID.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Venue(Ustr);

impl Venue {
    /// Creates a new `Venue` instance from the given identifier value.
    ///
    /// # Panics
    ///
    /// Panics if the value is not a valid string.
    pub fn new(value: &str) -> anyhow::Result<Self> {
        check_valid_string(value, stringify!(value))?;

        Ok(Self(Ustr::from(value)))
    }

    /// Sets the inner identifier value.
    pub(crate) fn set_inner(&mut self, value: &str) {
        self.0 = Ustr::from(value);
    }

    /// Returns the inner identifier value.
    #[must_use]
    pub fn inner(&self) -> Ustr {
        self.0
    }

    /// Returns the inner value as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    #[must_use]
    pub fn from_str_unchecked(s: &str) -> Self {
        Self(Ustr::from(s))
    }

    pub fn from_code(code: &str) -> anyhow::Result<Self> {
        let map_guard = VENUE_MAP
            .lock()
            .map_err(|e| anyhow::anyhow!("Error acquiring lock on `VENUE_MAP`: {e}"))?;
        map_guard
            .get(code)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Unknown venue code: {code}"))
    }

    #[must_use]
    pub fn synthetic() -> Self {
        // SAFETY: Unwrap safe as using known synthetic venue constant
        Self::new(SYNTHETIC_VENUE).unwrap()
    }

    #[must_use]
    pub fn is_synthetic(&self) -> bool {
        self.0.as_str() == SYNTHETIC_VENUE
    }
}

impl Debug for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Display for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for Venue {
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

    use crate::identifiers::{stubs::*, venue::Venue};

    #[rstest]
    fn test_string_reprs(venue_binance: Venue) {
        assert_eq!(venue_binance.as_str(), "BINANCE");
        assert_eq!(format!("{venue_binance}"), "BINANCE");
    }
}
