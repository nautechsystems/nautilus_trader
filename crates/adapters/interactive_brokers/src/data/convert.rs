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

//! Conversion utilities for Interactive Brokers data types.

use chrono::{DateTime, Utc};
use ibapi::market_data::historical::{
    BarSize as HistoricalBarSize, Duration as IBDuration, ToDuration,
    WhatToShow as HistoricalWhatToShow,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType},
    enums::{BarAggregation, PriceType},
    types::{Price, Quantity},
};
use time::OffsetDateTime;

/// Convert Nautilus BarType to IB HistoricalBarSize.
///
/// # Arguments
///
/// * `bar_type` - The Nautilus bar type specification
///
/// # Errors
///
/// Returns an error if the bar aggregation/step combination is not supported by IB.
pub fn bar_type_to_ib_bar_size(bar_type: &BarType) -> anyhow::Result<HistoricalBarSize> {
    let spec = bar_type.spec();
    let aggregation = spec.aggregation;
    let step = spec.step.get();

    let bar_size = match (aggregation, step) {
        // Seconds
        (BarAggregation::Second, 1) => HistoricalBarSize::Sec,
        (BarAggregation::Second, 5) => HistoricalBarSize::Sec5,
        (BarAggregation::Second, 15) => HistoricalBarSize::Sec15,
        (BarAggregation::Second, 30) => HistoricalBarSize::Sec30,
        // Minutes
        (BarAggregation::Minute, 1) => HistoricalBarSize::Min,
        (BarAggregation::Minute, 2) => HistoricalBarSize::Min2,
        (BarAggregation::Minute, 3) => HistoricalBarSize::Min3,
        (BarAggregation::Minute, 5) => HistoricalBarSize::Min5,
        (BarAggregation::Minute, 10) => HistoricalBarSize::Min15, // IB doesn't have 10-min, closest is 15
        (BarAggregation::Minute, 15) => HistoricalBarSize::Min15,
        (BarAggregation::Minute, 20) => HistoricalBarSize::Min20,
        (BarAggregation::Minute, 30) => HistoricalBarSize::Min30,
        // Hours
        (BarAggregation::Hour, 1) => HistoricalBarSize::Hour,
        (BarAggregation::Hour, 2) => HistoricalBarSize::Hour2,
        (BarAggregation::Hour, 3) => HistoricalBarSize::Hour3,
        (BarAggregation::Hour, 4) => HistoricalBarSize::Hour4,
        (BarAggregation::Hour, 8) => HistoricalBarSize::Hour8,
        // Days
        (BarAggregation::Day, 1) => HistoricalBarSize::Day,
        // Weeks
        (BarAggregation::Week, 1) => HistoricalBarSize::Week,
        // Months
        (BarAggregation::Month, 1) => HistoricalBarSize::Month,
        _ => {
            anyhow::bail!("Unsupported bar aggregation/step combination: {aggregation:?}/{step}",);
        }
    };

    Ok(bar_size)
}

/// Convert Nautilus PriceType to IB WhatToShow.
///
/// # Arguments
///
/// * `price_type` - The Nautilus price type
///
/// # Returns
///
/// Returns the corresponding IB WhatToShow value.
#[must_use]
pub fn price_type_to_ib_what_to_show(price_type: PriceType) -> HistoricalWhatToShow {
    match price_type {
        PriceType::Last => HistoricalWhatToShow::Trades,
        PriceType::Bid => HistoricalWhatToShow::Bid,
        PriceType::Ask => HistoricalWhatToShow::Ask,
        PriceType::Mid => HistoricalWhatToShow::MidPoint,
        _ => HistoricalWhatToShow::Trades, // Default to trades
    }
}

/// Implement bar price validation logic.
/// Matches Python's `_validate_bar_prices` behavior.
fn _validate_bar_prices(open: &mut f64, high: &mut f64, low: &mut f64, close: &f64) {
    if *high < *low || *high < *open || *high < *close || *low > *open || *low > *close {
        tracing::warn!(
            "Invalid bar prices detected: O:{}, H:{}, L:{}, C:{}. Correcting using close price",
            open,
            high,
            low,
            close
        );
        *open = *close;
        *high = *close;
        *low = *close;
    }
}

