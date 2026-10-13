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

use std::str::FromStr;

use ibapi::market_data::{
    historical::{
        BarSize as HistoricalBarSize, BarTimestamp, Duration as IBDuration, ToDuration,
        WhatToShow as HistoricalWhatToShow,
    },
    realtime::WhatToShow as RealtimeWhatToShow,
};
use jiff::Timestamp;
use nautilus_core::{DurationNanos, UnixNanos};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType},
    enums::{AssetClass, BarAggregation, PriceType},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::{Decimal, RoundingStrategy};
use time::OffsetDateTime;

/// Converts the prices IB reports for an instrument into the instrument's prices.
///
/// IB reports a magnified contract's prices in a sub-unit of its currency (CBOT grains in cents,
/// LSE stocks in pence), so these are divided by the contract's price magnifier. IB also reports
/// an FX pair's prices in steps finer than the pair's price increment, its order tick, so these
/// are rounded to the increment: a bid down, an ask up, and any other price to the nearest, ties to
/// the even multiple. A quote then never locks or crosses unless IB's did, and an ask is never
/// below IB's ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IbPriceConverter {
    precision: u8,
    increment: Option<Price>,
    magnifier: i32,
}

impl IbPriceConverter {
    /// Creates a converter that applies the precision only, with no magnifier and no rounding to
    /// an increment.
    #[must_use]
    pub const fn new(precision: u8) -> Self {
        Self {
            precision,
            increment: None,
            magnifier: 1,
        }
    }

    /// Returns the converter with the contract's price magnifier, where a value below 1 means 1.
    #[must_use]
    pub const fn with_price_magnifier(mut self, magnifier: i32) -> Self {
        self.magnifier = if magnifier < 1 { 1 } else { magnifier };
        self
    }

    /// Creates the converter for `instrument` and its contract's `price_magnifier`, rounding to
    /// the price increment for FX pairs.
    #[must_use]
    pub fn for_instrument(instrument: &InstrumentAny, price_magnifier: i32) -> Self {
        let increment = match instrument {
            InstrumentAny::CurrencyPair(pair) if pair.asset_class() == AssetClass::FX => {
                Some(pair.price_increment())
            }
            _ => None,
        };

        Self {
            precision: instrument.price_precision(),
            increment,
            magnifier: 1,
        }
        .with_price_magnifier(price_magnifier)
    }

    /// Returns the price precision.
    #[must_use]
    pub const fn precision(&self) -> u8 {
        self.precision
    }

    /// Converts a price reported by IB into the instrument's price, rounding to the nearest
    /// multiple of the price increment, ties to the even multiple.
    ///
    /// Use this for a trade, midpoint, last, or index price. A quote's bid and ask use
    /// [`Self::bid_price`] and [`Self::ask_price`].
    ///
    /// # Errors
    ///
    /// Returns an error if the price is not finite or cannot be represented at the converter's
    /// precision.
    pub fn price(&self, ib_price: f64) -> anyhow::Result<Price> {
        self.convert(ib_price, RoundingStrategy::MidpointNearestEven)
    }

    /// Converts a bid price reported by IB, rounding down to a multiple of the price increment.
    ///
    /// A published bid is never above IB's bid.
    ///
    /// # Errors
    ///
    /// Returns an error if the price is not finite or cannot be represented at the converter's
    /// precision.
    pub fn bid_price(&self, ib_price: f64) -> anyhow::Result<Price> {
        self.convert(ib_price, RoundingStrategy::ToNegativeInfinity)
    }

    /// Converts an ask price reported by IB, rounding up to a multiple of the price increment.
    ///
    /// A published ask is never below IB's ask, so a limit order priced at it can fill, and the
    /// published bid stays below the published ask whenever IB's does.
    ///
    /// # Errors
    ///
    /// Returns an error if the price is not finite or cannot be represented at the converter's
    /// precision.
    pub fn ask_price(&self, ib_price: f64) -> anyhow::Result<Price> {
        self.convert(ib_price, RoundingStrategy::ToPositiveInfinity)
    }

