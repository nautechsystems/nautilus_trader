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

//! Bar aggregate structures, data types and functionality.

use std::{
    collections::HashMap,
    fmt::{Debug, Display, Formatter},
    hash::Hash,
    str::FromStr,
};

use chrono::{DateTime, Datelike, Duration, TimeDelta, Timelike, Utc};
use derive_builder::Builder;
use indexmap::IndexMap;
use nautilus_core::{correctness::FAILED, nanos::UnixNanos, serialization::Serializable};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::GetTsInit;
use crate::{
    enums::{AggregationSource, BarAggregation, PriceType},
    identifiers::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

/// Returns the bar interval as a `TimeDelta`.
///
/// # Panics
///
/// This function panics:
/// - If the aggregation method of the given `bar_type` is not time based.
pub fn get_bar_interval(bar_type: &BarType) -> TimeDelta {
    let spec = bar_type.spec();

    match spec.aggregation {
        BarAggregation::Millisecond => TimeDelta::milliseconds(spec.step as i64),
        BarAggregation::Second => TimeDelta::seconds(spec.step as i64),
        BarAggregation::Minute => TimeDelta::minutes(spec.step as i64),
        BarAggregation::Hour => TimeDelta::hours(spec.step as i64),
        BarAggregation::Day => TimeDelta::days(spec.step as i64),
        _ => panic!("Aggregation not time based"),
    }
}

/// Returns the bar interval as `UnixNanos`.
///
/// # Panics
///
/// This function panics:
/// - If the aggregation method of the given `bar_type` is not time based.
pub fn get_bar_interval_ns(bar_type: &BarType) -> UnixNanos {
    let interval_ns = get_bar_interval(bar_type)
        .num_nanoseconds()
        .expect("Invalid bar interval") as u64;
    UnixNanos::from(interval_ns)
}

/// Returns the time bar start as a timezone-aware `DateTime<Utc>`.
pub fn get_time_bar_start(now: DateTime<Utc>, bar_type: &BarType) -> DateTime<Utc> {
    let spec = bar_type.spec();
    let step = spec.step;

    match spec.aggregation {
        BarAggregation::Millisecond => {
            let diff_milliseconds = now.timestamp_subsec_millis() as usize % step;
            now - TimeDelta::milliseconds(diff_milliseconds as i64)
        }
        BarAggregation::Second => {
            let diff_seconds = now.timestamp() % step as i64;
            now - TimeDelta::seconds(diff_seconds)
        }
        BarAggregation::Minute => {
            let diff_minutes = now.time().minute() as usize % step;
            now - TimeDelta::minutes(diff_minutes as i64)
                - TimeDelta::seconds(now.time().second() as i64)
        }
        BarAggregation::Hour => {
            let diff_hours = now.time().hour() as usize % step;
            let diff_days = if diff_hours == 0 { 0 } else { (step / 24) - 1 };
            now - TimeDelta::days(diff_days as i64)
                - TimeDelta::hours(diff_hours as i64)
                - TimeDelta::minutes(now.minute() as i64)
                - TimeDelta::seconds(now.second() as i64)
        }
        BarAggregation::Day => {
            now - TimeDelta::days(now.day() as i64 % step as i64)
                - TimeDelta::hours(now.hour() as i64)
                - TimeDelta::minutes(now.minute() as i64)
                - TimeDelta::seconds(now.second() as i64)
        }
        _ => panic!(
            "Aggregation type {} not supported for time bars",
            spec.aggregation
        ),
    }
}

/// Represents a bar aggregation specification including a step, aggregation
/// method/rule and price type.
#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize, Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[cfg_attr(feature = "trivial_copy", derive(Copy))]
pub struct BarSpecification {
    /// The step for binning samples for bar aggregation.
    pub step: usize,
    /// The type of bar aggregation.
    pub aggregation: BarAggregation,
    /// The price type to use for aggregation.
    pub price_type: PriceType,
}

impl BarSpecification {
    /// Creates a new [`BarSpecification`] instance.
    #[must_use]
    pub fn new(step: usize, aggregation: BarAggregation, price_type: PriceType) -> Self {
        Self {
            step,
            aggregation,
            price_type,
        }
    }

    pub fn timedelta(&self) -> TimeDelta {
        match self.aggregation {
            BarAggregation::Millisecond => Duration::milliseconds(self.step as i64),
            BarAggregation::Second => Duration::seconds(self.step as i64),
            BarAggregation::Minute => Duration::minutes(self.step as i64),
            BarAggregation::Hour => Duration::hours(self.step as i64),
            BarAggregation::Day => Duration::days(self.step as i64),
            _ => panic!(
                "Timedelta not supported for aggregation type: {:?}",
                self.aggregation
            ),
        }
    }

    /// Return a value indicating whether the aggregation method is time-driven:
    ///  - [`BarAggregation::Millisecond`]
    ///  - [`BarAggregation::Second`]
    ///  - [`BarAggregation::Minute`]
    ///  - [`BarAggregation::Hour`]
    ///  - [`BarAggregation::Day`]
    ///  - [`BarAggregation::Month`]
    pub fn is_time_aggregated(&self) -> bool {
        matches!(
            self.aggregation,
            BarAggregation::Millisecond
                | BarAggregation::Second
                | BarAggregation::Minute
                | BarAggregation::Hour
                | BarAggregation::Day
                | BarAggregation::Month
        )
    }

    /// Return a value indicating whether the aggregation method is threshold-driven:
    ///  - [`BarAggregation::Tick`]
    ///  - [`BarAggregation::TickImbalance`]
    ///  - [`BarAggregation::Volume`]
    ///  - [`BarAggregation::VolumeImbalance`]
    ///  - [`BarAggregation::Value`]
    ///  - [`BarAggregation::ValueImbalance`]
    pub fn is_threshold_aggregated(&self) -> bool {
        matches!(
            self.aggregation,
            BarAggregation::Tick
                | BarAggregation::TickImbalance
                | BarAggregation::Volume
                | BarAggregation::VolumeImbalance
                | BarAggregation::Value
                | BarAggregation::ValueImbalance
        )
    }

    /// Return a value indicating whether the aggregation method is information-driven:
    ///  - [`BarAggregation::TickRuns`]
    ///  - [`BarAggregation::VolumeRuns`]
    ///  - [`BarAggregation::ValueRuns`]
    pub fn is_information_aggregated(&self) -> bool {
        matches!(
            self.aggregation,
            BarAggregation::TickRuns | BarAggregation::VolumeRuns | BarAggregation::ValueRuns
        )
    }
}

impl Display for BarSpecification {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}-{}", self.step, self.aggregation, self.price_type)
    }
}