/// Convert IB Bar to Nautilus Bar.
///
/// # Arguments
///
/// * `ib_bar` - The IB historical bar
/// * `bar_type` - The Nautilus bar type
/// * `price_precision` - Price precision for the instrument
/// * `size_precision` - Size precision for the instrument
///
/// # Errors
///
/// Returns an error if conversion fails.
pub fn ib_bar_to_nautilus_bar(
    ib_bar: &ibapi::market_data::historical::Bar,
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<Bar> {
    // Convert IB timestamp to UnixNanos
    let ts_event = ib_timestamp_to_unix_nanos(&ib_bar.date);
    let ts_init = ts_event; // Use same timestamp for init

    // Validate and correct prices
    let mut open = ib_bar.open;
    let mut high = ib_bar.high;
    let mut low = ib_bar.low;
    let close = ib_bar.close;
    _validate_bar_prices(&mut open, &mut high, &mut low, &close);

    // Create prices
    let open_price = Price::new(open, price_precision);
    let high_price = Price::new(high, price_precision);
    let low_price = Price::new(low, price_precision);
    let close_price = Price::new(close, price_precision);

    // Volume: IB uses -1 for unavailable volume, convert to 0
    let volume = if ib_bar.volume < 0.0 {
        Quantity::zero(size_precision)
    } else {
        Quantity::new(ib_bar.volume, size_precision)
    };

    Ok(Bar::new(
        bar_type,
        open_price,
        high_price,
        low_price,
        close_price,
        volume,
        ts_event,
        ts_init,
    ))
}

/// Convert IB timestamp (OffsetDateTime) to UnixNanos.
///
/// # Arguments
///
/// * `dt` - IB timestamp
///
/// # Returns
///
/// Returns UnixNanos timestamp.
#[must_use]
pub fn ib_timestamp_to_unix_nanos(dt: &OffsetDateTime) -> UnixNanos {
    let timestamp = dt.unix_timestamp_nanos();
    UnixNanos::from(timestamp as u64)
}

/// Convert `DateTime<Utc>` to OffsetDateTime.
///
/// # Arguments
///
/// * `dt` - Chrono DateTime
///
/// # Returns
///
/// Returns time OffsetDateTime.
pub fn chrono_to_ib_datetime(dt: &DateTime<Utc>) -> OffsetDateTime {
    let timestamp = dt.timestamp();
    let nanos = dt.timestamp_subsec_nanos();
    let total_nanos = timestamp as i128 * 1_000_000_000 + nanos as i128;
    OffsetDateTime::from_unix_timestamp_nanos(total_nanos)
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
}

/// Calculate duration for IB historical data request.
///
/// # Arguments
///
/// * `start` - Start time (optional)
/// * `end` - End time (optional)
///
/// # Errors
///
/// Returns an error if duration calculation fails.
///
/// # Returns
///
/// Returns IB Duration calculated from the time range.
pub fn calculate_duration(
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> anyhow::Result<IBDuration> {
    match (start, end) {
        (Some(start_dt), Some(end_dt)) => {
            let duration = end_dt.signed_duration_since(start_dt);
            let days = duration.num_days();

            if days > 0 && days <= i32::MAX as i64 {
                Ok((days as i32).days())
            } else {
                // Fallback to seconds if less than a day or too large
                let seconds = duration.num_seconds();
                if seconds > 0 && seconds <= i32::MAX as i64 {
                    Ok((seconds as i32).seconds())
                } else {
                    // Default to 1 day if calculation fails
                    Ok(1.days())
                }
            }
        }
        (None, Some(_)) => {
            // Default to 1 day if only end is provided
            Ok(1.days())
        }
        (Some(_), None) => {
            // Default to 1 day if only start is provided
            Ok(1.days())
        }
        (None, None) => {
            // Default to 1 day if neither is provided
            Ok(1.days())
        }
    }
}

/// Calculate duration segments for IB historical data request.
///
/// This is used to break down a large time range into multiple requests
/// to comply with IB's duration limits for specific bar sizes.
///
/// # Arguments
///
/// * `start` - Start time
/// * `end` - End time
///
/// # Returns
///
/// Returns a vector of (end_date, duration) tuples.
pub fn calculate_duration_segments(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, IBDuration)> {
    let mut results = Vec::new();
    let duration = end.signed_duration_since(start);
    let mut total_seconds = duration.num_seconds();

    if total_seconds <= 0 {
        return results;
    }

    let years = total_seconds / (365 * 24 * 3600);
    total_seconds %= 365 * 24 * 3600;
    let days = total_seconds / (24 * 3600);
    total_seconds %= 24 * 3600;
    let seconds = total_seconds;

    if years > 0 {
        results.push((end, (years as i32).years()));
    }

    if days > 0 {
        let minus_years_duration = chrono::Duration::days(years * 365);
        let minus_years_date = end - minus_years_duration;
        results.push((minus_years_date, (days as i32).days()));
    }

    if seconds > 0 {
        let minus_years_duration = chrono::Duration::days(years * 365);
        let minus_days_duration = chrono::Duration::days(days);
        let minus_days_date = end - minus_years_duration - minus_days_duration;
        results.push((minus_days_date, (seconds as i32).seconds()));
    }

    results
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::{BarSpecification, BarType},
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::{InstrumentId, Symbol, Venue},
    };
    use rstest::rstest;
    use time::macros::datetime;

    use super::*;

    fn create_test_instrument_id() -> InstrumentId {
        InstrumentId::new(Symbol::from("AAPL"), Venue::from("NASDAQ"))
    }

    #[rstest]
    fn test_bar_type_to_ib_bar_size_seconds() {
        let instrument_id = create_test_instrument_id();
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Second, PriceType::Last),
            AggregationSource::External,
        );
        let result = bar_type_to_ib_bar_size(&bar_type);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), HistoricalBarSize::Sec);

        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(5, BarAggregation::Second, PriceType::Last),
            AggregationSource::External,
        );
        let result = bar_type_to_ib_bar_size(&bar_type);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), HistoricalBarSize::Sec5);
    }

    #[rstest]
    fn test_bar_type_to_ib_bar_size_minutes() {
        let instrument_id = create_test_instrument_id();
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let result = bar_type_to_ib_bar_size(&bar_type);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), HistoricalBarSize::Min);

        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(15, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let result = bar_type_to_ib_bar_size(&bar_type);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), HistoricalBarSize::Min15);
    }

    #[rstest]
    fn test_bar_type_to_ib_bar_size_hours() {
        let instrument_id = create_test_instrument_id();
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Hour, PriceType::Last),
            AggregationSource::External,
        );
        let result = bar_type_to_ib_bar_size(&bar_type);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), HistoricalBarSize::Hour);
    }

    #[rstest]
    fn test_bar_type_to_ib_bar_size_days() {
        let instrument_id = create_test_instrument_id();
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Day, PriceType::Last),
            AggregationSource::External,
        );
        let result = bar_type_to_ib_bar_size(&bar_type);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), HistoricalBarSize::Day);
    }

    #[rstest]
    fn test_bar_type_to_ib_bar_size_unsupported() {
        let instrument_id = create_test_instrument_id();
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(99, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let result = bar_type_to_ib_bar_size(&bar_type);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_price_type_to_ib_what_to_show() {
        assert_eq!(
            price_type_to_ib_what_to_show(PriceType::Last),
            HistoricalWhatToShow::Trades
        );
        assert_eq!(
            price_type_to_ib_what_to_show(PriceType::Bid),
            HistoricalWhatToShow::Bid
        );
        assert_eq!(
            price_type_to_ib_what_to_show(PriceType::Ask),
            HistoricalWhatToShow::Ask
        );
        assert_eq!(
            price_type_to_ib_what_to_show(PriceType::Mid),
            HistoricalWhatToShow::MidPoint
        );
    }

    #[rstest]
    fn test_ib_bar_to_nautilus_bar() {
        let ib_bar = ibapi::market_data::historical::Bar {
            date: datetime!(2024-01-01 10:00:00 UTC),
            open: 150.0,
            high: 151.0,
            low: 149.0,
            close: 150.5,
            volume: 1000.0,
            wap: 150.25,
            count: 100,
        };

        let instrument_id = create_test_instrument_id();
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let result = ib_bar_to_nautilus_bar(&ib_bar, bar_type, 2, 0);
        assert!(result.is_ok());
        let bar = result.unwrap();
        assert_eq!(bar.open.as_f64(), 150.0);
        assert_eq!(bar.high.as_f64(), 151.0);
        assert_eq!(bar.low.as_f64(), 149.0);
        assert_eq!(bar.close.as_f64(), 150.5);
        assert_eq!(bar.volume.as_f64(), 1000.0);
    }

    #[rstest]
    fn test_ib_bar_to_nautilus_bar_negative_volume() {
        let ib_bar = ibapi::market_data::historical::Bar {
            date: datetime!(2024-01-01 10:00:00 UTC),
            open: 150.0,
            high: 151.0,
            low: 149.0,
            close: 150.5,
            volume: -1.0, // Unavailable volume
            wap: 150.25,
            count: 100,
        };

        let instrument_id = create_test_instrument_id();
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let result = ib_bar_to_nautilus_bar(&ib_bar, bar_type, 2, 0);
        assert!(result.is_ok());
        let bar = result.unwrap();
        // Negative volume should be converted to 0
        assert_eq!(bar.volume.as_f64(), 0.0);
    }

    #[rstest]
    fn test_ib_timestamp_to_unix_nanos() {
        let dt = datetime!(2024-01-01 10:00:00 UTC);
        let result = ib_timestamp_to_unix_nanos(&dt);
        assert!(result.as_i64() > 0);
    }

    #[rstest]
    fn test_chrono_to_ib_datetime() {
        let dt = DateTime::parse_from_rfc3339("2024-01-01T10:00:00Z").unwrap();
        let utc_dt = dt.with_timezone(&Utc);
        let result = chrono_to_ib_datetime(&utc_dt);
        assert_eq!(result.year(), 2024);
        assert_eq!(result.month(), time::Month::January);
        assert_eq!(result.day(), 1);
    }

    #[rstest]
    fn test_calculate_duration_with_start_and_end() {
        let start = DateTime::parse_from_rfc3339("2024-01-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = DateTime::parse_from_rfc3339("2024-01-02T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = calculate_duration(Some(start), Some(end));
        assert!(result.is_ok());
        // Should be 1 day
        let duration = result.unwrap();
        assert!(duration.to_string().contains("1 D") || duration.to_string().contains("1D"));
    }

    #[rstest]
    fn test_calculate_duration_no_start() {
        let end = DateTime::parse_from_rfc3339("2024-01-02T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = calculate_duration(None, Some(end));
        assert!(result.is_ok());
        // Should default to 1 day
        let duration = result.unwrap();
        assert!(duration.to_string().contains("1 D") || duration.to_string().contains("1D"));
    }

    #[rstest]
    fn test_calculate_duration_no_end() {
        let start = DateTime::parse_from_rfc3339("2024-01-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = calculate_duration(Some(start), None);
        assert!(result.is_ok());
        // Should default to 1 day
        let duration = result.unwrap();
        assert!(duration.to_string().contains("1 D") || duration.to_string().contains("1D"));
    }

    #[rstest]
    fn test_calculate_duration_segments() {
        // Test case: 1.5 years ago to now
        let now = Utc::now();
        let start = now - chrono::Duration::days(365 + 182); // ~1.5 years
        let segments = calculate_duration_segments(start, now);

        assert!(!segments.is_empty());
        // Should have at least one 1Y segment and one D/S segment
        assert!(segments.len() >= 2);

        // Check first segment is ~1Y
        let dur1 = &segments[0].1;
        assert!(dur1.to_string().contains("1 Y") || dur1.to_string().contains("1Y"));
    }
}
