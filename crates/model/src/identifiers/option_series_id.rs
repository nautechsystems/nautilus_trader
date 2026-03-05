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

//! Represents a unique option series identifier (venue + underlying + expiry).

use std::{
    fmt::{Debug, Display},
    hash::Hash,
    str::FromStr,
};

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{identifiers::Venue, instruments::CryptoOption};

/// Identifies a unique option series: a specific venue + underlying + settlement currency + expiration.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct OptionSeriesId {
    /// The trading venue.
    pub venue: Venue,
    /// The underlying asset symbol (e.g. "BTC").
    pub underlying: Ustr,
    /// The settlement currency code (e.g. "BTC" for inverse, "USDC" for linear).
    pub settlement_currency: Ustr,
    /// UNIX timestamp (nanoseconds) for contract expiration.
    pub expiration_ns: UnixNanos,
}

impl OptionSeriesId {
    /// Creates a new [`OptionSeriesId`] instance.
    #[must_use]
    pub fn new(
        venue: Venue,
        underlying: Ustr,
        settlement_currency: Ustr,
        expiration_ns: UnixNanos,
    ) -> Self {
        Self {
            venue,
            underlying,
            settlement_currency,
            expiration_ns,
        }
    }

    /// Creates an [`OptionSeriesId`] from venue name, underlying symbol, settlement currency, and date string.
    ///
    /// The `date_str` is parsed via `UnixNanos::FromStr`, which accepts `"YYYY-MM-DD"`,
    /// RFC 3339 timestamps, integer nanoseconds, or floating-point seconds.
    ///
    /// # Errors
    ///
    /// Returns an error if `date_str` cannot be parsed as a valid date or timestamp.
    pub fn from_expiry(
        venue: &str,
        underlying: &str,
        settlement_currency: &str,
        date_str: &str,
    ) -> anyhow::Result<Self> {
        let expiration_ns = UnixNanos::from_str(date_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse expiry date '{date_str}': {e}"))?;
        Ok(Self {
            venue: Venue::new(venue),
            underlying: Ustr::from(underlying),
            settlement_currency: Ustr::from(settlement_currency),
            expiration_ns,
        })
    }

    /// Returns the canonical wire representation with nanosecond expiry
    /// (e.g. `DERIBIT:BTC:BTC:1772524800000000000`).
    ///
    /// Used for serialization and persistence where exact round-tripping is required.
    #[must_use]
    pub fn to_wire_string(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.venue, self.underlying, self.settlement_currency, self.expiration_ns
        )
    }

    /// Creates an [`OptionSeriesId`] from a [`CryptoOption`] instrument.
    #[must_use]
    pub fn from_crypto_option(option: &CryptoOption) -> Self {
        Self {
            venue: option.id.venue,
            underlying: option.underlying.code,
            settlement_currency: option.settlement_currency.code,
            expiration_ns: option.expiration_ns,
        }
    }
}

impl Display for OptionSeriesId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dt = self.expiration_ns.to_datetime_utc();
        write!(
            f,
            "{}:{}:{}:{}",
            self.venue,
            self.underlying,
            self.settlement_currency,
            dt.format("%Y-%m-%dT%H:%M:%SZ"),
        )
    }
}

impl Debug for OptionSeriesId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dt = self.expiration_ns.to_datetime_utc();
        write!(
            f,
            "\"{}:{}:{}:{}\"",
            self.venue,
            self.underlying,
            self.settlement_currency,
            dt.format("%Y-%m-%dT%H:%M:%SZ"),
        )
    }
}

impl FromStr for OptionSeriesId {
    type Err = anyhow::Error;

    /// Parses `VENUE:UNDERLYING:SETTLEMENT:EXPIRY` where EXPIRY can be
    /// nanoseconds (`1772524800000000000`) or a date (`2026-03-03`).
    fn from_str(s: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = s.splitn(4, ':').collect();
        if parts.len() != 4 {
            anyhow::bail!(
                "Error parsing `OptionSeriesId` from '{s}': expected format 'VENUE:UNDERLYING:SETTLEMENT:EXPIRY'"
            );
        }

        let venue = Venue::new(parts[0]);
        let underlying = Ustr::from(parts[1]);
        let settlement_currency = Ustr::from(parts[2]);
        let expiration_ns = UnixNanos::from_str(parts[3]).map_err(|e| {
            anyhow::anyhow!(
                "Error parsing `OptionSeriesId` expiration from '{}': {e}",
                parts[3]
            )
        })?;

        Ok(Self {
            venue,
            underlying,
            settlement_currency,
            expiration_ns,
        })
    }
}

impl Serialize for OptionSeriesId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_wire_string())
    }
}

