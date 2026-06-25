// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Represents a valid instrument ID.

use std::{
    fmt::{Debug, Display},
    hash::Hash,
    str::FromStr,
};

use nautilus_core::correctness::{CorrectnessError, FAILED};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

#[cfg(feature = "defi")]
use crate::defi::{Blockchain, validation::validate_address};
use crate::{
    enums::InstrumentClass,
    identifiers::{Symbol, Venue},
};

/// Represents a valid instrument ID.
///
/// The symbol and venue combination should uniquely identify the instrument.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct InstrumentId {
    /// The instruments ticker symbol.
    pub symbol: Symbol,
    /// The instruments trading venue.
    pub venue: Venue,
}

/// Error returned when a value is not a valid [`InstrumentId`].
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum InstrumentIdError {
    /// The value does not contain the required separator.
    #[error(
        "invalid `InstrumentId` value '{value}': missing '.' separator between symbol and venue components"
    )]
    MissingSeparator {
        /// The invalid identifier value.
        value: String,
    },
    /// The symbol component is invalid.
    #[error("invalid `InstrumentId` value '{value}': invalid symbol: {source}")]
    InvalidSymbol {
        /// The invalid identifier value.
        value: String,
        /// The symbol validation failure.
        source: Box<CorrectnessError>,
    },
    /// The venue component is invalid.
    #[error("invalid `InstrumentId` value '{value}': invalid venue: {source}")]
    InvalidVenue {
        /// The invalid identifier value.
        value: String,
        /// The venue validation failure.
        source: Box<CorrectnessError>,
    },
    /// The blockchain address component is invalid.
    #[error("invalid `InstrumentId` value '{value}': invalid blockchain address: {reason}")]
    InvalidAddress {
        /// The invalid identifier value.
        value: String,
        /// The address validation failure.
        reason: String,
    },
}

impl InstrumentId {
    /// Creates a new [`InstrumentId`] instance.
    #[must_use]
    pub fn new(symbol: Symbol, venue: Venue) -> Self {
        Self { symbol, venue }
    }

    #[must_use]
    pub fn is_synthetic(&self) -> bool {
        self.venue.is_synthetic()
    }
}

impl InstrumentId {
    /// # Errors
    ///
    /// Returns an error if `value` is not a valid identifier.
    pub fn from_as_ref<T: AsRef<str>>(value: T) -> Result<Self, InstrumentIdError> {
        Self::from_str(value.as_ref())
    }

    /// Extracts the blockchain from the venue if it's a DEX venue.
    #[cfg(feature = "defi")]
    #[must_use]
    pub fn blockchain(&self) -> Option<Blockchain> {
        self.venue
            .parse_dex()
            .map(|(blockchain, _)| blockchain)
            .ok()
    }

    /// Returns the parent-symbol components `(root, class)` if this id has
    /// a recognised parent shape `<root>.<class>` in its symbol component.
    ///
    /// Returns `None` when the symbol has zero or more than one `.`, or when
    /// the suffix is not a recognised [`InstrumentClass`] parent suffix
    /// (see [`InstrumentClass::try_from_parent_suffix`]).
    ///
    /// Used to gate parent-style subscription fan-out: a `None` return means
    /// the id does not refer to a parent group and must not be expanded.
    #[must_use]
    pub fn parse_parent_components(&self) -> Option<(&str, InstrumentClass)> {
        let symbol_str = self.symbol.as_str();
        let (root, suffix) = symbol_str.split_once('.')?;
        if root.is_empty() || suffix.contains('.') {
            return None;
        }
        let class = InstrumentClass::try_from_parent_suffix(suffix)?;
        Some((root, class))
    }
}

impl FromStr for InstrumentId {
    type Err = InstrumentIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = s.to_string();
        let (symbol_part, venue_part) =
            s.rsplit_once('.')
                .ok_or_else(|| InstrumentIdError::MissingSeparator {
                    value: value.clone(),
                })?;

        let venue =
            Venue::new_checked(venue_part).map_err(|source| InstrumentIdError::InvalidVenue {
                value: value.clone(),
                source: Box::new(source),
            })?;