/// Represents a bar type including the instrument ID, bar specification and
/// aggregation source.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub enum BarType {
    Standard {
        /// The bar type's instrument ID.
        instrument_id: InstrumentId,
        /// The bar type's specification.
        spec: BarSpecification,
        /// The bar type's aggregation source.
        aggregation_source: AggregationSource,
    },
    Composite {
        /// The bar type's instrument ID.
        instrument_id: InstrumentId,
        /// The bar type's specification.
        spec: BarSpecification,
        /// The bar type's aggregation source.
        aggregation_source: AggregationSource,

        /// The composite step for binning samples for bar aggregation.
        composite_step: usize,
        /// The composite type of bar aggregation.
        composite_aggregation: BarAggregation,
        /// The composite bar type's aggregation source.
        composite_aggregation_source: AggregationSource,
    },
}

impl BarType {
    /// Creates a new [`BarType`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        spec: BarSpecification,
        aggregation_source: AggregationSource,
    ) -> Self {
        Self::Standard {
            instrument_id,
            spec,
            aggregation_source,
        }
    }

    /// Creates a new composite [`BarType`] instance.
    pub fn new_composite(
        instrument_id: InstrumentId,
        spec: BarSpecification,
        aggregation_source: AggregationSource,

        composite_step: usize,
        composite_aggregation: BarAggregation,
        composite_aggregation_source: AggregationSource,
    ) -> Self {
        Self::Composite {
            instrument_id,
            spec,
            aggregation_source,

            composite_step,
            composite_aggregation,
            composite_aggregation_source,
        }
    }

    /// Returns whether this instance is a standard bar type.
    pub fn is_standard(&self) -> bool {
        match &self {
            BarType::Standard { .. } => true,
            BarType::Composite { .. } => false,
        }
    }

    /// Returns whether this instance is a composite bar type.
    pub fn is_composite(&self) -> bool {
        match &self {
            BarType::Standard { .. } => false,
            BarType::Composite { .. } => true,
        }
    }

    /// Returns the standard bar type component.
    pub fn standard(&self) -> Self {
        match &self {
            &&b @ BarType::Standard { .. } => b,
            BarType::Composite {
                instrument_id,
                spec,
                aggregation_source,
                ..
            } => Self::new(*instrument_id, *spec, *aggregation_source),
        }
    }

    /// Returns any composite bar type component.
    pub fn composite(&self) -> Self {
        match &self {
            &&b @ BarType::Standard { .. } => b, // case shouldn't be used if is_composite is called before
            BarType::Composite {
                instrument_id,
                spec,
                aggregation_source: _,

                composite_step,
                composite_aggregation,
                composite_aggregation_source,
            } => Self::new(
                *instrument_id,
                BarSpecification::new(*composite_step, *composite_aggregation, spec.price_type),
                *composite_aggregation_source,
            ),
        }
    }

    /// Returns the [`InstrumentId`] for this bar type.
    pub fn instrument_id(&self) -> InstrumentId {
        match &self {
            BarType::Standard { instrument_id, .. } | BarType::Composite { instrument_id, .. } => {
                *instrument_id
            }
        }
    }

    /// Returns the [`BarSpecification`] for this bar type.
    pub fn spec(&self) -> BarSpecification {
        match &self {
            BarType::Standard { spec, .. } | BarType::Composite { spec, .. } => *spec,
        }
    }

    /// Returns the [`AggregationSource`] for this bar type.
    pub fn aggregation_source(&self) -> AggregationSource {
        match &self {
            BarType::Standard {
                aggregation_source, ..
            }
            | BarType::Composite {
                aggregation_source, ..
            } => *aggregation_source,
        }
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Error parsing `BarType` from '{input}', invalid token: '{token}' at position {position}")]
pub struct BarTypeParseError {
    input: String,
    token: String,
    position: usize,
}

impl FromStr for BarType {
    type Err = BarTypeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('@').collect();
        let standard = parts[0];
        let composite_str = parts.get(1);

        let pieces: Vec<&str> = standard.rsplitn(5, '-').collect();
        let rev_pieces: Vec<&str> = pieces.into_iter().rev().collect();
        if rev_pieces.len() != 5 {
            return Err(BarTypeParseError {
                input: s.to_string(),
                token: String::new(),
                position: 0,
            });
        }

        let instrument_id =
            InstrumentId::from_str(rev_pieces[0]).map_err(|_| BarTypeParseError {
                input: s.to_string(),
                token: rev_pieces[0].to_string(),
                position: 0,
            })?;

        let step = rev_pieces[1].parse().map_err(|_| BarTypeParseError {
            input: s.to_string(),
            token: rev_pieces[1].to_string(),
            position: 1,
        })?;
        let aggregation =
            BarAggregation::from_str(rev_pieces[2]).map_err(|_| BarTypeParseError {
                input: s.to_string(),
                token: rev_pieces[2].to_string(),
                position: 2,
            })?;
        let price_type = PriceType::from_str(rev_pieces[3]).map_err(|_| BarTypeParseError {
            input: s.to_string(),
            token: rev_pieces[3].to_string(),
            position: 3,
        })?;
        let aggregation_source =
            AggregationSource::from_str(rev_pieces[4]).map_err(|_| BarTypeParseError {
                input: s.to_string(),
                token: rev_pieces[4].to_string(),
                position: 4,
            })?;

        if let Some(composite_str) = composite_str {
            let composite_pieces: Vec<&str> = composite_str.rsplitn(3, '-').collect();
            let rev_composite_pieces: Vec<&str> = composite_pieces.into_iter().rev().collect();
            if rev_composite_pieces.len() != 3 {
                return Err(BarTypeParseError {
                    input: s.to_string(),
                    token: String::new(),
                    position: 5,
                });
            }

            let composite_step =
                rev_composite_pieces[0]
                    .parse()
                    .map_err(|_| BarTypeParseError {
                        input: s.to_string(),
                        token: rev_composite_pieces[0].to_string(),
                        position: 5,
                    })?;
            let composite_aggregation =
                BarAggregation::from_str(rev_composite_pieces[1]).map_err(|_| {
                    BarTypeParseError {
                        input: s.to_string(),
                        token: rev_composite_pieces[1].to_string(),
                        position: 6,
                    }
                })?;
            let composite_aggregation_source = AggregationSource::from_str(rev_composite_pieces[2])
                .map_err(|_| BarTypeParseError {
                    input: s.to_string(),
                    token: rev_composite_pieces[2].to_string(),
                    position: 7,
                })?;

            Ok(Self::new_composite(
                instrument_id,
                BarSpecification {
                    step,
                    aggregation,
                    price_type,
                },
                aggregation_source,
                composite_step,
                composite_aggregation,
                composite_aggregation_source,
            ))
        } else {
            Ok(Self::Standard {
                instrument_id,
                spec: BarSpecification {
                    step,
                    aggregation,
                    price_type,
                },
                aggregation_source,
            })
        }
    }
}

