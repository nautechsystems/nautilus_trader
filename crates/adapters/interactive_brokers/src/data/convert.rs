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

use ibapi::market_data::{
    historical::{
        BarSize as HistoricalBarSize, BarTimestamp, Duration as IBDuration, ToDuration,
        WhatToShow as HistoricalWhatToShow,
    },
    realtime::WhatToShow as RealtimeWhatToShow,
};
use jiff::Timestamp;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarSpecification, BarType},
    enums::{BarAggregation, PriceType},
    types::{Price, Quantity},
};
use time::OffsetDateTime;

/// Convert Nautilus BarType to IB HistoricalBarSize.
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
        (BarAggregation::Minute, 10) => HistoricalBarSize::Min10,
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

/// Whether IB requires `AGGTRADES` (not `TRADES`) for this request.
///
/// TWS rejects `TRADES` for crypto contracts (ZEROHASH/PAXOS) with error 10299 on
/// both `reqHistoricalData` and `reqRealTimeBars`; crypto trade-price data is served
/// only under `AGGTRADES`. Non-crypto contracts and non-trade price types are
/// unaffected. Mirrors the Java engine's `LiveOneMinBarIngestionService.whatToShowFor`.
#[must_use]
fn uses_agg_trades(is_crypto: bool, price_type: PriceType) -> bool {
    is_crypto && price_type == PriceType::Last
}

/// Convert Nautilus PriceType to IB WhatToShow for historical bars, mapping crypto
/// trade-price (`PriceType::Last`) to `AGGTRADES` (see `uses_agg_trades`).
#[must_use]
pub fn price_type_to_ib_what_to_show_for_security(
    price_type: PriceType,
    is_crypto: bool,
) -> HistoricalWhatToShow {
    if uses_agg_trades(is_crypto, price_type) {
        return HistoricalWhatToShow::AggTrades;
    }
    price_type_to_ib_what_to_show(price_type)
}

/// Convert Nautilus PriceType to IB WhatToShow for real-time (5-second) bars.
///
/// Unmapped price types default to [`RealtimeWhatToShow::Trades`].
#[must_use]
pub fn price_type_to_ib_realtime_what_to_show(price_type: PriceType) -> RealtimeWhatToShow {
    match price_type {
        PriceType::Last => RealtimeWhatToShow::Trades,
        PriceType::Bid => RealtimeWhatToShow::Bid,
        PriceType::Ask => RealtimeWhatToShow::Ask,
        PriceType::Mid => RealtimeWhatToShow::MidPoint,
        _ => RealtimeWhatToShow::Trades, // Default to trades
    }
}

/// Convert Nautilus PriceType to IB WhatToShow for real-time (5-second) bars, mapping
/// crypto trade-price (`PriceType::Last`) to `AGGTRADES` (see `uses_agg_trades`).
#[must_use]
pub fn price_type_to_ib_realtime_what_to_show_for_security(
    price_type: PriceType,
    is_crypto: bool,
) -> RealtimeWhatToShow {
    if uses_agg_trades(is_crypto, price_type) {
        return RealtimeWhatToShow::AggTrades;
    }
    price_type_to_ib_realtime_what_to_show(price_type)
}

#[must_use]
pub fn apply_price_magnifier(price: f64, price_magnifier: i32) -> f64 {
    if price_magnifier > 0 {
        price / f64::from(price_magnifier)
    } else {
        price
    }
}

