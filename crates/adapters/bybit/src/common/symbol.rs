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

//! Helpers for working with Bybit-specific symbol strings.

use std::{
    borrow::Cow,
    fmt::{Display, Formatter},
};

use nautilus_model::identifiers::{InstrumentId, Symbol};
use ustr::Ustr;

use super::{consts::BYBIT_VENUE, enums::BybitProductType};

const VALID_SUFFIXES: &[&str] = &["-SPOT", "-LINEAR", "-INVERSE", "-OPTION"];

/// Returns true if the supplied value contains a recognised Bybit product suffix.
fn has_valid_suffix(value: &str) -> bool {
    VALID_SUFFIXES.iter().any(|suffix| value.contains(suffix))
}

/// Represents a Bybit symbol augmented with a product-type suffix.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BybitSymbol {
    value: Ustr,
}

impl BybitSymbol {
    /// Creates a new [`BybitSymbol`] after validating the suffix and normalising to upper case.
    ///
    /// # Errors
    ///
    /// Returns an error if the value does not contain one of the recognised Bybit suffixes.
    pub fn new<S: AsRef<str>>(value: S) -> anyhow::Result<Self> {
        let value_ref = value.as_ref();
        let needs_upper = value_ref.bytes().any(|b| b.is_ascii_lowercase());
        let normalised: Cow<'_, str> = if needs_upper {
            Cow::Owned(value_ref.to_ascii_uppercase())
        } else {
            Cow::Borrowed(value_ref)
        };
        anyhow::ensure!(
            has_valid_suffix(normalised.as_ref()),
            "invalid Bybit symbol '{value_ref}': expected suffix in {VALID_SUFFIXES:?}"
        );
        Ok(Self {
            value: Ustr::from(normalised.as_ref()),
        })
    }

    /// Returns the underlying symbol without the Bybit suffix.
    #[must_use]
    pub fn raw_symbol(&self) -> &str {
        self.value
            .rsplit_once('-')
            .map_or(self.value.as_str(), |(prefix, _)| prefix)
    }

    /// Returns the product type identified by the suffix.
    #[must_use]
    pub fn product_type(&self) -> BybitProductType {
        if self.value.ends_with("-SPOT") {
            BybitProductType::Spot
        } else if self.value.ends_with("-LINEAR") {
            BybitProductType::Linear
        } else if self.value.ends_with("-INVERSE") {
            BybitProductType::Inverse
        } else if self.value.ends_with("-OPTION") {
            BybitProductType::Option
        } else {
            unreachable!("symbol checked for suffix during construction")
        }
    }

    /// Returns the instrument identifier corresponding to this symbol.
    #[must_use]
    pub fn to_instrument_id(&self) -> InstrumentId {
        InstrumentId::new(Symbol::from_ustr_unchecked(self.value), *BYBIT_VENUE)
    }

    /// Returns the symbol value as `Ustr`.
    #[must_use]
    pub fn as_ustr(&self) -> Ustr {
        self.value
    }
}

impl Display for BybitSymbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.value.as_str())
    }
}

impl TryFrom<&str> for BybitSymbol {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> anyhow::Result<Self> {
        Self::new(value)
    }
}

impl TryFrom<String> for BybitSymbol {
    type Error = anyhow::Error;

    fn try_from(value: String) -> anyhow::Result<Self> {
        Self::new(value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn new_valid_symbol_is_uppercased() {
        let symbol = BybitSymbol::new("btcusdt-linear").unwrap();
        assert_eq!(symbol.to_string(), "BTCUSDT-LINEAR");
    }

    #[rstest]
    fn new_invalid_symbol_errors() {
        let err = BybitSymbol::new("BTCUSDT").unwrap_err();
        assert!(format!("{err}").contains("expected suffix"));
    }

    #[rstest]
    fn raw_symbol_strips_suffix() {
        let symbol = BybitSymbol::new("ETH-26JUN26-16000-P-OPTION").unwrap();
        assert_eq!(symbol.raw_symbol(), "ETH-26JUN26-16000-P");
    }

    #[rstest]
    fn product_type_detection_matches_suffix() {
        let linear = BybitSymbol::new("BTCUSDT-LINEAR").unwrap();
        assert!(linear.product_type().is_linear());

        let inverse = BybitSymbol::new("BTCUSD-INVERSE").unwrap();
        assert!(inverse.product_type().is_inverse());

        let spot = BybitSymbol::new("ETHUSDT-SPOT").unwrap();
        assert!(spot.product_type().is_spot());

        let option = BybitSymbol::new("ETH-26JUN26-16000-P-OPTION").unwrap();
        assert!(option.product_type().is_option());
    }

    #[rstest]
    fn instrument_id_uses_bybit_venue() {
        let symbol = BybitSymbol::new("BTCUSDT-LINEAR").unwrap();
        let instrument_id = symbol.to_instrument_id();
        assert_eq!(instrument_id.to_string(), "BTCUSDT-LINEAR.BYBIT");
    }
}
