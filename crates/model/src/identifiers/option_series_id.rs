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

//! Represents an option series and its underlying reference instrument.

use std::{
    fmt::{Debug, Display},
    hash::Hash,
    str::FromStr,
};

use nautilus_core::{UnixNanos, correctness::CorrectnessError};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ustr::Ustr;

use crate::{
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::CryptoOption,
};

/// Identifies an option series and the instrument supplying its reference price.
///
/// A reference matching `<UNDERLYING>.<VENUE>` uses the four-part representation. Others use
/// `VENUE:UNDERLYING:UNDERLYING_INSTRUMENT_ID:SETTLEMENT:EXPIRY`.
/// The reference instrument participates in equality, hashing, and ordering.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
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
    /// The instrument supplying the reference price, which may trade on another venue.
    pub underlying_instrument_id: InstrumentId,
}

/// Error returned when a value is not a valid [`OptionSeriesId`].
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum OptionSeriesIdError {
    /// The value does not match the expected format.
    #[error(
        "invalid `OptionSeriesId` value '{value}': expected format 'VENUE:UNDERLYING:[REFERENCE:]SETTLEMENT:EXPIRY'"
    )]
    InvalidFormat {
        /// The invalid identifier value.
        value: String,
    },
    /// The venue component is invalid.
    #[error("invalid `OptionSeriesId` value '{value}': invalid venue: {source}")]
    InvalidVenue {
        /// The invalid identifier value.
        value: String,
        /// The venue validation failure.
        source: Box<CorrectnessError>,
    },
    /// The underlying reference instrument is invalid.
    #[error(
        "invalid `OptionSeriesId` value '{value}': invalid underlying instrument '{instrument}': {reason}"
    )]
    InvalidUnderlyingInstrument {
        /// The invalid identifier value.
        value: String,
        /// The invalid underlying instrument component.
        instrument: String,
        /// The instrument validation failure.
        reason: String,
    },
    /// The expiration component is invalid.
    #[error(
        "invalid `OptionSeriesId` value '{value}': invalid expiration '{expiration}': {reason}"
    )]
    InvalidExpiration {
        /// The invalid identifier value.
        value: String,
        /// The invalid expiration component.
        expiration: String,
        /// The expiration validation failure.
        reason: String,
    },
}

impl OptionSeriesId {
    /// Creates an option series with an explicit reference instrument.
    #[must_use]
    pub fn new(
        venue: Venue,
        underlying: Ustr,
        settlement_currency: Ustr,
        expiration_ns: UnixNanos,
        underlying_instrument_id: InstrumentId,
    ) -> Self {
        Self {
            venue,
            underlying,
            settlement_currency,
            expiration_ns,
            underlying_instrument_id,
        }
    }

    /// Creates a series with the legacy `<UNDERLYING>.<VENUE>` reference instrument.
    ///
    /// # Panics
    ///
    /// Panics if `underlying` is empty or whitespace-only.
    #[must_use]
    pub fn new_derived(
        venue: Venue,
        underlying: Ustr,
        settlement_currency: Ustr,
        expiration_ns: UnixNanos,
    ) -> Self {
        Self::new(
            venue,
            underlying,
            settlement_currency,
            expiration_ns,
            InstrumentId::new(Symbol::new(underlying.as_str()), venue),
        )
    }

    /// Creates a series from a date string and an optional typed reference instrument.
    ///
    /// The date accepts `YYYY-MM-DD`, RFC 3339, integer nanoseconds, or floating-point seconds.
    /// An absent reference derives `<UNDERLYING>.<VENUE>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the venue, derived underlying instrument, or expiration is invalid.
    pub fn from_expiry(
        venue: &str,
        underlying: &str,
        settlement_currency: &str,
        date_str: &str,
        underlying_instrument_id: Option<InstrumentId>,
    ) -> Result<Self, OptionSeriesIdError> {
        let value = format!("{venue}:{underlying}:{settlement_currency}:{date_str}");

        let venue =
            Venue::new_checked(venue).map_err(|source| OptionSeriesIdError::InvalidVenue {
                value: value.clone(),
                source: Box::new(source),
            })?;
        let expiration_ns =
            UnixNanos::from_str(date_str).map_err(|e| OptionSeriesIdError::InvalidExpiration {
                value: value.clone(),
                expiration: date_str.to_string(),
                reason: e.to_string(),
            })?;

        let instrument = match underlying_instrument_id {
            Some(instrument) => instrument,
            None => derived_instrument(venue, underlying, &value)?,
        };

        Ok(Self::new(
            venue,
            Ustr::from(underlying),
            Ustr::from(settlement_currency),
            expiration_ns,
            instrument,
        ))
    }