impl<'de> Deserialize<'de> for OptionSeriesId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(deserializer)?;
        Self::from_str(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    fn test_series_id() -> OptionSeriesId {
        OptionSeriesId::new(
            Venue::new("DERIBIT"),
            Ustr::from("BTC"),
            Ustr::from("BTC"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        )
    }

    #[rstest]
    fn test_option_series_id_new() {
        let venue = Venue::new("DERIBIT");
        let underlying = Ustr::from("BTC");
        let settlement = Ustr::from("BTC");
        let expiration_ns = UnixNanos::from(1_700_000_000_000_000_000u64);

        let id = OptionSeriesId::new(venue, underlying, settlement, expiration_ns);

        assert_eq!(id.venue, venue);
        assert_eq!(id.underlying, underlying);
        assert_eq!(id.settlement_currency, settlement);
        assert_eq!(id.expiration_ns, expiration_ns);
    }

    #[rstest]
    fn test_option_series_id_display() {
        let id = test_series_id();
        assert_eq!(id.to_string(), "DERIBIT:BTC:BTC:2023-11-14T22:13:20Z");
    }

    #[rstest]
    fn test_option_series_id_wire_string() {
        let id = test_series_id();
        assert_eq!(id.to_wire_string(), "DERIBIT:BTC:BTC:1700000000000000000");
    }

    #[rstest]
    fn test_option_series_id_debug() {
        let id = test_series_id();
        assert_eq!(
            format!("{id:?}"),
            "\"DERIBIT:BTC:BTC:2023-11-14T22:13:20Z\""
        );
    }

    #[rstest]
    fn test_option_series_id_from_str() {
        let id = OptionSeriesId::from_str("DERIBIT:BTC:BTC:1700000000000000000").unwrap();

        assert_eq!(id.venue, Venue::new("DERIBIT"));
        assert_eq!(id.underlying, Ustr::from("BTC"));
        assert_eq!(id.settlement_currency, Ustr::from("BTC"));
        assert_eq!(
            id.expiration_ns,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
    }

    #[rstest]
    fn test_option_series_id_from_str_rfc3339() {
        let id = OptionSeriesId::from_str("DERIBIT:BTC:BTC:2023-11-14T22:13:20Z").unwrap();
        assert_eq!(id.venue, Venue::new("DERIBIT"));
        assert_eq!(id.underlying, Ustr::from("BTC"));
        assert_eq!(
            id.expiration_ns,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
    }

    #[rstest]
    fn test_option_series_id_from_str_date() {
        let id = OptionSeriesId::from_str("DERIBIT:BTC:BTC:2023-11-14").unwrap();
        assert_eq!(id.venue, Venue::new("DERIBIT"));
        assert_eq!(id.underlying, Ustr::from("BTC"));
        // Date parses as midnight UTC (1699920000 seconds)
        assert_eq!(
            id.expiration_ns,
            UnixNanos::from(1_699_920_000_000_000_000u64)
        );
    }

    #[rstest]
    fn test_option_series_id_from_str_invalid_format() {
        assert!(OptionSeriesId::from_str("DERIBIT:BTC:BTC").is_err());
    }

    #[rstest]
    fn test_option_series_id_from_str_invalid_expiry() {
        assert!(OptionSeriesId::from_str("DERIBIT:BTC:BTC:not_a_date").is_err());
    }

    #[rstest]
    fn test_option_series_id_inequality() {
        let id1 = test_series_id();
        let id2 = OptionSeriesId::new(
            Venue::new("DERIBIT"),
            Ustr::from("ETH"),
            Ustr::from("ETH"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        );
        assert_ne!(id1, id2);
    }

    #[rstest]
    fn test_option_series_id_hash() {
        use std::collections::HashSet;

        let id1 = test_series_id();
        let id2 = OptionSeriesId::new(
            Venue::new("DERIBIT"),
            Ustr::from("ETH"),
            Ustr::from("ETH"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        );

        let mut set = HashSet::new();
        set.insert(id1);
        set.insert(id2);
        set.insert(id1); // duplicate

        assert_eq!(set.len(), 2);
    }

    #[rstest]
    fn test_option_series_id_serde_roundtrip() {
        let id = test_series_id();

        let json = serde_json::to_string(&id).unwrap();
        let deserialized: OptionSeriesId = serde_json::from_str(&json).unwrap();

        assert_eq!(id, deserialized);
    }

    #[rstest]
    fn test_from_expiry_happy_path() {
        let id = OptionSeriesId::from_expiry("DERIBIT", "BTC", "BTC", "2025-03-28").unwrap();
        assert_eq!(id.venue, Venue::new("DERIBIT"));
        assert_eq!(id.underlying, Ustr::from("BTC"));
        assert_eq!(id.settlement_currency, Ustr::from("BTC"));
        assert!(id.expiration_ns.as_u64() > 0);
    }

    #[rstest]
    fn test_from_expiry_invalid_date() {
        let result = OptionSeriesId::from_expiry("DERIBIT", "BTC", "BTC", "not-a-date");
        assert!(result.is_err());
    }

    #[rstest]
    fn test_from_expiry_roundtrip() {
        let id = OptionSeriesId::from_expiry("DERIBIT", "ETH", "ETH", "2025-06-27").unwrap();
        let s = id.to_string();
        let parsed = OptionSeriesId::from_str(&s).unwrap();
        assert_eq!(id, parsed);
    }
}
