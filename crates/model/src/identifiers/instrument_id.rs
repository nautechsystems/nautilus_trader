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

use nautilus_core::correctness::check_valid_string;
use serde::{Deserialize, Deserializer, Serialize};

#[cfg(feature = "defi")]
use crate::defi::{chain::Chain, dex::DexType};
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
}

impl FromStr for InstrumentId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.rsplit_once('.') {
            Some((symbol_part, venue_part)) => {
                check_valid_string(symbol_part, stringify!(value))?;
                check_valid_string(venue_part, stringify!(value))?;

                // Validate blockchain venues when defi feature is enabled
                #[cfg(feature = "defi")]
                if venue_part.contains(':') {
                    if let Err(e) = validate_blockchain_venue(venue_part) {
                        anyhow::bail!(err_message(s, e.to_string()));
                    }
                }

                Ok(Self {
                    symbol: Symbol::new(symbol_part),
                    venue: Venue::new(venue_part),
                })
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

#[cfg(feature = "defi")]
fn validate_blockchain_venue(venue_part: &str) -> anyhow::Result<()> {
    if let Some((chain_name, dex_id)) = venue_part.split_once(':') {
        if chain_name.is_empty() || dex_id.is_empty() {
            anyhow::bail!(
                "invalid blockchain venue '{}': expected format 'Chain:DexId'",
                venue_part
            );
        }
        if Chain::from_chain_name(chain_name).is_none() {
            anyhow::bail!(
                "invalid blockchain venue '{}': chain '{}' not recognized",
                venue_part,
                chain_name
            );
        }
        if DexType::from_dex_name(dex_id).is_none() {
            anyhow::bail!(
                "invalid blockchain venue '{}': dex '{}' not recognized",
                venue_part,
                dex_id
            );
        }
        Ok(())
    } else {
        anyhow::bail!(
            "invalid blockchain venue '{}': expected format 'Chain:DexId'",
            venue_part
        );
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
        expected = "Error parsing `InstrumentId` from '0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.InvalidChain:UniswapV3': invalid blockchain venue 'InvalidChain:UniswapV3': chain 'InvalidChain' not recognized"
    )]
    fn test_blockchain_instrument_id_invalid_chain() {
        let _ =
            InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.InvalidChain:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(
        expected = "Error parsing `InstrumentId` from '0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:': invalid blockchain venue 'Arbitrum:': expected format 'Chain:DexId'"
    )]
    fn test_blockchain_instrument_id_empty_dex() {
        let _ = InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_regular_venue_with_blockchain_like_name_but_without_dex() {
        // Should work fine since it doesn't contain ':'
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
        expected = "Error parsing `InstrumentId` from '0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:InvalidDex': invalid blockchain venue 'Arbitrum:InvalidDex': dex 'InvalidDex' not recognized"
    )]
    fn test_blockchain_instrument_id_invalid_dex() {
        let _ =
            InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:InvalidDex");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_blockchain_instrument_id_valid_dex_names() {
        // Test various valid DEX names
        let valid_dexes = vec![
            "UniswapV3",
            "UniswapV2",
            "UniswapV4",
            "SushiSwapV2",
            "SushiSwapV3",
            "PancakeSwapV3",
            "CamelotV3",
            "CurveFinance",
            "FluidDEX",
            "MaverickV1",
            "MaverickV2",
            "BaseX",
            "BaseSwapV2",
            "AerodromeV1",
            "AerodromeSlipstream",
            "BalancerV2",
            "BalancerV3",
        ];

        for dex_name in valid_dexes {
            let venue_str = format!("Arbitrum:{}", dex_name);
            let id_str = format!("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.{}", venue_str);
            let id = InstrumentId::from(id_str.as_str());
            assert_eq!(id.venue.to_string(), venue_str);
        }
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[should_panic(
        expected = "Error parsing `InstrumentId` from '0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:uniswapv3': invalid blockchain venue 'Arbitrum:uniswapv3': dex 'uniswapv3' not recognized"
    )]
    fn test_blockchain_instrument_id_dex_case_sensitive() {
        // DEX names should be case sensitive
        let _ = InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:uniswapv3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_blockchain_instrument_id_chain_and_dex_validation_combined() {
        // Test that both chain and DEX validation work together
        let id =
            InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Ethereum:UniswapV3");
        assert_eq!(
            id.symbol.to_string(),
            "0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443"
        );
        assert_eq!(id.venue.to_string(), "Ethereum:UniswapV3");
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_blockchain_instrument_id_various_chain_dex_combinations() {
        // Test various valid chain:dex combinations
        let valid_combinations = vec![
            ("Ethereum", "UniswapV2"),
            ("Ethereum", "BalancerV2"),
            ("Arbitrum", "CamelotV3"),
            ("Base", "AerodromeV1"),
            ("Polygon", "SushiSwapV3"),
        ];

        for (chain, dex) in valid_combinations {
            let venue_str = format!("{}:{}", chain, dex);
            let id_str = format!("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.{}", venue_str);
            let id = InstrumentId::from(id_str.as_str());
            assert_eq!(id.venue.to_string(), venue_str);
        }
    }
}