impl From<&str> for BarType {
    fn from(value: &str) -> Self {
        Self::from_str(value).expect(FAILED)
    }
}

impl Display for BarType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            BarType::Standard {
                instrument_id,
                spec,
                aggregation_source,
            } => {
                write!(f, "{}-{}-{}", instrument_id, spec, aggregation_source)
            }
            BarType::Composite {
                instrument_id,
                spec,
                aggregation_source,

                composite_step,
                composite_aggregation,
                composite_aggregation_source,
            } => {
                write!(
                    f,
                    "{}-{}-{}@{}-{}-{}",
                    instrument_id,
                    spec,
                    aggregation_source,
                    *composite_step,
                    *composite_aggregation,
                    *composite_aggregation_source
                )
            }
        }
    }
}

impl Serialize for BarType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for BarType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// Represents an aggregated bar.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Bar {
    /// The bar type for this bar.
    pub bar_type: BarType,
    /// The bars open price.
    pub open: Price,
    /// The bars high price.
    pub high: Price,
    /// The bars low price.
    pub low: Price,
    /// The bars close price.
    pub close: Price,
    /// The bars volume.
    pub volume: Quantity,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the struct was initialized.
    pub ts_init: UnixNanos,
}

impl Bar {
    /// Creates a new [`Bar`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bar_type: BarType,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            bar_type,
            open,
            high,
            low,
            close,
            volume,
            ts_event,
            ts_init,
        }
    }

    pub fn instrument_id(&self) -> InstrumentId {
        self.bar_type.instrument_id()
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(
        bar_type: &BarType,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        let instrument_id = bar_type.instrument_id();
        metadata.insert("bar_type".to_string(), bar_type.to_string());
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), price_precision.to_string());
        metadata.insert("size_precision".to_string(), size_precision.to_string());
        metadata
    }

    /// Returns the field map for the type, for use with Arrow schemas.
    #[must_use]
    pub fn get_fields() -> IndexMap<String, String> {
        let mut metadata = IndexMap::new();
        metadata.insert("open".to_string(), "Int64".to_string());
        metadata.insert("high".to_string(), "Int64".to_string());
        metadata.insert("low".to_string(), "Int64".to_string());
        metadata.insert("close".to_string(), "Int64".to_string());
        metadata.insert("volume".to_string(), "UInt64".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }
}

