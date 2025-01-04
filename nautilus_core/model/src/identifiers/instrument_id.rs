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
    /// This function panics:
    /// - If the `value` string is not valid.
    fn from(value: &str) -> Self {
        Self::from_str(value).unwrap()
    }
}

impl From<String> for InstrumentId {
    /// Creates a [`InstrumentId`] from a string.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If the `value` string is not valid.
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
}
