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

//! Common parsing utilities for the Coinbase adapter.

use std::str::FromStr;

use nautilus_core::UnixNanos;
pub use nautilus_core::serialization::{
    deserialize_decimal_from_str, deserialize_decimal_or_zero,
    deserialize_optional_decimal_from_str, deserialize_string_to_u64, serialize_decimal_as_str,
    serialize_optional_decimal_as_str,
};
use nautilus_model::{
    data::BarType,
    enums::{AggregationSource, BarAggregation},
};
use serde::{
    Deserialize,
    de::{self, Unexpected},
};

use crate::common::enums::{CoinbaseGranularity, CoinbaseMarginType, CoinbaseProductType};

/// Deserializes an optional value where Coinbase uses an empty string for `None`.
pub fn deserialize_empty_string_to_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum EmptyOrValue<T> {
        Value(T),
        Empty(String),
    }

    match Option::<EmptyOrValue<T>>::deserialize(deserializer)? {
        None => Ok(None),
        Some(EmptyOrValue::Value(value)) => Ok(Some(value)),
        Some(EmptyOrValue::Empty(value)) if value.is_empty() => Ok(None),
        Some(EmptyOrValue::Empty(value)) => Err(de::Error::invalid_value(
            Unexpected::Str(&value),
            &"an empty string or a valid value",
        )),
    }
}

/// Deserializes a Coinbase product type and falls back to `Unknown`.
pub fn deserialize_product_type_or_unknown<'de, D>(
    deserializer: D,
) -> Result<CoinbaseProductType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    Ok(CoinbaseProductType::from_str(&value).unwrap_or(CoinbaseProductType::Unknown))
}

/// Deserializes the optional `margin_type` field on historical orders.
///
/// Coinbase returns one of `""`, `"UNKNOWN_MARGIN_TYPE"`, `"CROSS"`, or
/// `"ISOLATED"` here. The first two carry no information (spot orders, or
/// futures orders the venue declines to classify), so they map to `None`.
/// Unrecognized values also map to `None` so a future enum variant cannot
/// fail an entire historical-orders batch.
pub fn deserialize_margin_type_or_none<'de, D>(
    deserializer: D,
) -> Result<Option<CoinbaseMarginType>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value
        .filter(|s| !s.is_empty())
        .and_then(|s| CoinbaseMarginType::from_str(&s).ok()))
}

/// Converts a [`UnixNanos`] timestamp to an RFC 3339 string in UTC.
///
/// # Errors
///
/// Returns an error when the nanosecond value is outside the range
/// representable by [`chrono::DateTime::<chrono::Utc>::from_timestamp`].
pub fn format_rfc3339_from_nanos(ts: UnixNanos) -> anyhow::Result<String> {
    let secs = (ts.as_u64() / 1_000_000_000) as i64;
    let nanos = (ts.as_u64() % 1_000_000_000) as u32;
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| anyhow::anyhow!("UnixNanos {ts} is out of range for chrono::DateTime"))
}

/// Converts a Nautilus [`BarType`] to a [`CoinbaseGranularity`].
///
/// # Errors
///
/// Returns an error if the bar type uses an unsupported aggregation or step value.
pub fn bar_type_to_granularity(bar_type: &BarType) -> anyhow::Result<CoinbaseGranularity> {
    let spec = bar_type.spec();

    anyhow::ensure!(
        bar_type.aggregation_source() == AggregationSource::External,
        "Only EXTERNAL aggregation is supported"
    );

    let step = spec.step.get();

    match spec.aggregation {
        BarAggregation::Minute => match step {
            1 => Ok(CoinbaseGranularity::OneMinute),
            5 => Ok(CoinbaseGranularity::FiveMinute),
            15 => Ok(CoinbaseGranularity::FifteenMinute),
            30 => Ok(CoinbaseGranularity::ThirtyMinute),
            _ => anyhow::bail!("Unsupported minute step: {step}"),
        },
        BarAggregation::Hour => match step {
            1 => Ok(CoinbaseGranularity::OneHour),
            2 => Ok(CoinbaseGranularity::TwoHour),
            6 => Ok(CoinbaseGranularity::SixHour),
            _ => anyhow::bail!("Unsupported hour step: {step}"),
        },
        BarAggregation::Day => match step {
            1 => Ok(CoinbaseGranularity::OneDay),
            _ => anyhow::bail!("Unsupported day step: {step}"),
        },
        other => anyhow::bail!("Unsupported aggregation: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        "BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL",
        CoinbaseGranularity::OneMinute
    )]
    #[case(
        "BTC-USD.COINBASE-5-MINUTE-LAST-EXTERNAL",
        CoinbaseGranularity::FiveMinute
    )]
    #[case(
        "BTC-USD.COINBASE-15-MINUTE-LAST-EXTERNAL",
        CoinbaseGranularity::FifteenMinute
    )]
    #[case(
        "BTC-USD.COINBASE-30-MINUTE-LAST-EXTERNAL",
        CoinbaseGranularity::ThirtyMinute
    )]
    #[case("BTC-USD.COINBASE-1-HOUR-LAST-EXTERNAL", CoinbaseGranularity::OneHour)]
    #[case("BTC-USD.COINBASE-2-HOUR-LAST-EXTERNAL", CoinbaseGranularity::TwoHour)]
    #[case("BTC-USD.COINBASE-6-HOUR-LAST-EXTERNAL", CoinbaseGranularity::SixHour)]
    #[case("BTC-USD.COINBASE-1-DAY-LAST-EXTERNAL", CoinbaseGranularity::OneDay)]
    fn test_bar_type_to_granularity(
        #[case] bar_type_str: &str,
        #[case] expected: CoinbaseGranularity,
    ) {
        let bar_type = BarType::from(bar_type_str);
        let result = bar_type_to_granularity(&bar_type).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case("BTC-USD.COINBASE-3-MINUTE-LAST-EXTERNAL")]
    #[case("BTC-USD.COINBASE-4-HOUR-LAST-EXTERNAL")]
    #[case("BTC-USD.COINBASE-2-DAY-LAST-EXTERNAL")]
    fn test_bar_type_to_granularity_unsupported(#[case] bar_type_str: &str) {
        let bar_type = BarType::from(bar_type_str);
        assert!(bar_type_to_granularity(&bar_type).is_err());
    }

    #[rstest]
    fn test_format_rfc3339_from_nanos_round_trip() {
        // 2024-01-15T10:30:00.000000000Z
        let ts = UnixNanos::from(1_705_314_600_000_000_000u64);
        let s = format_rfc3339_from_nanos(ts).unwrap();
        assert_eq!(s, "2024-01-15T10:30:00+00:00");
    }

    #[rstest]
    fn test_format_rfc3339_from_nanos_preserves_subsecond_precision() {
        // 2024-01-15T10:30:00.123456789Z
        let ts = UnixNanos::from(1_705_314_600_123_456_789u64);
        let s = format_rfc3339_from_nanos(ts).unwrap();
        assert_eq!(s, "2024-01-15T10:30:00.123456789+00:00");
    }
}