impl Display for Bar {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{}",
            self.bar_type, self.open, self.high, self.low, self.close, self.volume, self.ts_event
        )
    }
}

impl Serializable for Bar {}

impl GetTsInit for Bar {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use rstest::rstest;

    use super::*;
    use crate::identifiers::{Symbol, Venue};

    #[rstest]
    #[case(BarAggregation::Millisecond, 1, TimeDelta::milliseconds(1))]
    #[case(BarAggregation::Millisecond, 10, TimeDelta::milliseconds(10))]
    #[case(BarAggregation::Second, 1, TimeDelta::seconds(1))]
    #[case(BarAggregation::Second, 15, TimeDelta::seconds(15))]
    #[case(BarAggregation::Minute, 1, TimeDelta::minutes(1))]
    #[case(BarAggregation::Minute, 60, TimeDelta::minutes(60))]
    #[case(BarAggregation::Hour, 1, TimeDelta::hours(1))]
    #[case(BarAggregation::Hour, 4, TimeDelta::hours(4))]
    #[case(BarAggregation::Day, 1, TimeDelta::days(1))]
    #[case(BarAggregation::Day, 2, TimeDelta::days(2))]
    #[should_panic(expected = "Aggregation not time based")]
    #[case(BarAggregation::Tick, 1, TimeDelta::zero())]
    fn test_get_bar_interval(
        #[case] aggregation: BarAggregation,
        #[case] step: usize,
        #[case] expected: TimeDelta,
    ) {
        let bar_type = BarType::Standard {
            instrument_id: InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            spec: BarSpecification {
                step,
                aggregation,
                price_type: PriceType::Last,
            },
            aggregation_source: AggregationSource::Internal,
        };

        let interval = get_bar_interval(&bar_type);
        assert_eq!(interval, expected);
    }

