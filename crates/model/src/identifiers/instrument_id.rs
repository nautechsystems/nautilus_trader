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

//! Represents a valid instrument ID.

use std::{
    fmt::{Debug, Display, Formatter},
    hash::Hash,
    str::FromStr,
};

use nautilus_core::correctness::{check_valid_string_ascii, check_valid_string_utf8};
use serde::{Deserialize, Deserializer, Serialize};

#[cfg(feature = "defi")]
use crate::defi::{Blockchain, validation::validate_address};
use crate::identifiers::{Symbol, Venue};

/// Represents a valid instrument ID.
///
/// The symbol and venue combination should uniquely identify the instrument.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct InstrumentId {
    /// The instruments ticker symbol.
    pub symbol: Symbol,
    /// The instruments trading venue.
    pub venue: Venue,
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
    /// Returns an error if parsing the string fails or string is invalid.
    pub fn from_as_ref<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
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
}

impl FromStr for InstrumentId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.rsplit_once('.') {
            Some((symbol_part, venue_part)) => {
                check_valid_string_utf8(symbol_part, stringify!(value))?;
                check_valid_string_ascii(venue_part, stringify!(value))?;

                let venue = Venue::new_checked(venue_part)?;

                let symbol = {
                    #[cfg(feature = "defi")]
                    if venue.is_dex() {
                        let validated_address = validate_address(symbol_part)
                            .map_err(|e| anyhow::anyhow!(err_message(s, e.to_string())))?;
                        Symbol::new(validated_address.to_string())
                    } else {
                        Symbol::new(symbol_part)
                    }

                    #[cfg(not(feature = "defi"))]
                    Symbol::new(symbol_part)
                };

                Ok(Self { symbol, venue })
            }
            None => {
                anyhow::bail!(err_message(
                    s,
                    "missing '.' separator between symbol and venue components".to_string()
                ))
            }
        }
    }
}

impl From<&str> for InstrumentId {
    /// Creates a [`InstrumentId`] from a string slice.
    ///
    /// # Panics
    ///
    /// Panics if the `value` string is not valid.
    fn from(value: &str) -> Self {
        Self::from_str(value).unwrap()
    }
}

impl From<String> for InstrumentId {
    /// Creates a [`InstrumentId`] from a string.
    ///
    /// # Panics
    ///
    /// Panics if the `value` string is not valid.
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl Debug for InstrumentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}.{}\"", self.symbol, self.venue)
    }
}

impl Display for InstrumentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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
        let instrument_id_str = String::deserialize(deserializer)?;
        Ok(Self::from(instrument_id_str.as_str()))
    }
}

fn err_message(s: &str, e: String) -> String {
    format!("Error parsing `InstrumentId` from '{s}': {e}")
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::InstrumentId;
    use crate::identifiers::stubs::*;

    #[rstest]
    fn test_instrument_id_parse_success(instrument_id_eth_usdt_binance: InstrumentId) {
        assert_eq!(instrument_id_eth_usdt_binance.symbol.to_string(), "ETHUSDT");
        assert_eq!(instrument_id_eth_usdt_binance.venue.to_string(), "BINANCE");
    }

    #[rstest]
    #[should_panic(
        expected = "Error parsing `InstrumentId` from 'ETHUSDT-BINANCE': missing '.' separator between symbol and venue components"
    )]
    fn test_instrument_id_parse_failure_no_dot() {
        let _ = InstrumentId::from("ETHUSDT-BINANCE");
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
        expected = "Error creating `Venue` from 'InvalidChain:UniswapV3': invalid blockchain venue 'InvalidChain:UniswapV3': chain 'InvalidChain' not recognized"
    )]
    fn test_blockchain_instrument_id_invalid_chain() {
        let _ =
            InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.InvalidChain:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(
        expected = "Error creating `Venue` from 'Arbitrum:': invalid blockchain venue 'Arbitrum:': expected format 'Chain:DexId'"
    )]
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
        expected = "Error parsing `InstrumentId` from 'invalidaddress.Ethereum:UniswapV3': Ethereum address must start with '0x': invalidaddress"
    )]
    fn test_blockchain_instrument_id_invalid_address_no_prefix() {
        let _ = InstrumentId::from("invalidaddress.Ethereum:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(
        expected = "Error parsing `InstrumentId` from '0x123.Ethereum:UniswapV3': Blockchain address '0x123' is incorrect: odd number of digits"
    )]
    fn test_blockchain_instrument_id_invalid_address_short() {
        let _ = InstrumentId::from("0x123.Ethereum:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(
        expected = "Error parsing `InstrumentId` from '0xC31E54c7a869B9FcBEcc14363CF510d1c41fa44G.Ethereum:UniswapV3': Blockchain address '0xC31E54c7a869B9FcBEcc14363CF510d1c41fa44G' is incorrect: invalid character 'G' at position 39"
    )]
    fn test_blockchain_instrument_id_invalid_address_non_hex() {
        let _ = InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa44G.Ethereum:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(
        expected = "Error parsing `InstrumentId` from '0xc31e54c7a869b9fcbecc14363cf510d1c41fa443.Ethereum:UniswapV3': Blockchain address '0xc31e54c7a869b9fcbecc14363cf510d1c41fa443' has incorrect checksum"
    )]
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
}