    /// Converts a price of a bar of `price_type`: a bid bar rounds down, an ask bar rounds up,
    /// and any other bar to the nearest multiple of the price increment.
    ///
    /// # Errors
    ///
    /// Returns an error if the price is not finite or cannot be represented at the converter's
    /// precision.
    pub fn bar_price(&self, price_type: PriceType, ib_price: f64) -> anyhow::Result<Price> {
        match price_type {
            PriceType::Bid => self.bid_price(ib_price),
            PriceType::Ask => self.ask_price(ib_price),
            _ => self.price(ib_price),
        }
    }

    /// Converts `ib_price` once, in `Decimal`, so that a midpoint is rounded a single time.
    ///
    /// With no increment and no magnifier the price is built as is. Otherwise the shortest
    /// round-trip decimal form of the `f64` is divided by the magnifier and rounded to the
    /// increment with `strategy`, or to the precision when there is no increment.
    fn convert(&self, ib_price: f64, strategy: RoundingStrategy) -> anyhow::Result<Price> {
        if self.increment.is_none() && self.magnifier == 1 {
            return Price::new_checked(ib_price, self.precision)
                .map_err(|e| anyhow::anyhow!("invalid price={ib_price}: {e}"));
        }

        if !ib_price.is_finite() {
            anyhow::bail!("invalid price={ib_price}: not finite");
        }

        let overflow = || anyhow::anyhow!("invalid price={ib_price}: out of range");
        let value = Decimal::from_str(&ib_price.to_string())
            .map_err(|e| anyhow::anyhow!("invalid price={ib_price}: {e}"))?
            .checked_div(Decimal::from(self.magnifier))
            .ok_or_else(overflow)?;

        let value = match self.increment {
            Some(increment) if increment.raw() > 0 => {
                let step = increment.as_decimal();
                value
                    .checked_div(step)
                    .ok_or_else(overflow)?
                    .round_dp_with_strategy(0, strategy)
                    .checked_mul(step)
                    .ok_or_else(overflow)?
            }
            _ => value,
        };

        Price::from_decimal_dp(value, self.precision)
            .map_err(|e| anyhow::anyhow!("invalid price={ib_price}: {e}"))
    }
}

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
    prices: IbPriceConverter,
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
    let price_type = bar_type.spec().price_type;
    let open_price = prices.bar_price(price_type, open)?;
    let high_price = prices.bar_price(price_type, high)?;
    let low_price = prices.bar_price(price_type, low)?;
    let close_price = prices.bar_price(price_type, close)?;

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

    let duration = match spec.aggregation {
        BarAggregation::Second
        | BarAggregation::Minute
        | BarAggregation::Hour
        | BarAggregation::Day => spec.timedelta(),
        _ => return open,
    };

    let Ok(duration) = DurationNanos::try_from(duration) else {
        return open;
    };

    let close = open.saturating_add(duration);
    if is_day {
        close.saturating_sub(DurationNanos::new(1))
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

/// Calculate duration segments for IB historical data request.
///
/// This is used to break down a large time range into multiple requests
/// to comply with IB's duration limits for specific bar sizes.
pub fn calculate_duration_segments(
    start: Option<Timestamp>,
    end: Option<Timestamp>,
    duration: Option<IBDuration>,
) -> Vec<(Timestamp, IBDuration)> {
    let end = end.unwrap_or_else(Timestamp::now);

    if let Some(duration) = duration {
        return vec![(end, duration)];
    }

    let Some(start) = start else {
        return vec![(end, 1.days())];
    };

    let mut results = Vec::new();
    let duration = end.duration_since(start);
    let mut total_seconds = duration.as_secs();
    if duration.subsec_nanos() > 0 {
        total_seconds += 1;
    }

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

/// Return whether a backward historical tick request needs another page.
#[must_use]
pub(crate) fn should_continue_historical_tick_pagination(
    current_start_date: Option<Timestamp>,
    current_end_date: Option<Timestamp>,
    current_len: usize,
    limit: Option<usize>,
) -> bool {
    limit.is_none_or(|limit| current_len < limit)
        && current_start_date
            .zip(current_end_date)
            .is_none_or(|(start, end)| end > start)
}

/// Add a historical tick page and advance the next request before its earliest tick.
#[allow(clippy::too_many_arguments)] // The parameters describe one pagination transition.
pub(crate) fn extend_historical_tick_batch<T>(
    all_ticks: &mut Vec<T>,
    batch_ticks: Vec<T>,
    current_start_date: Option<Timestamp>,
    current_end_date: &mut Option<Timestamp>,
    start_nanos: Option<UnixNanos>,
    end_nanos: Option<UnixNanos>,
    limit: Option<usize>,
    ts_event: impl Fn(&T) -> UnixNanos,
) -> bool {
    if batch_ticks.is_empty() {
        return false;
    }

    // The IB wire encodes the request end at second resolution and treats it as
    // inclusive, so each page re-delivers ticks from the boundary second that a
    // previous page already collected. Drop everything at or past the requested
    // end before extending; an entirely re-delivered page means the end second
    // cannot retreat (a single second holds a full page) and pagination must stop.
    let mut batch_ticks = batch_ticks;

    if let Some(end) = *current_end_date {
        let end_bound_nanos = end.as_nanosecond();
        batch_ticks.retain(|tick| i128::from(ts_event(tick).as_u64()) <= end_bound_nanos);
        if batch_ticks.is_empty() {
            tracing::warn!(
                "Historical tick pagination pinned at {end}; a single second exceeds the page \
                 size, earlier ticks within that second cannot be requested"
            );
            retain_historical_ticks_in_range(all_ticks, start_nanos, end_nanos, &ts_event);
            return false;
        }
    }

    let Some(min_ts_nanos) = batch_ticks.iter().map(&ts_event).min() else {
        return false;
    };

    let new_end_nanos = min_ts_nanos.as_u64().saturating_sub(1);

    let Ok(new_end) = Timestamp::from_nanosecond(i128::from(new_end_nanos)) else {
        return false;
    };

    *current_end_date = Some(new_end);

    all_ticks.extend(batch_ticks);
    if current_start_date
        .zip(*current_end_date)
        .is_some_and(|(start, end)| end <= start)
    {
        retain_historical_ticks_in_range(all_ticks, start_nanos, end_nanos, &ts_event);
        return false;
    }

    limit.is_none_or(|limit| all_ticks.len() < limit)
}

/// Retain historical ticks within the inclusive requested range.
pub(crate) fn retain_historical_ticks_in_range<T>(
    ticks: &mut Vec<T>,
    start_nanos: Option<UnixNanos>,
    end_nanos: Option<UnixNanos>,
    ts_event: impl Fn(&T) -> UnixNanos,
) {
    ticks.retain(|tick| {
        let ts_event = ts_event(tick);
        start_nanos.is_none_or(|start| ts_event >= start)
            && end_nanos.is_none_or(|end| ts_event <= end)
    });
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
    use std::cmp::Ordering;

    use nautilus_model::{
        data::{BarSpecification, BarType},
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::{InstrumentId, Symbol, Venue},
        instruments::Equity,
        types::Currency,
    };
    use rstest::rstest;
    use time::macros::datetime;

    use super::*;
    use crate::stubs::currency_pair;

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
        let result = ib_bar_to_nautilus_bar(&ib_bar, bar_type, IbPriceConverter::new(2), 0);
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
        let result = ib_bar_to_nautilus_bar(&ib_bar, bar_type, IbPriceConverter::new(2), 0);
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
    fn test_calculate_duration_segments_with_start_and_end() {
        let start = "2024-01-01T10:00:00Z".parse::<Timestamp>().unwrap();
        let end = "2024-01-02T10:00:00Z".parse::<Timestamp>().unwrap();
        let result = calculate_duration_segments(Some(start), Some(end), None);

        assert_eq!(result, vec![(end, IBDuration::days(1))]);
    }

    #[rstest]
    fn test_calculate_duration_segments_without_start_defaults_to_one_day() {
        let end = "2024-01-02T10:00:00Z".parse::<Timestamp>().unwrap();
        let result = calculate_duration_segments(None, Some(end), None);

        assert_eq!(result, vec![(end, IBDuration::days(1))]);
    }

    #[rstest]
    fn test_calculate_duration_segments_with_explicit_duration() {
        let end = "2024-01-02T10:00:00Z".parse::<Timestamp>().unwrap();
        let result = calculate_duration_segments(None, Some(end), Some(IBDuration::weeks(2)));

        assert_eq!(result, vec![(end, IBDuration::weeks(2))]);
    }

    #[rstest]
    fn test_calculate_duration_segments_splits_years_days_and_seconds() {
        let end = "2025-07-03T10:00:01Z".parse::<Timestamp>().unwrap();
        let start = "2024-01-01T10:00:00Z".parse::<Timestamp>().unwrap();
        let segments = calculate_duration_segments(Some(start), Some(end), None);

        assert_eq!(
            segments,
            vec![
                (end, IBDuration::years(1)),
                (
                    "2024-07-03T10:00:01Z".parse::<Timestamp>().unwrap(),
                    IBDuration::days(184),
                ),
                (
                    "2024-01-01T10:00:01Z".parse::<Timestamp>().unwrap(),
                    IBDuration::seconds(1),
                ),
            ]
        );
    }

    #[rstest]
    fn test_calculate_duration_segments_rounds_subsecond_up() {
        let start = Timestamp::new(1, 0).unwrap();
        let end = Timestamp::new(1, 1).unwrap();
        let segments = calculate_duration_segments(Some(start), Some(end), None);

        assert_eq!(segments, vec![(end, IBDuration::seconds(1))]);
    }

    #[rstest]
    #[case(None, Some(Timestamp::from_second(2).unwrap()), 0, Some(10), true)]
    #[case(Some(Timestamp::from_second(1).unwrap()), Some(Timestamp::from_second(2).unwrap()), 0, Some(10), true)]
    #[case(Some(Timestamp::from_second(2).unwrap()), Some(Timestamp::from_second(1).unwrap()), 0, Some(10), false)]
    #[case(Some(Timestamp::from_second(1).unwrap()), Some(Timestamp::from_second(2).unwrap()), 10, Some(10), false)]
    #[case(Some(Timestamp::from_second(1).unwrap()), Some(Timestamp::from_second(2).unwrap()), 10, None, true)]
    fn test_should_continue_historical_tick_pagination(
        #[case] start: Option<Timestamp>,
        #[case] end: Option<Timestamp>,
        #[case] current_len: usize,
        #[case] limit: Option<usize>,
        #[case] expected: bool,
    ) {
        assert_eq!(
            should_continue_historical_tick_pagination(start, end, current_len, limit),
            expected
        );
    }

    #[rstest]
    fn test_extend_historical_tick_batch_retreats_one_nanosecond() {
        let mut ticks = vec![UnixNanos::from(30)];
        let mut end = Some(Timestamp::from_nanosecond(100).unwrap());

        let should_continue = extend_historical_tick_batch(
            &mut ticks,
            vec![UnixNanos::from(20), UnixNanos::from(10)],
            Some(Timestamp::from_nanosecond(1).unwrap()),
            &mut end,
            Some(UnixNanos::from(1)),
            Some(UnixNanos::from(100)),
            Some(10),
            |ts| *ts,
        );

        assert!(should_continue);
        assert_eq!(
            ticks,
            vec![
                UnixNanos::from(30),
                UnixNanos::from(20),
                UnixNanos::from(10)
            ]
        );
        assert_eq!(end, Some(Timestamp::from_nanosecond(9).unwrap()));
    }

    #[rstest]
    fn test_extend_historical_tick_batch_stops_and_trims_at_start() {
        let mut ticks = Vec::new();
        let mut end = Some(Timestamp::from_nanosecond(100).unwrap());

        let should_continue = extend_historical_tick_batch(
            &mut ticks,
            vec![UnixNanos::from(20), UnixNanos::from(10)],
            Some(Timestamp::from_nanosecond(10).unwrap()),
            &mut end,
            Some(UnixNanos::from(10)),
            Some(UnixNanos::from(15)),
            Some(10),
            |ts| *ts,
        );

        assert!(!should_continue);
        assert_eq!(ticks, vec![UnixNanos::from(10)]);
        assert_eq!(end, Some(Timestamp::from_nanosecond(9).unwrap()));
    }

    #[rstest]
    fn test_extend_historical_tick_batch_drops_boundary_duplicates() {
        let mut ticks = vec![UnixNanos::from(30)];
        let mut end = Some(Timestamp::from_nanosecond(29).unwrap());

        let should_continue = extend_historical_tick_batch(
            &mut ticks,
            vec![
                UnixNanos::from(35),
                UnixNanos::from(30),
                UnixNanos::from(20),
            ],
            None,
            &mut end,
            None,
            None,
            Some(10),
            |ts| *ts,
        );

        assert!(should_continue);
        assert_eq!(ticks, vec![UnixNanos::from(30), UnixNanos::from(20)]);
        assert_eq!(end, Some(Timestamp::from_nanosecond(19).unwrap()));
    }

    #[rstest]
    fn test_extend_historical_tick_batch_stops_when_end_cannot_retreat() {
        let mut ticks = vec![UnixNanos::from(30)];
        let mut end = Some(Timestamp::from_nanosecond(29).unwrap());

        let should_continue = extend_historical_tick_batch(
            &mut ticks,
            vec![UnixNanos::from(35), UnixNanos::from(30)],
            None,
            &mut end,
            None,
            None,
            Some(10),
            |ts| *ts,
        );

        assert!(!should_continue);
        assert_eq!(ticks, vec![UnixNanos::from(30)]);
        assert_eq!(end, Some(Timestamp::from_nanosecond(29).unwrap()));
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

    fn create_equity() -> InstrumentAny {
        let equity = Equity::builder()
            .instrument_id(create_test_instrument_id())
            .raw_symbol(Symbol::from("AAPL"))
            .currency(Currency::from("USD"))
            .price_precision(2)
            .price_increment(Price::from("0.05"))
            .lot_size(Quantity::from("1"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap();
        InstrumentAny::from(equity)
    }

    #[rstest]
    #[case(1.12097, "1.12095")]
    #[case(1.12098, "1.12100")]
    #[case(1.12100, "1.12100")]
    #[case(1.12095, "1.12095")]
    #[case(-1.12097, "-1.12095")]
    fn test_price_converter_rounds_fx_pair_to_increment(
        #[case] ib_price: f64,
        #[case] expected: &str,
    ) {
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        let price = prices.price(ib_price).unwrap();

        assert_eq!(price, Price::from(expected));
        assert_eq!(price.precision, 5);
    }

    #[rstest]
    #[case(1e25)]
    #[case(-1e25)]
    #[case(7.9e28)]
    #[case(1e300)]
    fn test_price_converter_returns_error_for_out_of_range_price(#[case] ib_price: f64) {
        let fx = IbPriceConverter::for_instrument(&currency_pair("EUR", "USD", 5, "0.00005"), 1);
        let magnified = IbPriceConverter::new(4).with_price_magnifier(100);

        assert!(fx.price(ib_price).is_err());
        assert!(fx.bid_price(ib_price).is_err());
        assert!(fx.ask_price(ib_price).is_err());
        assert!(magnified.price(ib_price).is_err());
    }

    #[rstest]
    fn test_price_converter_rounds_jpy_pair_to_increment() {
        let instrument = currency_pair("USD", "JPY", 3, "0.005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        assert_eq!(prices.price(151.234).unwrap(), Price::from("151.235"));
        assert_eq!(prices.price(151.232).unwrap(), Price::from("151.230"));
    }

    #[rstest]
    #[case(1.000025, "1.00000")]
    #[case(1.000075, "1.00010")]
    #[case(1.000125, "1.00010")]
    #[case(1.000026, "1.00005")]
    #[case(1.120025, "1.12000")]
    #[case(1.120175, "1.12020")]
    fn test_price_converter_ties_go_to_even_multiple(
        #[case] ib_price: f64,
        #[case] expected: &str,
    ) {
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        let price = prices.price(ib_price).unwrap();

        assert_eq!(price, Price::from(expected));
        assert_eq!(price.precision, 5);
    }

    #[rstest]
    #[case(151.0025, "151.000")]
    #[case(151.0075, "151.010")]
    #[case(151.0125, "151.010")]
    fn test_price_converter_ties_go_to_even_multiple_for_jpy_pair(
        #[case] ib_price: f64,
        #[case] expected: &str,
    ) {
        let instrument = currency_pair("USD", "JPY", 3, "0.005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        assert_eq!(prices.price(ib_price).unwrap(), Price::from(expected));
    }

    #[rstest]
    fn test_price_converter_rounds_every_fx_midpoint_like_exact_ties_to_even() {
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        // IB prices in steps of 0.000005 around 1.12, half of the 0.00005 increment
        for n in 223_850_i64..=224_150 {
            let micros = n * 5;
            let ib_price: f64 = format!("{}.{:06}", micros / 1_000_000, micros % 1_000_000)
                .parse()
                .unwrap();

            // The expectation is in integers: n steps of 5 micros against a step of 10 micros
            let (quotient, remainder) = (n / 10, n % 10);
            let steps = match remainder.cmp(&5) {
                Ordering::Less => quotient,
                Ordering::Greater => quotient + 1,
                Ordering::Equal => quotient + quotient % 2,
            };
            let hundred_thousandths = steps * 5;
            let expected = Price::from(
                format!(
                    "{}.{:05}",
                    hundred_thousandths / 100_000,
                    hundred_thousandths % 100_000
                )
                .as_str(),
            );

            assert_eq!(
                prices.price(ib_price).unwrap(),
                expected,
                "ib_price={ib_price}"
            );
        }
    }

    #[rstest]
    fn test_price_converter_does_not_round_crypto_pair() {
        let instrument = currency_pair("BTC", "USD", 2, "0.05");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        assert_eq!(prices.price(100.03).unwrap(), Price::from("100.03"));
    }

    #[rstest]
    fn test_price_converter_does_not_round_equity() {
        let instrument = create_equity();
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        assert_eq!(prices.precision(), 2);
        assert_eq!(prices.price(100.03).unwrap(), Price::from("100.03"));
    }

    #[rstest]
    fn test_price_converter_new_applies_precision_only() {
        let prices = IbPriceConverter::new(5);

        assert_eq!(prices.precision(), 5);
        assert_eq!(prices.price(1.12097).unwrap(), Price::from("1.12097"));
    }

    #[rstest]
    fn test_price_converter_rejects_non_finite_price() {
        let prices = IbPriceConverter::new(2);

        assert!(prices.price(f64::NAN).is_err());
    }

    #[rstest]
    fn test_ib_bar_to_nautilus_bar_rounds_fx_prices_to_increment() {
        let ib_bar = ibapi::market_data::historical::Bar {
            date: datetime!(2024-01-01 10:00:00 UTC).into(),
            open: 1.12097,
            high: 1.12103,
            low: 1.12091,
            close: 1.12098,
            volume: 0.0,
            wap: 0.0,
            count: 0,
        };
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid),
            AggregationSource::External,
        );

        let bar = ib_bar_to_nautilus_bar(
            &ib_bar,
            bar_type,
            IbPriceConverter::for_instrument(&instrument, 1),
            0,
        )
        .unwrap();

        assert_eq!(bar.open, Price::from("1.12095"));
        assert_eq!(bar.high, Price::from("1.12105"));
        assert_eq!(bar.low, Price::from("1.12090"));
        assert_eq!(bar.close, Price::from("1.12100"));
    }

    #[rstest]
    #[case(1.12098, "1.12095", "1.12100")]
    #[case(1.12099, "1.12095", "1.12100")]
    #[case(1.12097, "1.12095", "1.12100")]
    #[case(1.12095, "1.12095", "1.12095")]
    #[case(-1.12097, "-1.12100", "-1.12095")]
    fn test_price_converter_rounds_bid_down_and_ask_up(
        #[case] ib_price: f64,
        #[case] bid: &str,
        #[case] ask: &str,
    ) {
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        assert_eq!(prices.bid_price(ib_price).unwrap(), Price::from(bid));
        assert_eq!(prices.ask_price(ib_price).unwrap(), Price::from(ask));
    }

    #[rstest]
    fn test_price_converter_rounds_bid_down_and_ask_up_for_jpy_pair() {
        let instrument = currency_pair("USD", "JPY", 3, "0.005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        assert_eq!(prices.bid_price(151.0049).unwrap(), Price::from("151.000"));
        assert_eq!(prices.ask_price(151.0001).unwrap(), Price::from("151.005"));
    }

    #[rstest]
    #[case(PriceType::Bid, "1.12095")]
    #[case(PriceType::Ask, "1.12100")]
    #[case(PriceType::Mid, "1.12095")]
    #[case(PriceType::Last, "1.12095")]
    fn test_price_converter_bar_price_follows_price_type(
        #[case] price_type: PriceType,
        #[case] expected: &str,
    ) {
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        assert_eq!(
            prices.bar_price(price_type, 1.12097).unwrap(),
            Price::from(expected)
        );
    }

    #[rstest]
    fn test_price_converter_bid_and_ask_do_not_round_equity() {
        let instrument = create_equity();
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        assert_eq!(prices.bid_price(100.03).unwrap(), Price::from("100.03"));
        assert_eq!(prices.ask_price(100.03).unwrap(), Price::from("100.03"));
    }

    #[rstest]
    #[case(PriceType::Bid, "1.12095", "1.12105", "1.12090", "1.12095")]
    #[case(PriceType::Ask, "1.12100", "1.12110", "1.12095", "1.12100")]
    fn test_ib_bar_to_nautilus_bar_rounds_fx_bid_and_ask_bars(
        #[case] price_type: PriceType,
        #[case] open: &str,
        #[case] high: &str,
        #[case] low: &str,
        #[case] close: &str,
    ) {
        let ib_bar = ibapi::market_data::historical::Bar {
            date: datetime!(2024-01-01 10:00:00 UTC).into(),
            open: 1.12097,
            high: 1.12109,
            low: 1.12091,
            close: 1.12096,
            volume: 0.0,
            wap: 0.0,
            count: 0,
        };
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(1, BarAggregation::Minute, price_type),
            AggregationSource::External,
        );

        let bar = ib_bar_to_nautilus_bar(
            &ib_bar,
            bar_type,
            IbPriceConverter::for_instrument(&instrument, 1),
            0,
        )
        .unwrap();

        assert_eq!(bar.open, Price::from(open));
        assert_eq!(bar.high, Price::from(high));
        assert_eq!(bar.low, Price::from(low));
        assert_eq!(bar.close, Price::from(close));
        assert!(bar.high >= bar.low);
    }

    #[rstest]
    #[case(425.25, "4.2525")]
    #[case(486.25, "4.8625")]
    #[case(-100.0, "-1.0000")]
    fn test_price_converter_divides_by_price_magnifier(
        #[case] ib_price: f64,
        #[case] expected: &str,
    ) {
        let prices = IbPriceConverter::new(4).with_price_magnifier(100);

        assert_eq!(prices.price(ib_price).unwrap(), Price::from(expected));
    }

    #[rstest]
    fn test_price_converter_converts_magnified_quarter_cent_steps_exactly() {
        let prices = IbPriceConverter::new(4).with_price_magnifier(100);

        // A quarter-cent step of 0.25 cents is 0.0025 in the contract's currency
        for n in 0_i64..=2000 {
            let ib_price = n as f64 * 0.25;
            let ten_thousandths = n * 25;
            let expected = Price::from(
                format!(
                    "{}.{:04}",
                    ten_thousandths / 10_000,
                    ten_thousandths % 10_000
                )
                .as_str(),
            );

            assert_eq!(
                prices.price(ib_price).unwrap(),
                expected,
                "ib_price={ib_price}"
            );
        }
    }

    #[rstest]
    #[case(0)]
    #[case(-5)]
    #[case(1)]
    fn test_price_converter_treats_magnifier_below_one_as_one(#[case] magnifier: i32) {
        let prices = IbPriceConverter::new(2).with_price_magnifier(magnifier);

        assert_eq!(prices.price(150.25).unwrap(), Price::from("150.25"));
    }

    #[rstest]
    fn test_price_converter_divides_before_rounding_fx_to_increment() {
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let prices = IbPriceConverter::for_instrument(&instrument, 100);

        assert_eq!(prices.price(112.097).unwrap(), Price::from("1.12095"));
    }

    #[rstest]
    fn test_ib_bar_to_nautilus_bar_divides_magnified_prices() {
        let ib_bar = ibapi::market_data::historical::Bar {
            date: datetime!(2024-01-01 10:00:00 UTC).into(),
            open: 425.25,
            high: 430.0,
            low: 424.5,
            close: 428.5,
            volume: 10.0,
            wap: 0.0,
            count: 1,
        };
        let bar_type = BarType::new(
            create_test_instrument_id(),
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );

        let bar = ib_bar_to_nautilus_bar(
            &ib_bar,
            bar_type,
            IbPriceConverter::new(4).with_price_magnifier(100),
            0,
        )
        .unwrap();

        assert_eq!(bar.open, Price::from("4.2525"));
        assert_eq!(bar.high, Price::from("4.3000"));
        assert_eq!(bar.low, Price::from("4.2450"));
        assert_eq!(bar.close, Price::from("4.2850"));
    }
}