    /// Creates a series from an expiration timestamp and an optional typed reference instrument.
    ///
    /// An absent reference derives `<UNDERLYING>.<VENUE>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the venue or derived underlying instrument is invalid.
    pub fn from_expiry_ns(
        venue: &str,
        underlying: &str,
        settlement_currency: &str,
        expiration_ns: UnixNanos,
        underlying_instrument_id: Option<InstrumentId>,
    ) -> Result<Self, OptionSeriesIdError> {
        let value = format!("{venue}:{underlying}:{settlement_currency}:{expiration_ns}");

        let venue =
            Venue::new_checked(venue).map_err(|source| OptionSeriesIdError::InvalidVenue {
                value: value.clone(),
                source: Box::new(source),
            })?;

        let instrument = match underlying_instrument_id {
            Some(instrument) => instrument,
            None => derived_instrument(venue, underlying, &value)?,
        };

        Ok(Self::new(
            venue,
            Ustr::from(underlying),
            Ustr::from(settlement_currency),
            expiration_ns,
            instrument,
        ))
    }

    /// Returns the canonical wire representation with exact nanosecond expiry.
    #[must_use]
    pub fn to_wire_string(&self) -> String {
        if self.is_derived() {
            format!(
                "{}:{}:{}:{}",
                self.venue, self.underlying, self.settlement_currency, self.expiration_ns
            )
        } else {
            format!(
                "{}:{}:{}:{}:{}",
                self.venue,
                self.underlying,
                self.underlying_instrument_id,
                self.settlement_currency,
                self.expiration_ns
            )
        }
    }

    /// Creates a series from a crypto option and its explicit reference instrument.
    #[must_use]
    pub fn from_crypto_option(
        option: &CryptoOption,
        underlying_instrument_id: InstrumentId,
    ) -> Self {
        Self::new(
            option.id.venue,
            option.underlying.code,
            option.settlement_currency.code,
            option.expiration_ns,
            underlying_instrument_id,
        )
    }

    /// Returns whether the reference is `<UNDERLYING>.<VENUE>`.
    #[must_use]
    pub fn is_derived(&self) -> bool {
        self.underlying_instrument_id.venue == self.venue
            && self.underlying_instrument_id.symbol.as_str() == self.underlying.as_str()
    }
}

impl Display for OptionSeriesId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:", self.venue, self.underlying)?;

        if !self.is_derived() {
            write!(f, "{}:", self.underlying_instrument_id)?;
        }

        write!(
            f,
            "{}:{}",
            self.settlement_currency,
            self.expiration_ns.to_datetime_utc()
        )
    }
}

impl Debug for OptionSeriesId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{self}\"")
    }
}

impl FromStr for OptionSeriesId {
    type Err = OptionSeriesIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(3, ':');

        let (Some(venue), Some(underlying), Some(rest)) =
            (parts.next(), parts.next(), parts.next())
        else {
            return Err(OptionSeriesIdError::InvalidFormat {
                value: s.to_string(),
            });
        };

        let venue =
            Venue::new_checked(venue).map_err(|source| OptionSeriesIdError::InvalidVenue {
                value: s.to_string(),
                source: Box::new(source),
            })?;

        let (instrument, settlement, expiration) = parse_reference(rest);