    #[rstest]
    #[case(BarAggregation::Millisecond, 1, UnixNanos::from(1_000_000))]
    #[case(BarAggregation::Millisecond, 10, UnixNanos::from(10_000_000))]
    #[case(BarAggregation::Second, 1, UnixNanos::from(1_000_000_000))]
    #[case(BarAggregation::Second, 10, UnixNanos::from(10_000_000_000))]
    #[case(BarAggregation::Minute, 1, UnixNanos::from(60_000_000_000))]
    #[case(BarAggregation::Minute, 60, UnixNanos::from(3_600_000_000_000))]
    #[case(BarAggregation::Hour, 1, UnixNanos::from(3_600_000_000_000))]
    #[case(BarAggregation::Hour, 4, UnixNanos::from(14_400_000_000_000))]
    #[case(BarAggregation::Day, 1, UnixNanos::from(86_400_000_000_000))]
    #[case(BarAggregation::Day, 2, UnixNanos::from(172_800_000_000_000))]
    #[should_panic(expected = "Aggregation not time based")]
    #[case(BarAggregation::Tick, 1, UnixNanos::from(0))]
    fn test_get_bar_interval_ns(
        #[case] aggregation: BarAggregation,
        #[case] step: usize,
        #[case] expected: UnixNanos,
    ) {
        let bar_type = BarType::Standard {
            instrument_id: InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            spec: BarSpecification {
                step,
                aggregation,
                price_type: PriceType::Last,
            },
            aggregation_source: AggregationSource::Internal,
        };

        let interval_ns = get_bar_interval_ns(&bar_type);
        assert_eq!(interval_ns, expected);
    }