        let symbol = {
            #[cfg(feature = "defi")]
            if venue.is_dex() {
                let validated_address = validate_address(symbol_part).map_err(|e| {
                    InstrumentIdError::InvalidAddress {
                        value: value.clone(),
                        reason: e.to_string(),
                    }
                })?;
                Symbol::new_checked(validated_address.to_string()).map_err(|source| {
                    InstrumentIdError::InvalidSymbol {
                        value: value.clone(),
                        source: Box::new(source),
                    }
                })?
            } else {
                Symbol::new_checked(symbol_part).map_err(|source| {
                    InstrumentIdError::InvalidSymbol {
                        value: value.clone(),
                        source: Box::new(source),
                    }
                })?
            }

            #[cfg(not(feature = "defi"))]
            Symbol::new_checked(symbol_part).map_err(|source| InstrumentIdError::InvalidSymbol {
                value: value.clone(),
                source: Box::new(source),
            })?
        };

        Ok(Self { symbol, venue })
    }
}

impl<T: AsRef<str>> From<T> for InstrumentId {
    fn from(value: T) -> Self {
        match Self::from_str(value.as_ref()) {
            Ok(instrument_id) => instrument_id,
            Err(e) => panic!("{FAILED}: {e}"),
        }
    }
}

impl Debug for InstrumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}.{}\"", self.symbol, self.venue)
    }
}

impl Display for InstrumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.symbol, self.venue)
    }
}