        let instrument = match instrument {
            Some(instrument) => InstrumentId::from_str(instrument).map_err(|e| {
                OptionSeriesIdError::InvalidUnderlyingInstrument {
                    value: s.to_string(),
                    instrument: instrument.to_string(),
                    reason: e.to_string(),
                }
            })?,
            None => derived_instrument(venue, underlying, s)?,
        };

        let (Some(settlement), Some(expiration)) = (settlement, expiration) else {
            return Err(OptionSeriesIdError::InvalidFormat {
                value: s.to_string(),
            });
        };

        let expiration_ns = UnixNanos::from_str(expiration).map_err(|e| {
            OptionSeriesIdError::InvalidExpiration {
                value: s.to_string(),
                expiration: expiration.to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok(Self::new(
            venue,
            Ustr::from(underlying),
            Ustr::from(settlement),
            expiration_ns,
            instrument,
        ))
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
        let s: std::borrow::Cow<'de, str> = Deserialize::deserialize(deserializer)?;
        Self::from_str(s.as_ref()).map_err(serde::de::Error::custom)
    }
}

fn parse_reference(rest: &str) -> (Option<&str>, Option<&str>, Option<&str>) {
    let legacy = rest.split_once(':');
    if let Some((settlement, expiration)) = legacy
        && UnixNanos::from_str(expiration).is_ok()
    {
        return (None, Some(settlement), Some(expiration));
    }

    let mut extended = None;

    for (index, _) in rest.match_indices(':') {
        let reference = &rest[..index];
        if !reference.contains('.') {
            continue;
        }

        let Some((settlement, expiration)) = rest[index + 1..].split_once(':') else {
            continue;
        };

        extended = Some((Some(reference), Some(settlement), Some(expiration)));
        if UnixNanos::from_str(expiration).is_ok() {
            break;
        }
    }

    extended.unwrap_or(match legacy {
        Some((settlement, expiration)) => (None, Some(settlement), Some(expiration)),
        None => (None, None, None),
    })
}

fn derived_instrument(
    venue: Venue,
    underlying: &str,
    value: &str,
) -> Result<InstrumentId, OptionSeriesIdError> {
    let symbol = Symbol::new_checked(underlying).map_err(|e| {
        OptionSeriesIdError::InvalidUnderlyingInstrument {
            value: value.to_string(),
            instrument: format!("{underlying}.{venue}"),
            reason: e.to_string(),
        }
    })?;

    Ok(InstrumentId::new(symbol, venue))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rstest::*;

    use super::*;
    use crate::{instruments::stubs::crypto_option_btc_deribit, types::Currency};

    fn test_series_id() -> OptionSeriesId {
        OptionSeriesId::new_derived(
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

        let id = OptionSeriesId::new_derived(venue, underlying, settlement, expiration_ns);

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
        let error = OptionSeriesId::from_str("DERIBIT:BTC:BTC").unwrap_err();

        assert_eq!(
            error,
            OptionSeriesIdError::InvalidFormat {
                value: "DERIBIT:BTC:BTC".to_string(),
            },
        );
        assert_eq!(
            error.to_string(),
            "invalid `OptionSeriesId` value 'DERIBIT:BTC:BTC': expected format 'VENUE:UNDERLYING:[REFERENCE:]SETTLEMENT:EXPIRY'",
        );
    }

    #[rstest]
    fn test_option_series_id_from_str_invalid_venue() {
        let error = OptionSeriesId::from_str("DÉRIBIT:BTC:BTC:1700000000000000000").unwrap_err();

        assert_eq!(
            error,
            OptionSeriesIdError::InvalidVenue {
                value: "DÉRIBIT:BTC:BTC:1700000000000000000".to_string(),
                source: Box::new(CorrectnessError::NonAsciiString {
                    param: "value".to_string(),
                    value: "DÉRIBIT".to_string(),
                }),
            },
        );
        assert_eq!(
            error.to_string(),
            concat!(
                "invalid `OptionSeriesId` value 'DÉRIBIT:BTC:BTC:1700000000000000000': ",
                "invalid venue: invalid string for 'value' contained a non-ASCII char, ",
                "was 'DÉRIBIT'",
            ),
        );
    }

    #[rstest]
    fn test_option_series_id_from_str_invalid_expiry() {
        let error = OptionSeriesId::from_str("DERIBIT:BTC:BTC:not_a_date").unwrap_err();

        assert_eq!(
            error,
            OptionSeriesIdError::InvalidExpiration {
                value: "DERIBIT:BTC:BTC:not_a_date".to_string(),
                expiration: "not_a_date".to_string(),
                reason: "Invalid format: not_a_date".to_string(),
            },
        );
        assert_eq!(
            error.to_string(),
            concat!(
                "invalid `OptionSeriesId` value 'DERIBIT:BTC:BTC:not_a_date': ",
                "invalid expiration 'not_a_date': Invalid format: not_a_date",
            ),
        );
    }

    #[rstest]
    fn test_option_series_id_inequality() {
        let id1 = test_series_id();
        let id2 = OptionSeriesId::new_derived(
            Venue::new("DERIBIT"),
            Ustr::from("ETH"),
            Ustr::from("ETH"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        );
        assert_ne!(id1, id2);
    }

    #[rstest]
    fn test_option_series_id_hash() {
        let id1 = test_series_id();
        let id2 = OptionSeriesId::new_derived(
            Venue::new("DERIBIT"),
            Ustr::from("ETH"),
            Ustr::from("ETH"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        );

        let mut set = HashSet::new();
        set.insert(id1);
        set.insert(id2);
        set.insert(id1);

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
    fn test_option_series_id_deserialize_from_owned_value() {
        let id = test_series_id();
        let value = serde_json::Value::String(id.to_wire_string());

        let deserialized: OptionSeriesId = serde_json::from_value(value).unwrap();
        assert_eq!(id, deserialized);
    }

    #[rstest]
    fn test_from_expiry_happy_path() {
        let id = OptionSeriesId::from_expiry("DERIBIT", "BTC", "BTC", "2025-03-28", None).unwrap();
        assert_eq!(id.venue, Venue::new("DERIBIT"));
        assert_eq!(id.underlying, Ustr::from("BTC"));
        assert_eq!(id.settlement_currency, Ustr::from("BTC"));
        assert_eq!(
            id.expiration_ns,
            UnixNanos::from(1_743_120_000_000_000_000u64)
        );
    }

    #[rstest]
    fn test_from_expiry_invalid_date() {
        let result = OptionSeriesId::from_expiry("DERIBIT", "BTC", "BTC", "not-a-date", None);
        let error = result.unwrap_err();

        assert_eq!(
            error,
            OptionSeriesIdError::InvalidExpiration {
                value: "DERIBIT:BTC:BTC:not-a-date".to_string(),
                expiration: "not-a-date".to_string(),
                reason: "Invalid format: not-a-date".to_string(),
            },
        );
    }

    #[rstest]
    fn test_from_expiry_invalid_venue() {
        let error =
            OptionSeriesId::from_expiry("DÉRIBIT", "BTC", "BTC", "2025-03-28", None).unwrap_err();

        assert_eq!(
            error,
            OptionSeriesIdError::InvalidVenue {
                value: "DÉRIBIT:BTC:BTC:2025-03-28".to_string(),
                source: Box::new(CorrectnessError::NonAsciiString {
                    param: "value".to_string(),
                    value: "DÉRIBIT".to_string(),
                }),
            },
        );
    }

    #[rstest]
    fn test_from_expiry_roundtrip() {
        let id = OptionSeriesId::from_expiry("DERIBIT", "ETH", "ETH", "2025-06-27", None).unwrap();
        let s = id.to_string();
        let parsed = OptionSeriesId::from_str(&s).unwrap();
        assert_eq!(id, parsed);
    }

    #[rstest]
    fn test_from_crypto_option(mut crypto_option_btc_deribit: CryptoOption) {
        crypto_option_btc_deribit.settlement_currency = Currency::USDC();

        let id = OptionSeriesId::from_crypto_option(
            &crypto_option_btc_deribit,
            InstrumentId::from("BTC.DERIBIT"),
        );

        assert_eq!(id.venue, Venue::new("DERIBIT"));
        assert_eq!(id.underlying, Ustr::from("BTC"));
        assert_eq!(id.settlement_currency, Ustr::from("USDC"));
        assert_eq!(
            id.expiration_ns,
            UnixNanos::from(1_673_596_800_000_000_000u64)
        );
    }

    #[rstest]
    fn test_from_expiry_ns_happy_path() {
        let id = OptionSeriesId::from_expiry_ns(
            "DERIBIT",
            "ETH",
            "USDC",
            UnixNanos::from(1_700_000_000_000_000_000u64),
            None,
        )
        .unwrap();

        assert_eq!(id.venue, Venue::new("DERIBIT"));
        assert_eq!(id.underlying, Ustr::from("ETH"));
        assert_eq!(id.settlement_currency, Ustr::from("USDC"));
        assert_eq!(
            id.expiration_ns,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
    }

    #[rstest]
    fn test_from_expiry_ns_empty_venue() {
        let error = OptionSeriesId::from_expiry_ns(
            "",
            "ETH",
            "USDC",
            UnixNanos::from(1_700_000_000_000_000_000u64),
            None,
        )
        .unwrap_err();

        assert_eq!(
            error,
            OptionSeriesIdError::InvalidVenue {
                value: ":ETH:USDC:1700000000000000000".to_string(),
                source: Box::new(CorrectnessError::EmptyString {
                    param: "value".to_string(),
                }),
            },
        );
        assert_eq!(
            error.to_string(),
            concat!(
                "invalid `OptionSeriesId` value ':ETH:USDC:1700000000000000000': ",
                "invalid venue: invalid string for 'value', was empty",
            ),
        );
    }

    #[rstest]
    fn test_from_expiry_ns_non_ascii_venue() {
        let error = OptionSeriesId::from_expiry_ns(
            "DÉRIBIT",
            "ETH",
            "USDC",
            UnixNanos::from(1_700_000_000_000_000_000u64),
            None,
        )
        .unwrap_err();

        assert_eq!(
            error,
            OptionSeriesIdError::InvalidVenue {
                value: "DÉRIBIT:ETH:USDC:1700000000000000000".to_string(),
                source: Box::new(CorrectnessError::NonAsciiString {
                    param: "value".to_string(),
                    value: "DÉRIBIT".to_string(),
                }),
            },
        );
    }

    #[rstest]
    fn test_from_expiry_ns_rejects_empty_derived_underlying() {
        let id = OptionSeriesId::from_expiry_ns(
            "DERIBIT",
            "",
            "",
            UnixNanos::from(1_700_000_000_000_000_000u64),
            None,
        )
        .unwrap_err();

        assert_eq!(
            id,
            OptionSeriesIdError::InvalidUnderlyingInstrument {
                value: "DERIBIT:::1700000000000000000".to_string(),
                instrument: ".DERIBIT".to_string(),
                reason: "invalid string for 'value', was empty".to_string(),
            }
        );
    }

    #[rstest]
    #[case("BTC.DERIBIT", "USD", "DERIBIT:BTC:USD:2023-11-14T22:13:20.123456789Z")]
    #[case(
        "BTCUSDT.BINANCE",
        "USD",
        "DERIBIT:BTC:BTCUSDT.BINANCE:USD:2023-11-14T22:13:20.123456789Z"
    )]
    #[case(
        "xyz:TSLA-USD-PERP.HYPERLIQUID",
        "USD",
        "DERIBIT:BTC:xyz:TSLA-USD-PERP.HYPERLIQUID:USD:2023-11-14T22:13:20.123456789Z"
    )]
    #[case(
        "UD:2E: SG 2500275.GLBX",
        "USD",
        "DERIBIT:BTC:UD:2E: SG 2500275.GLBX:USD:2023-11-14T22:13:20.123456789Z"
    )]
    #[case(
        "BTC.DERIBIT",
        "USDC.e",
        "DERIBIT:BTC:USDC.e:2023-11-14T22:13:20.123456789Z"
    )]
    #[case(
        "BTCUSDT.BINANCE",
        "USDC.e",
        "DERIBIT:BTC:BTCUSDT.BINANCE:USDC.e:2023-11-14T22:13:20.123456789Z"
    )]
    #[case(
        "xyz:TSLA-USD-PERP.HYPERLIQUID",
        "USDC.e",
        "DERIBIT:BTC:xyz:TSLA-USD-PERP.HYPERLIQUID:USDC.e:2023-11-14T22:13:20.123456789Z"
    )]
    fn test_reference_roundtrip(
        #[case] reference: &str,
        #[case] settlement: &str,
        #[case] expected: &str,
    ) {
        let instrument = InstrumentId::from(reference);

        let id = OptionSeriesId::new(
            Venue::from("DERIBIT"),
            Ustr::from("BTC"),
            Ustr::from(settlement),
            UnixNanos::from(1_700_000_000_123_456_789u64),
            instrument,
        );
        let display = id.to_string();
        let wire = id.to_wire_string();
        let json = serde_json::to_value(id).unwrap();
        assert_eq!(display, expected);
        assert_eq!(OptionSeriesId::from_str(&display).unwrap(), id);
        assert_eq!(OptionSeriesId::from_str(&wire).unwrap(), id);
        assert_eq!(serde_json::from_value::<OptionSeriesId>(json).unwrap(), id);
        assert_eq!(id.underlying_instrument_id, instrument);
    }

    #[rstest]
    fn test_reference_participates_in_identity() {
        let first = OptionSeriesId::new(
            Venue::from("XCME"),
            Ustr::from("ES"),
            Ustr::from("USD"),
            UnixNanos::from(17u64),
            InstrumentId::from("ESH6.XCME"),
        );

        let second = OptionSeriesId {
            underlying_instrument_id: InstrumentId::from("ESM6.XCME"),
            ..first
        };

        let mut ids = HashSet::new();
        ids.insert(first);
        ids.insert(second);
        assert_ne!(first, second);
        assert_ne!(first.cmp(&second), std::cmp::Ordering::Equal);
        assert_eq!(ids.len(), 2);
        assert_eq!(first.to_wire_string(), "XCME:ES:ESH6.XCME:USD:17");
    }

    #[rstest]
    #[case("DERIBIT::USD:1700000000000000000")]
    #[case("DERIBIT:   :USD:1700000000000000000")]
    #[case("DERIBIT:BTC:.BINANCE:USD:1700000000000000000")]
    fn test_invalid_reference_returns_typed_error(#[case] value: &str) {
        let error = OptionSeriesId::from_str(value).unwrap_err();
        assert!(matches!(
            error,
            OptionSeriesIdError::InvalidUnderlyingInstrument { .. }
        ));
        assert!(
            serde_json::from_value::<OptionSeriesId>(serde_json::Value::String(value.to_string()))
                .is_err()
        );
    }

    #[rstest]
    fn test_explicit_reference_constructors() {
        let reference = InstrumentId::from("BTCUSDT.BINANCE");
        let id =
            OptionSeriesId::from_expiry("DERIBIT", "BTC", "USD", "2026-03-20", Some(reference))
                .unwrap();
        let ns = OptionSeriesId::from_expiry_ns(
            "DERIBIT",
            "BTC",
            "USD",
            id.expiration_ns,
            Some(reference),
        )
        .unwrap();
        assert_eq!(id, ns);
        assert_eq!(id.venue, Venue::from("DERIBIT"));
        assert_eq!(id.underlying, Ustr::from("BTC"));
        assert_eq!(id.settlement_currency, Ustr::from("USD"));
        assert_eq!(id.expiration_ns, UnixNanos::from_str("2026-03-20").unwrap());
        assert_eq!(id.underlying_instrument_id, reference);
    }
}
