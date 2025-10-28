// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Represents a valid ticker symbol ID for a tradable instrument.

use std::{
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use nautilus_core::correctness::{FAILED, check_valid_string_utf8};
use ustr::Ustr;

/// Represents a valid ticker symbol ID for a tradable instrument.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Symbol(Ustr);

impl Symbol {
    /// Creates a new [`Symbol`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `value` is not a valid string.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        let value = value.as_ref();
        check_valid_string_utf8(value, stringify!(value))?;
        Ok(Self(Ustr::from(value)))
    }

    /// Creates a new [`Symbol`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `value` is not a valid string.
    pub fn new<T: AsRef<str>>(value: T) -> Self {
        Self::new_checked(value).expect(FAILED)
    }

    /// Sets the inner identifier value.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(crate) fn set_inner(&mut self, value: &str) {
        self.0 = Ustr::from(value);
    }

    #[must_use]
    pub fn from_str_unchecked<T: AsRef<str>>(s: T) -> Self {
        Self(Ustr::from(s.as_ref()))
    }

    #[must_use]
    pub const fn from_ustr_unchecked(s: Ustr) -> Self {
        Self(s)
    }

    /// Returns the inner identifier value.
    #[must_use]
    pub fn inner(&self) -> Ustr {
        self.0
    }

    /// Returns the inner identifier value as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns true if the symbol string contains a period (`.`).
    #[must_use]
    pub fn is_composite(&self) -> bool {
        self.as_str().contains('.')
    }

    /// Returns the symbol root.
    ///
    /// The symbol root is the substring that appears before the first period (`.`)
    /// in the full symbol string. It typically represents the underlying asset for
    /// futures and options contracts. If no period is found, the entire symbol
    /// string is considered the root.
    #[must_use]
    pub fn root(&self) -> &str {
        let symbol_str = self.as_str();
        if let Some(index) = symbol_str.find('.') {
            &symbol_str[..index]
        } else {
            symbol_str
        }
    }

    /// Returns the symbol topic.
    ///
    /// The symbol topic is the root symbol with a wildcard (`*`) appended if the symbol has a root,
    /// otherwise returns the full symbol string.
    #[must_use]
    pub fn topic(&self) -> String {
        let root_str = self.root();
        if root_str == self.as_str() {
            root_str.to_string()
        } else {
            format!("{root_str}*")
        }
    }
}

impl Debug for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Ustr> for Symbol {
    fn from(value: Ustr) -> Self {
        Self(value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::identifiers::{Symbol, stubs::*};

    #[rstest]
    fn test_string_reprs(symbol_eth_perp: Symbol) {
        assert_eq!(symbol_eth_perp.as_str(), "ETH-PERP");
        assert_eq!(format!("{symbol_eth_perp}"), "ETH-PERP");
    }

    #[rstest]
    #[case("AUDUSD", false)]
    #[case("AUD/USD", false)]
    #[case("CL.FUT", true)]
    #[case("LO.OPT", true)]
    #[case("ES.c.0", true)]
    fn test_symbol_is_composite(#[case] input: &str, #[case] expected: bool) {
        let symbol = Symbol::new(input);
        assert_eq!(symbol.is_composite(), expected);
    }

    #[rstest]
    #[case("AUDUSD", "AUDUSD")]
    #[case("AUD/USD", "AUD/USD")]
    #[case("CL.FUT", "CL")]
    #[case("LO.OPT", "LO")]
    #[case("ES.c.0", "ES")]
    fn test_symbol_root(#[case] input: &str, #[case] expected_root: &str) {
        let symbol = Symbol::new(input);
        assert_eq!(symbol.root(), expected_root);
    }

    #[rstest]
    #[case("AUDUSD", "AUDUSD")]
    #[case("AUD/USD", "AUD/USD")]
    #[case("CL.FUT", "CL*")]
    #[case("LO.OPT", "LO*")]
    #[case("ES.c.0", "ES*")]
    fn test_symbol_topic(#[case] input: &str, #[case] expected_topic: &str) {
        let symbol = Symbol::new(input);
        assert_eq!(symbol.topic(), expected_topic);
    }

    #[rstest]
    #[case("")] // Empty string
    #[case("   ")] // Whitespace only
    fn test_symbol_with_invalid_values(#[case] input: &str) {
        assert!(Symbol::new_checked(input).is_err());
    }
}