impl Serialize for InstrumentId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for InstrumentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let instrument_id_str: std::borrow::Cow<'de, str> = Deserialize::deserialize(deserializer)?;
        Self::from_str(instrument_id_str.as_ref()).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_core::correctness::CorrectnessError;
    use rstest::rstest;

    use super::{InstrumentId, InstrumentIdError};
    use crate::identifiers::stubs::*;

    #[rstest]
    fn test_instrument_id_parse_success(instrument_id_eth_usdt_binance: InstrumentId) {
        assert_eq!(instrument_id_eth_usdt_binance.symbol.to_string(), "ETHUSDT");
        assert_eq!(instrument_id_eth_usdt_binance.venue.to_string(), "BINANCE");
    }

    #[rstest]
    fn test_instrument_id_from_str_missing_separator_returns_typed_error() {
        let error = InstrumentId::from_str("ETHUSDT-BINANCE").unwrap_err();

        assert_eq!(
            error,
            InstrumentIdError::MissingSeparator {
                value: "ETHUSDT-BINANCE".to_string(),
            },
        );
        assert_eq!(
            error.to_string(),
            "invalid `InstrumentId` value 'ETHUSDT-BINANCE': missing '.' separator between symbol and venue components",
        );
    }

    #[rstest]
    #[should_panic(expected = "missing '.' separator between symbol and venue components")]
    fn test_instrument_id_from_panics_with_display_error() {
        let _ = InstrumentId::from("ETHUSDT-BINANCE");
    }

    #[rstest]
    fn test_instrument_id_from_str_invalid_symbol_returns_typed_error() {
        let error = InstrumentId::from_str(".BINANCE").unwrap_err();

        assert_eq!(
            error,
            InstrumentIdError::InvalidSymbol {
                value: ".BINANCE".to_string(),
                source: Box::new(CorrectnessError::EmptyString {
                    param: "value".to_string(),
                }),
            },
        );
        assert_eq!(
            error.to_string(),
            "invalid `InstrumentId` value '.BINANCE': invalid symbol: invalid string for 'value', was empty",
        );
    }

    #[rstest]
    fn test_instrument_id_from_str_invalid_venue_returns_typed_error() {
        let error = InstrumentId::from_str("ETHUSDT.BINANCÉ").unwrap_err();

        assert_eq!(
            error,
            InstrumentIdError::InvalidVenue {
                value: "ETHUSDT.BINANCÉ".to_string(),
                source: Box::new(CorrectnessError::NonAsciiString {
                    param: "value".to_string(),
                    value: "BINANCÉ".to_string(),
                }),
            },
        );
        assert_eq!(
            error.to_string(),
            concat!(
                "invalid `InstrumentId` value 'ETHUSDT.BINANCÉ': invalid venue: ",
                "invalid string for 'value' contained a non-ASCII char, was 'BINANCÉ'",
            ),
        );
    }

    #[rstest]
    fn test_string_reprs() {
        let id = InstrumentId::from("ETH/USDT.BINANCE");
        assert_eq!(id.to_string(), "ETH/USDT.BINANCE");
        assert_eq!(format!("{id}"), "ETH/USDT.BINANCE");
    }

    #[rstest]
    fn test_instrument_id_from_str_with_utf8_symbol() {
        let non_ascii_symbol = "TËST-PÉRP";
        let non_ascii_instrument = "TËST-PÉRP.BINANCE";

        let id = InstrumentId::from_str(non_ascii_instrument).unwrap();
        assert_eq!(id.symbol.to_string(), non_ascii_symbol);
        assert_eq!(id.venue.to_string(), "BINANCE");
        assert_eq!(id.to_string(), non_ascii_instrument);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_blockchain_instrument_id_valid() {
        let id =
            InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:UniswapV3");
        assert_eq!(
            id.symbol.to_string(),
            "0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443"
        );
        assert_eq!(id.venue.to_string(), "Arbitrum:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(
        expected = "invalid venue: Error creating `Venue` from 'InvalidChain:UniswapV3'"
    )]
    fn test_blockchain_instrument_id_invalid_chain() {
        let _ =
            InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.InvalidChain:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(expected = "invalid venue: Error creating `Venue` from 'Arbitrum:'")]
    fn test_blockchain_instrument_id_empty_dex() {
        let _ = InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_regular_venue_with_blockchain_like_name_but_without_dex() {
        // Should work fine since it doesn't contain ':' (not a DEX venue)
        let id = InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Ethereum");
        assert_eq!(
            id.symbol.to_string(),
            "0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443"
        );
        assert_eq!(id.venue.to_string(), "Ethereum");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(
        expected = "invalid blockchain address: Ethereum address must start with '0x': invalidaddress"
    )]
    fn test_blockchain_instrument_id_invalid_address_no_prefix() {
        let _ = InstrumentId::from("invalidaddress.Ethereum:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(
        expected = "invalid blockchain address: Blockchain address '0x123' is incorrect"
    )]
    fn test_blockchain_instrument_id_invalid_address_short() {
        let _ = InstrumentId::from("0x123.Ethereum:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(expected = "invalid character 'G' at position 39")]
    fn test_blockchain_instrument_id_invalid_address_non_hex() {
        let _ = InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa44G.Ethereum:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(expected = "has incorrect checksum")]
    fn test_blockchain_instrument_id_invalid_address_checksum() {
        let _ = InstrumentId::from("0xc31e54c7a869b9fcbecc14363cf510d1c41fa443.Ethereum:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_blockchain_extraction_valid_dex() {
        let id =
            InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:UniswapV3");
        let blockchain = id.blockchain();
        assert!(blockchain.is_some());
        assert_eq!(blockchain.unwrap(), crate::defi::Blockchain::Arbitrum);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_blockchain_extraction_tradifi_venue() {
        let id = InstrumentId::from("ETH/USDT.BINANCE");
        let blockchain = id.blockchain();
        assert!(blockchain.is_none());
    }

    use crate::enums::InstrumentClass;

    #[rstest]
    #[case("ES.FUT.XCME", Some(("ES", InstrumentClass::Future)))]
    #[case("ES.FUTURE.XCME", Some(("ES", InstrumentClass::Future)))]
    #[case("ES.OPT.XCME", Some(("ES", InstrumentClass::Option)))]
    #[case("ES.OPTION.XCME", Some(("ES", InstrumentClass::Option)))]
    #[case("CL.FUT.XNYM", Some(("CL", InstrumentClass::Future)))]
    #[case("ECES.OPT.XCME", Some(("ECES", InstrumentClass::Option)))]
    #[case("ESZ4.XCME", None)]
    #[case("AUDUSD.SIM", None)]
    #[case("1.211334112-31570229.BETFAIR", None)]
    #[case("ES.UNKNOWN.XCME", None)]
    #[case("ES.FUT.OOPS.XCME", None)]
    #[case("ES.fut.XCME", None)]
    #[case("ES.opt.XCME", None)]
    #[case(".FUT.XCME", None)]
    #[case(".OPT.XCME", None)]
    fn test_parse_parent_components(
        #[case] id_str: &str,
        #[case] expected: Option<(&str, InstrumentClass)>,
    ) {
        let id = InstrumentId::from(id_str);
        assert_eq!(id.parse_parent_components(), expected);
    }
}