#[must_use]
pub fn apply_bar_price_magnifier(
    ib_bar: &ibapi::market_data::historical::Bar,
    price_magnifier: i32,
) -> ibapi::market_data::historical::Bar {
    ibapi::market_data::historical::Bar {
        date: ib_bar.date,
        open: apply_price_magnifier(ib_bar.open, price_magnifier),
        high: apply_price_magnifier(ib_bar.high, price_magnifier),
        low: apply_price_magnifier(ib_bar.low, price_magnifier),
        close: apply_price_magnifier(ib_bar.close, price_magnifier),
        volume: ib_bar.volume,
        wap: apply_price_magnifier(ib_bar.wap, price_magnifier),
        count: ib_bar.count,
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
/// `ts_event` and `ts_init` are set to the bar close ([`bar_close_from_open`]).
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
    let ts_event = bar_close_from_open(
        ib_bar_timestamp_to_unix_nanos(&ib_bar.date),
        &bar_type.spec(),
    );
    let ts_init = ts_event;

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

/// Compute a bar's close timestamp from its open timestamp and [`BarSpecification`].
///
/// Weekly/monthly bars are returned unchanged (IB stamps these at the period end).
#[must_use]
pub fn bar_close_from_open(open: UnixNanos, spec: &BarSpecification) -> UnixNanos {
    let is_day = spec.aggregation == BarAggregation::Day;
    let duration_ns = match spec.aggregation {
        BarAggregation::Second
        | BarAggregation::Minute
        | BarAggregation::Hour
        | BarAggregation::Day => spec.timedelta().as_nanos(),
        _ => return open,
    };
    let Ok(duration_ns) = u64::try_from(duration_ns) else {
        return open;
    };
    let close = open.saturating_add_ns(duration_ns);
    if is_day {
        close.saturating_sub_ns(1_u64)
    } else {
        close
    }
}

/// Convert IB historical bar timestamp to UnixNanos.
#[must_use]
pub fn ib_bar_timestamp_to_unix_nanos(dt: &BarTimestamp) -> UnixNanos {
    match dt {
        BarTimestamp::Date(date) => ib_timestamp_to_unix_nanos(&date.midnight().assume_utc()),
        BarTimestamp::DateTime(dt) => ib_timestamp_to_unix_nanos(dt),
    }
}

/// Convert IB timestamp (OffsetDateTime) to UnixNanos.
#[must_use]
pub fn ib_timestamp_to_unix_nanos(dt: &OffsetDateTime) -> UnixNanos {
    let timestamp = dt.unix_timestamp_nanos();
    UnixNanos::from(timestamp as u64)
}

/// Convert `Timestamp` to OffsetDateTime.
pub fn jiff_to_ib_datetime(dt: &Timestamp) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(dt.as_nanosecond())
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
}

/// Calculate duration for IB historical data request.
///
/// # Errors
///
/// Returns an error if duration calculation fails.
pub fn calculate_duration(
    start: Option<Timestamp>,
    end: Option<Timestamp>,
) -> anyhow::Result<IBDuration> {
    match (start, end) {
        (Some(start_dt), Some(end_dt)) => {
            let duration = end_dt.duration_since(start_dt);
            let days = duration.as_secs() / (24 * 60 * 60);

            if days > 0 && days <= i32::MAX as i64 {
                Ok((days as i32).days())
            } else {
                // Fallback to seconds if less than a day or too large
                let seconds = duration.as_secs();
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
pub fn calculate_duration_segments(
    start: Timestamp,
    end: Timestamp,
) -> Vec<(Timestamp, IBDuration)> {
    let mut results = Vec::new();
    let duration = end.duration_since(start);
    let mut total_seconds = duration.as_secs();

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
        let minus_years_duration = jiff::SignedDuration::from_hours(24 * (years * 365));
        let minus_years_date = end - minus_years_duration;
        results.push((minus_years_date, (days as i32).days()));
    }

    if seconds > 0 {
        let minus_years_duration = jiff::SignedDuration::from_hours(24 * (years * 365));
        let minus_days_duration = jiff::SignedDuration::from_hours(24 * (days));
        let minus_days_date = end - minus_years_duration - minus_days_duration;
        results.push((minus_days_date, (seconds as i32).seconds()));
    }

    results
}

/// Adapt duration segments for an IB historical bars request.
///
/// For continuous futures the end date is dropped and only the first segment is
/// kept (IB rejects an explicit end date with error 10339), logging a warning
/// when the requested range cannot be honored.
pub fn bar_request_segments(
    segments: Vec<(Timestamp, IBDuration)>,
    is_continuous_future: bool,
) -> Vec<(Option<Timestamp>, IBDuration)> {
    if is_continuous_future {
        // Treat end dates within the last second as "now" so requests whose
        // end defaults to the current time do not trigger a spurious warning.
        let now = Timestamp::now();
        let end_in_past = segments
            .first()
            .is_some_and(|(end, _)| *end < now - jiff::SignedDuration::from_secs(1));

        if end_in_past || segments.len() > 1 {
            tracing::warn!(
                "Continuous futures cannot use an explicit end_date_time (IB error 10339); \
                 the request is anchored to the current time using only the first duration \
                 segment, so the returned bars may not cover the full requested range"
            );
        }

        segments
            .into_iter()
            .take(1)
            .map(|(_, d)| (None, d))
            .collect()
    } else {
        segments
            .into_iter()
            .map(|(end, d)| (Some(end), d))
            .collect()
    }
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
            BarSpecification::new(12, BarAggregation::Minute, PriceType::Last),
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
    fn test_price_type_to_ib_what_to_show_for_security_crypto() {
        // Crypto trade-price (Last) must map to AGGTRADES, not TRADES - TWS rejects
        // TRADES for crypto (error 10299). Mirrors the Java whatToShowFor rule.
        assert_eq!(
            price_type_to_ib_what_to_show_for_security(PriceType::Last, true),
            HistoricalWhatToShow::AggTrades
        );
        // Non-trade price types are unaffected by the crypto special case.
        assert_eq!(
            price_type_to_ib_what_to_show_for_security(PriceType::Bid, true),
            HistoricalWhatToShow::Bid
        );
        assert_eq!(
            price_type_to_ib_what_to_show_for_security(PriceType::Ask, true),
            HistoricalWhatToShow::Ask
        );
        assert_eq!(
            price_type_to_ib_what_to_show_for_security(PriceType::Mid, true),
            HistoricalWhatToShow::MidPoint
        );
    }

    #[rstest]
    fn test_price_type_to_ib_what_to_show_for_security_non_crypto() {
        // Non-crypto: trade-price stays TRADES (equities/futures), everything else
        // identical to the plain mapping.
        assert_eq!(
            price_type_to_ib_what_to_show_for_security(PriceType::Last, false),
            HistoricalWhatToShow::Trades
        );
        assert_eq!(
            price_type_to_ib_what_to_show_for_security(PriceType::Bid, false),
            HistoricalWhatToShow::Bid
        );
        assert_eq!(
            price_type_to_ib_what_to_show_for_security(PriceType::Mid, false),
            HistoricalWhatToShow::MidPoint
        );
    }

    #[rstest]
    fn test_aggtrades_wire_string() {
        // The vendored ibapi patch must serialize AggTrades as the exact IB wire
        // token "AGGTRADES" on BOTH the historical and realtime enums.
        assert_eq!(HistoricalWhatToShow::AggTrades.to_string(), "AGGTRADES");
        assert_eq!(RealtimeWhatToShow::AggTrades.to_string(), "AGGTRADES");
    }

    #[rstest]
    fn test_price_type_to_ib_realtime_what_to_show() {
        // `RealtimeWhatToShow` does not derive `PartialEq`, so match on the variants.
        assert!(matches!(
            price_type_to_ib_realtime_what_to_show(PriceType::Last),
            RealtimeWhatToShow::Trades
        ));
        assert!(matches!(
            price_type_to_ib_realtime_what_to_show(PriceType::Bid),
            RealtimeWhatToShow::Bid
        ));
        assert!(matches!(
            price_type_to_ib_realtime_what_to_show(PriceType::Ask),
            RealtimeWhatToShow::Ask
        ));
        assert!(matches!(
            price_type_to_ib_realtime_what_to_show(PriceType::Mid),
            RealtimeWhatToShow::MidPoint
        ));
    }

    #[rstest]
    fn test_price_type_to_ib_realtime_what_to_show_for_security_crypto() {
        // Crypto trade-price (Last) 5-second bars must request AGGTRADES on the
        // realtime path too - TWS rejects TRADES for crypto (error 10299) on
        // reqRealTimeBars, exactly as on the historical path. Mirrors the Java
        // engine passing whatToShowFor(CRYPTO)="AGGTRADES" to subscribeRealTimeBars.
        assert!(matches!(
            price_type_to_ib_realtime_what_to_show_for_security(PriceType::Last, true),
            RealtimeWhatToShow::AggTrades
        ));
        // Non-trade price types unaffected by the crypto special case.
        assert!(matches!(
            price_type_to_ib_realtime_what_to_show_for_security(PriceType::Mid, true),
            RealtimeWhatToShow::MidPoint
        ));
        assert!(matches!(
            price_type_to_ib_realtime_what_to_show_for_security(PriceType::Bid, true),
            RealtimeWhatToShow::Bid
        ));
        // Non-crypto trade-price stays TRADES.
        assert!(matches!(
            price_type_to_ib_realtime_what_to_show_for_security(PriceType::Last, false),
            RealtimeWhatToShow::Trades
        ));
    }

    #[rstest]
    fn test_ib_bar_to_nautilus_bar() {
        let ib_bar = ibapi::market_data::historical::Bar {
            date: datetime!(2024-01-01 10:00:00 UTC).into(),
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
        let close = ib_timestamp_to_unix_nanos(&datetime!(2024-01-01 10:01:00 UTC));
        assert_eq!(bar.ts_event.as_u64(), close.as_u64());
        assert_eq!(bar.ts_init.as_u64(), close.as_u64());
    }

    #[rstest]
    fn test_ib_bar_to_nautilus_bar_negative_volume() {
        let ib_bar = ibapi::market_data::historical::Bar {
            date: datetime!(2024-01-01 10:00:00 UTC).into(),
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
    fn test_bar_close_from_open_intraday() {
        let open = ib_timestamp_to_unix_nanos(&datetime!(2024-01-01 10:00:00 UTC));

        let spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        assert_eq!(
            bar_close_from_open(open, &spec).as_u64(),
            ib_timestamp_to_unix_nanos(&datetime!(2024-01-01 10:00:01 UTC)).as_u64(),
        );

        let spec = BarSpecification::new(5, BarAggregation::Second, PriceType::Last);
        assert_eq!(
            bar_close_from_open(open, &spec).as_u64(),
            ib_timestamp_to_unix_nanos(&datetime!(2024-01-01 10:00:05 UTC)).as_u64(),
        );

        let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
        assert_eq!(
            bar_close_from_open(open, &spec).as_u64(),
            ib_timestamp_to_unix_nanos(&datetime!(2024-01-01 10:01:00 UTC)).as_u64(),
        );

        let spec = BarSpecification::new(1, BarAggregation::Hour, PriceType::Last);
        assert_eq!(
            bar_close_from_open(open, &spec).as_u64(),
            ib_timestamp_to_unix_nanos(&datetime!(2024-01-01 11:00:00 UTC)).as_u64(),
        );
    }

    #[rstest]
    fn test_bar_close_from_open_day() {
        let open = ib_timestamp_to_unix_nanos(&datetime!(2024-01-01 00:00:00 UTC));
        let spec = BarSpecification::new(1, BarAggregation::Day, PriceType::Last);
        assert_eq!(
            bar_close_from_open(open, &spec).as_u64(),
            open.as_u64() + 86_400_000_000_000 - 1,
        );
    }

    #[rstest]
    fn test_bar_close_from_open_week_month() {
        let open = ib_timestamp_to_unix_nanos(&datetime!(2024-01-13 00:00:00 UTC));

        let spec = BarSpecification::new(1, BarAggregation::Week, PriceType::Last);
        assert_eq!(bar_close_from_open(open, &spec).as_u64(), open.as_u64());

        let spec = BarSpecification::new(1, BarAggregation::Month, PriceType::Last);
        assert_eq!(bar_close_from_open(open, &spec).as_u64(), open.as_u64());
    }

    #[rstest]
    fn test_ib_timestamp_to_unix_nanos() {
        let dt = datetime!(2024-01-01 10:00:00 UTC);
        let result = ib_timestamp_to_unix_nanos(&dt);
        assert!(result.as_i64() > 0);
    }

    #[rstest]
    fn test_jiff_to_ib_datetime() {
        let utc_dt = "2024-01-01T10:00:00Z".parse::<Timestamp>().unwrap();
        let result = jiff_to_ib_datetime(&utc_dt);
        assert_eq!(result.year(), 2024);
        assert_eq!(result.month(), time::Month::January);
        assert_eq!(result.day(), 1);
    }

    #[rstest]
    fn test_calculate_duration_with_start_and_end() {
        let start = "2024-01-01T10:00:00Z".parse::<Timestamp>().unwrap();
        let end = "2024-01-02T10:00:00Z".parse::<Timestamp>().unwrap();
        let result = calculate_duration(Some(start), Some(end));
        assert!(result.is_ok());
        // Should be 1 day
        let duration = result.unwrap();
        assert!(duration.to_string().contains("1 D") || duration.to_string().contains("1D"));
    }

    #[rstest]
    fn test_calculate_duration_no_start() {
        let end = "2024-01-02T10:00:00Z".parse::<Timestamp>().unwrap();
        let result = calculate_duration(None, Some(end));
        assert!(result.is_ok());
        // Should default to 1 day
        let duration = result.unwrap();
        assert!(duration.to_string().contains("1 D") || duration.to_string().contains("1D"));
    }

    #[rstest]
    fn test_calculate_duration_no_end() {
        let start = "2024-01-01T10:00:00Z".parse::<Timestamp>().unwrap();
        let result = calculate_duration(Some(start), None);
        assert!(result.is_ok());
        // Should default to 1 day
        let duration = result.unwrap();
        assert!(duration.to_string().contains("1 D") || duration.to_string().contains("1D"));
    }

    #[rstest]
    fn test_calculate_duration_segments() {
        // Test case: 1.5 years ago to now
        let now = Timestamp::now();
        let start = now - jiff::SignedDuration::from_hours(24 * (365 + 182)); // ~1.5 years
        let segments = calculate_duration_segments(start, now);

        assert!(!segments.is_empty());
        // Should have at least one 1Y segment and one D/S segment
        assert!(segments.len() >= 2);

        // Check first segment is ~1Y
        let dur1 = &segments[0].1;
        assert!(dur1.to_string().contains("1 Y") || dur1.to_string().contains("1Y"));
    }

    #[rstest]
    fn test_bar_request_segments_attaches_end_dates_when_not_continuous() {
        let end = "2025-01-01T00:00:00Z".parse::<Timestamp>().unwrap();
        let earlier = "2024-06-01T00:00:00Z".parse::<Timestamp>().unwrap();
        let segments = vec![(end, IBDuration::years(1)), (earlier, IBDuration::days(30))];

        let result = bar_request_segments(segments, false);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, Some(end));
        assert_eq!(result[1].0, Some(earlier));
    }

    #[rstest]
    fn test_bar_request_segments_drops_end_date_and_keeps_only_first_for_continuous() {
        let end = "2025-01-01T00:00:00Z".parse::<Timestamp>().unwrap();
        let earlier = "2024-06-01T00:00:00Z".parse::<Timestamp>().unwrap();
        let segments = vec![(end, IBDuration::years(1)), (earlier, IBDuration::days(30))];

        let result = bar_request_segments(segments, true);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, None);
        assert_eq!(result[0].1, IBDuration::years(1));
    }

    #[rstest]
    fn test_bar_request_segments_empty_input_yields_nothing_for_continuous() {
        let result = bar_request_segments(vec![], true);
        assert!(result.is_empty());
    }
}