    #[rstest]
    #[case::millisecond(
    Utc.timestamp_opt(1658349296, 123_000_000).unwrap(), // 2024-07-21 12:34:56.123 UTC
    BarAggregation::Millisecond,
    1,
    Utc.timestamp_opt(1658349296, 123_000_000).unwrap(),  // 2024-07-21 12:34:56.123 UTC
    )]
    #[rstest]
    #[case::millisecond(
    Utc.timestamp_opt(1658349296, 123_000_000).unwrap(), // 2024-07-21 12:34:56.123 UTC
    BarAggregation::Millisecond,
    10,
    Utc.timestamp_opt(1658349296, 120_000_000).unwrap(),  // 2024-07-21 12:34:56.120 UTC
    )]
    #[case::second(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Millisecond,
    1000,
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap()
    )]
    #[case::second(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Second,
    1,
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap()
    )]
    #[case::second(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Second,
    5,
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 55).unwrap()
    )]
    #[case::second(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Second,
    60,
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 0).unwrap()
    )]
    #[case::minute(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Minute,
    1,
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 0).unwrap()
    )]
    #[case::minute(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Minute,
    5,
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 30, 0).unwrap()
    )]
    #[case::minute(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Minute,
    60,
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 0, 0).unwrap()
    )]
    #[case::hour(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Hour,
    1,
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 0, 0).unwrap()
    )]
    #[case::hour(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Hour,
    2,
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 0, 0).unwrap()
    )]
    #[case::day(
    Utc.with_ymd_and_hms(2024, 7, 21, 12, 34, 56).unwrap(),
    BarAggregation::Day,
    1,
    Utc.with_ymd_and_hms(2024, 7, 21, 0, 0, 0).unwrap()
    )]
    fn test_get_time_bar_start(
        #[case] now: DateTime<Utc>,
        #[case] aggregation: BarAggregation,
        #[case] step: usize,
        #[case] expected: DateTime<Utc>,
    ) {
        let bar_type = BarType::Standard {
            instrument_id: InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            spec: BarSpecification {
                step,
                aggregation,
                price_type: PriceType::Last,
            },
            aggregation_source: AggregationSource::Internal,
        };

        let start_time = get_time_bar_start(now, &bar_type);
        assert_eq!(start_time, expected);
    }

    #[rstest]
    fn test_bar_spec_string_reprs() {
        let bar_spec = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        assert_eq!(bar_spec.to_string(), "1-MINUTE-BID");
        assert_eq!(format!("{bar_spec}"), "1-MINUTE-BID");
    }

    #[rstest]
    fn test_bar_type_parse_valid() {
        let input = "BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL";
        let bar_type = BarType::from(input);

        assert_eq!(
            bar_type.instrument_id(),
            InstrumentId::from("BTCUSDT-PERP.BINANCE")
        );
        assert_eq!(
            bar_type.spec(),
            BarSpecification {
                step: 1,
                aggregation: BarAggregation::Minute,
                price_type: PriceType::Last,
            }
        );
        assert_eq!(bar_type.aggregation_source(), AggregationSource::External);
        assert_eq!(bar_type, BarType::from(input));
    }

    #[rstest]
    fn test_bar_type_composite_parse_valid() {
        let input = "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL";
        let bar_type = BarType::from(input);
        let standard = bar_type.standard();

        assert_eq!(
            bar_type.instrument_id(),
            InstrumentId::from("BTCUSDT-PERP.BINANCE")
        );
        assert_eq!(
            bar_type.spec(),
            BarSpecification {
                step: 2,
                aggregation: BarAggregation::Minute,
                price_type: PriceType::Last,
            }
        );
        assert_eq!(bar_type.aggregation_source(), AggregationSource::Internal);
        assert_eq!(bar_type, BarType::from(input));
        assert!(bar_type.is_composite());

        assert_eq!(
            standard.instrument_id(),
            InstrumentId::from("BTCUSDT-PERP.BINANCE")
        );
        assert_eq!(
            standard.spec(),
            BarSpecification {
                step: 2,
                aggregation: BarAggregation::Minute,
                price_type: PriceType::Last,
            }
        );
        assert_eq!(standard.aggregation_source(), AggregationSource::Internal);
        assert!(standard.is_standard());

        let composite = bar_type.composite();
        let composite_input = "BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL";

        assert_eq!(
            composite.instrument_id(),
            InstrumentId::from("BTCUSDT-PERP.BINANCE")
        );
        assert_eq!(
            composite.spec(),
            BarSpecification {
                step: 1,
                aggregation: BarAggregation::Minute,
                price_type: PriceType::Last,
            }
        );
        assert_eq!(composite.aggregation_source(), AggregationSource::External);
        assert_eq!(composite, BarType::from(composite_input));
        assert!(composite.is_standard());
    }

    #[rstest]
    fn test_bar_type_parse_invalid_token_pos_0() {
        let input = "BTCUSDT-PERP-1-MINUTE-LAST-INTERNAL";
        let result = BarType::from_str(input);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!("Error parsing `BarType` from '{input}', invalid token: 'BTCUSDT-PERP' at position 0")
        );
    }

    #[rstest]
    fn test_bar_type_parse_invalid_token_pos_1() {
        let input = "BTCUSDT-PERP.BINANCE-INVALID-MINUTE-LAST-INTERNAL";
        let result = BarType::from_str(input);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 1"
            )
        );
    }

    #[rstest]
    fn test_bar_type_parse_invalid_token_pos_2() {
        let input = "BTCUSDT-PERP.BINANCE-1-INVALID-LAST-INTERNAL";
        let result = BarType::from_str(input);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 2"
            )
        );
    }

    #[rstest]
    fn test_bar_type_parse_invalid_token_pos_3() {
        let input = "BTCUSDT-PERP.BINANCE-1-MINUTE-INVALID-INTERNAL";
        let result = BarType::from_str(input);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 3"
            )
        );
    }

    #[rstest]
    fn test_bar_type_parse_invalid_token_pos_4() {
        let input = "BTCUSDT-PERP.BINANCE-1-MINUTE-BID-INVALID";
        let result = BarType::from_str(input);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 4"
            )
        );
    }

    #[rstest]
    fn test_bar_type_parse_invalid_token_pos_5() {
        let input = "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@INVALID-MINUTE-EXTERNAL";
        let result = BarType::from_str(input);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 5"
            )
        );
    }

    #[rstest]
    fn test_bar_type_parse_invalid_token_pos_6() {
        let input = "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-INVALID-EXTERNAL";
        let result = BarType::from_str(input);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 6"
            )
        );
    }

    #[rstest]
    fn test_bar_type_parse_invalid_token_pos_7() {
        let input = "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-MINUTE-INVALID";
        let result = BarType::from_str(input);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 7"
            )
        );
    }

    #[rstest]
    fn test_bar_type_equality() {
        let instrument_id1 = InstrumentId {
            symbol: Symbol::new("AUD/USD"),
            venue: Venue::new("SIM"),
        };
        let instrument_id2 = InstrumentId {
            symbol: Symbol::new("GBP/USD"),
            venue: Venue::new("SIM"),
        };
        let bar_spec = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_type1 = BarType::Standard {
            instrument_id: instrument_id1,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };
        let bar_type2 = BarType::Standard {
            instrument_id: instrument_id1,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };
        let bar_type3 = BarType::Standard {
            instrument_id: instrument_id2,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };
        assert_eq!(bar_type1, bar_type1);
        assert_eq!(bar_type1, bar_type2);
        assert_ne!(bar_type1, bar_type3);
    }

    #[rstest]
    fn test_bar_type_comparison() {
        let instrument_id1 = InstrumentId {
            symbol: Symbol::new("AUD/USD"),
            venue: Venue::new("SIM"),
        };

        let instrument_id2 = InstrumentId {
            symbol: Symbol::new("GBP/USD"),
            venue: Venue::new("SIM"),
        };
        let bar_spec = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_spec2 = BarSpecification {
            step: 2,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_type1 = BarType::Standard {
            instrument_id: instrument_id1,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };
        let bar_type2 = BarType::Standard {
            instrument_id: instrument_id1,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };
        let bar_type3 = BarType::Standard {
            instrument_id: instrument_id2,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };
        let bar_type4 = BarType::Composite {
            instrument_id: instrument_id2,
            spec: bar_spec2,
            aggregation_source: AggregationSource::Internal,

            composite_step: 1,
            composite_aggregation: BarAggregation::Minute,
            composite_aggregation_source: AggregationSource::External,
        };

        assert!(bar_type1 <= bar_type2);
        assert!(bar_type1 < bar_type3);
        assert!(bar_type3 > bar_type1);
        assert!(bar_type3 >= bar_type1);
        assert!(bar_type4 >= bar_type1);
    }

    #[rstest]
    fn test_bar_equality() {
        let instrument_id = InstrumentId {
            symbol: Symbol::new("AUDUSD"),
            venue: Venue::new("SIM"),
        };
        let bar_spec = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_type = BarType::Standard {
            instrument_id,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };
        let bar1 = Bar {
            bar_type,
            open: Price::from("1.00001"),
            high: Price::from("1.00004"),
            low: Price::from("1.00002"),
            close: Price::from("1.00003"),
            volume: Quantity::from("100000"),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::from(1),
        };

        let bar2 = Bar {
            bar_type,
            open: Price::from("1.00000"),
            high: Price::from("1.00004"),
            low: Price::from("1.00002"),
            close: Price::from("1.00003"),
            volume: Quantity::from("100000"),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::from(1),
        };
        assert_eq!(bar1, bar1);
        assert_ne!(bar1, bar2);
    }

    #[rstest]
    fn test_json_serialization() {
        let bar = Bar::default();
        let serialized = bar.as_json_bytes().unwrap();
        let deserialized = Bar::from_json_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, bar);
    }

    #[rstest]
    fn test_msgpack_serialization() {
        let bar = Bar::default();
        let serialized = bar.as_msgpack_bytes().unwrap();
        let deserialized = Bar::from_msgpack_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, bar);
    }
}
