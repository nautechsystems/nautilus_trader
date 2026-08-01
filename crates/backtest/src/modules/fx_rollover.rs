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

//! FX rollover interest simulation module.

use std::cell::{Cell, RefCell};

use ahash::{AHashMap, AHashSet};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use chrono_tz::US::Eastern;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::Data,
    enums::{AssetClass, PriceType},
    identifiers::InstrumentId,
    instruments::Instrument,
    types::{Currency, Money},
};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use super::{
    AccountAdjustmentError, AccountAdjustmentOutcome, ExchangeContext, SimulationModule,
    SimulationModuleResult,
};

const LOCATION_CURRENCY_MAP: &[(&str, &str)] = &[
    ("AUS", "AUD"),
    ("CAN", "CAD"),
    ("CHE", "CHF"),
    ("EA19", "EUR"),
    ("USA", "USD"),
    ("JPN", "JPY"),
    ("NZL", "NZD"),
    ("GBR", "GBP"),
    ("RUS", "RUB"),
    ("NOR", "NOK"),
    ("CHN", "CNY"),
    ("MEX", "MXN"),
    ("ZAF", "ZAR"),
];

/// A single interest rate data entry.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.backtest", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
pub struct InterestRateRecord {
    /// OECD location code using ISO 3166 alpha-3 (e.g., "AUS", "USA") or "EA19".
    /// Records with unsupported codes are ignored.
    pub location: String,
    /// Time period key (e.g., "2024-01" for monthly, "2024-Q1" for quarterly).
    pub time: String,
    /// Interest rate value as a percentage (e.g., 5.25 means 5.25%). Must be finite.
    pub value: f64,
}

impl InterestRateRecord {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.value.is_finite(),
            "Interest rate for location '{}' at '{}' must be finite, was {}",
            self.location,
            self.time,
            self.value
        );
        Ok(())
    }
}

/// Calculates overnight rollover interest rates for FX currency pairs.
///
/// Uses short-term interest rate data (OECD format) to compute the daily
/// differential between base and quote currency rates.
#[derive(Debug, Clone)]
pub struct RolloverInterestCalculator {
    // currency code -> {time_key -> rate_percentage}
    rates: AHashMap<String, AHashMap<String, f64>>,
}

impl RolloverInterestCalculator {
    /// Creates a new calculator from interest rate records.
    ///
    /// Records with unsupported location codes are ignored. "CHN" supplies both CNY and CNH;
    /// later records replace earlier records for the same currency and time.
    ///
    /// # Errors
    ///
    /// Returns an error if any interest rate is not finite.
    pub fn new(records: Vec<InterestRateRecord>) -> anyhow::Result<Self> {
        let location_to_currency: AHashMap<&str, &str> =
            LOCATION_CURRENCY_MAP.iter().copied().collect();

        let mut rates: AHashMap<String, AHashMap<String, f64>> = AHashMap::new();

        for record in records {
            record.validate()?;

            // CHN maps to both CNY and CNH
            if record.location == "CHN" {
                rates
                    .entry("CNH".to_string())
                    .or_default()
                    .insert(record.time.clone(), record.value);
            }

            if let Some(&currency) = location_to_currency.get(record.location.as_str()) {
                rates
                    .entry(currency.to_string())
                    .or_default()
                    .insert(record.time, record.value);
            }
        }

        Ok(Self { rates })
    }

    /// Calculates the overnight interest rate differential for a currency pair.
    ///
    /// Returns `(base_rate - quote_rate) / 365 / 100` as a daily decimal rate.
    ///
    /// # Errors
    ///
    /// Returns an error if rate data is missing for either currency.
    pub fn calc_overnight_rate(
        &self,
        instrument_id: InstrumentId,
        date: NaiveDate,
    ) -> anyhow::Result<f64> {
        let symbol = instrument_id.symbol.as_str();
        if symbol.len() < 6 {
            anyhow::bail!("FX symbol must be at least 6 characters: {symbol}");
        }

        let base_currency = &symbol[..3];
        let quote_currency = &symbol[symbol.len() - 3..];

        let base_rate = self.lookup_rate(base_currency, date)?;
        let quote_rate = self.lookup_rate(quote_currency, date)?;

        Ok((base_rate - quote_rate) / 365.0 / 100.0)
    }

    fn lookup_rate(&self, currency: &str, date: NaiveDate) -> anyhow::Result<f64> {
        let currency_rates = self
            .rates
            .get(currency)
            .ok_or_else(|| anyhow::anyhow!("No rate data for currency {currency}"))?;

        // Try monthly key first
        let monthly_key = format!("{}-{:02}", date.year(), date.month());
        if let Some(&rate) = currency_rates.get(&monthly_key) {
            return Ok(rate);
        }

        // Fall back to quarterly key
        let quarter = (date.month() - 1) / 3 + 1;
        let quarterly_key = format!("{}-Q{quarter}", date.year());
        if let Some(&rate) = currency_rates.get(&quarterly_key) {
            return Ok(rate);
        }

        anyhow::bail!("No rate data for {currency} at {monthly_key} or {quarterly_key}")
    }
}

/// Simulates FX rollover (swap) interest applied at 5 PM US/Eastern daily.
///
/// When holding FX positions overnight, the interest rate differential
/// between the two currencies is credited or debited. Wednesday and Friday
/// rollovers are tripled (Wednesday for T+2 settlement, Friday for the weekend).
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.backtest",
        unsendable,
        skip_from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
pub struct FXRolloverInterestModule {
    calculator: RolloverInterestCalculator,
    rollover_completed: Cell<bool>,
    rollover_day: RefCell<Option<RolloverDayState>>,
    rollover_totals: RefCell<AHashMap<Currency, f64>>,
    unapplied_rollover_totals: RefCell<AHashMap<Currency, f64>>,
}

#[derive(Debug, Clone)]
struct RolloverDayState {
    date: NaiveDate,
    warned_failures: AHashSet<(NaiveDate, InstrumentId, RolloverFailureKind)>,
    warned_adjustment_failures: AHashSet<(NaiveDate, Currency, AccountAdjustmentFailureKind)>,
    pending_adjustments: Option<Vec<RolloverAdjustment>>,
    pending_end_date: Option<NaiveDate>,
    attempt_time: Option<UnixNanos>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RolloverAdjustment {
    booking_date: NaiveDate,
    amount: Money,
}

enum RolloverCalculationOutcome {
    Completed(Vec<Money>),
    Retry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RolloverFailureDisposition {
    RetryDay,
    SkipInstrument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccountAdjustmentFailureDisposition {
    Retry,
    RecordUnapplied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RolloverFailureKind {
    Engine,
    Money,
    Price,
    Rate,
    Xrate,
}

impl RolloverFailureKind {
    const fn disposition(self) -> RolloverFailureDisposition {
        match self {
            Self::Engine | Self::Price | Self::Xrate => RolloverFailureDisposition::RetryDay,
            Self::Money | Self::Rate => RolloverFailureDisposition::SkipInstrument,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AccountAdjustmentFailureKind {
    TotalOverflow,
    FreeBalanceOverflow,
    MissingBalance,
    MissingAccount,
    AccountStateGeneration,
}

impl From<&AccountAdjustmentError> for AccountAdjustmentFailureKind {
    fn from(error: &AccountAdjustmentError) -> Self {
        match error {
            AccountAdjustmentError::TotalOverflow(_) => Self::TotalOverflow,
            AccountAdjustmentError::FreeBalanceOverflow(_) => Self::FreeBalanceOverflow,
            AccountAdjustmentError::MissingBalance(_) => Self::MissingBalance,
            AccountAdjustmentError::MissingAccount(_) => Self::MissingAccount,
            AccountAdjustmentError::AccountStateGeneration(_) => Self::AccountStateGeneration,
        }
    }
}

impl AccountAdjustmentFailureKind {
    const fn disposition(self) -> AccountAdjustmentFailureDisposition {
        match self {
            Self::TotalOverflow | Self::FreeBalanceOverflow | Self::AccountStateGeneration => {
                AccountAdjustmentFailureDisposition::Retry
            }
            Self::MissingBalance | Self::MissingAccount => {
                AccountAdjustmentFailureDisposition::RecordUnapplied
            }
        }
    }
}

impl FXRolloverInterestModule {
    /// Creates a new FX rollover interest module.
    ///
    /// Records with unsupported location codes are ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if any interest rate is not finite.
    pub fn new(records: Vec<InterestRateRecord>) -> anyhow::Result<Self> {
        Ok(Self {
            calculator: RolloverInterestCalculator::new(records)?,
            rollover_completed: Cell::new(false),
            rollover_day: RefCell::new(None),
            rollover_totals: RefCell::new(AHashMap::new()),
            unapplied_rollover_totals: RefCell::new(AHashMap::new()),
        })
    }

    fn initialize_rollover_day(&self, date: NaiveDate) {
        self.rollover_day.replace(Some(RolloverDayState {
            date,
            warned_failures: AHashSet::new(),
            warned_adjustment_failures: AHashSet::new(),
            pending_adjustments: None,
            pending_end_date: None,
            attempt_time: None,
        }));
        self.rollover_completed.set(false);
    }

    fn rollover_time_ns(date: NaiveDate) -> u64 {
        let rollover_eastern =
            date.and_time(NaiveTime::from_hms_opt(17, 0, 0).expect("valid rollover time"));
        Eastern
            .from_local_datetime(&rollover_eastern)
            .single()
            .expect("unambiguous rollover time")
            .timestamp_nanos_opt()
            .expect("rollover timestamp in range")
            .cast_unsigned()
    }

    fn weekday_on_or_before(mut date: NaiveDate) -> NaiveDate {
        while date.weekday().number_from_monday() > 5 {
            date = date.pred_opt().expect("previous rollover date in range");
        }
        date
    }

    fn next_weekday(mut date: NaiveDate) -> NaiveDate {
        loop {
            date = date.succ_opt().expect("next rollover date in range");
            if date.weekday().number_from_monday() <= 5 {
                return date;
            }
        }
    }

    /// Logs a calculation failure at warn level once per (booking date, instrument,
    /// kind), demoting repeats to debug: a `Retry` outcome re-runs the calculation
    /// on every process call until it completes, and repeating the identical
    /// warning per attempt would flood the log. The booking date is part of the key
    /// because one catch-up batch spans many dates, and a permanent per-instrument
    /// skip must stay visible for each date it drops rather than warning only for
    /// the first. The set is cleared on a new day, on completion, and on reset.
    fn log_calculation_failure(
        &self,
        booking_date: NaiveDate,
        instrument_id: InstrumentId,
        kind: RolloverFailureKind,
        message: &str,
    ) {
        let first_failure = self
            .rollover_day
            .borrow_mut()
            .as_mut()
            .expect("rollover day initialized")
            .warned_failures
            .insert((booking_date, instrument_id, kind));

        if first_failure {
            log::warn!("{message}");
        } else {
            log::debug!("{message}");
        }
    }

    fn calculate_rollover_interest(
        &self,
        date: NaiveDate,
        iso_weekday: u32,
        ctx: &ExchangeContext,
    ) -> RolloverCalculationOutcome {
        let mut instrument_ids = ctx.instruments.keys().copied().collect::<Vec<_>>();
        instrument_ids.sort_unstable();
        let mut adjustments = Vec::new();

        for instrument_id in instrument_ids {
            let instrument = &ctx.instruments[&instrument_id];

            if instrument.asset_class() != AssetClass::FX {
                continue;
            }

            let positions =
                ctx.cache
                    .positions_open(Some(&ctx.venue), Some(&instrument_id), None, None, None);

            if positions.is_empty() {
                continue;
            }

            // Look up the immutable rate data before any transient market
            // inputs: a permanently missing rate must skip the instrument
            // even when the engine or price would first retry the day.
            let interest_rate = match self.calculator.calc_overnight_rate(instrument_id, date) {
                Ok(rate) => rate,
                Err(e) => {
                    let kind = RolloverFailureKind::Rate;
                    self.log_calculation_failure(
                        date,
                        instrument_id,
                        kind,
                        &format!("Skipping rollover for {instrument_id} on {date}: {e}"),
                    );

                    match kind.disposition() {
                        RolloverFailureDisposition::RetryDay => {
                            return RolloverCalculationOutcome::Retry;
                        }
                        RolloverFailureDisposition::SkipInstrument => continue,
                    }
                }
            };

            let Some(matching_engine) = ctx.matching_engines.get(&instrument_id) else {
                self.log_calculation_failure(
                    date,
                    instrument_id,
                    RolloverFailureKind::Engine,
                    &format!("Cannot calculate rollover for {instrument_id}: no matching engine"),
                );
                return RolloverCalculationOutcome::Retry;
            };
            let book = matching_engine.get_book();
            let mid = if let Some(mid) = book.midpoint() {
                mid
            } else if let Some(price) = book.best_bid_price() {
                price.as_f64()
            } else if let Some(price) = book.best_ask_price() {
                price.as_f64()
            } else {
                self.log_calculation_failure(
                    date,
                    instrument_id,
                    RolloverFailureKind::Price,
                    &format!("Cannot calculate rollover for {instrument_id}: no market price"),
                );
                return RolloverCalculationOutcome::Retry;
            };

            let net_qty: f64 = positions.iter().map(|p| p.signed_qty).sum();

            let mut rollover = net_qty * mid * interest_rate;

            // Triple for Wednesday (T+2 settlement) and Friday (weekend)
            if iso_weekday == 3 || iso_weekday == 5 {
                rollover *= 3.0;
            }

            let currency = if let Some(base) = ctx.base_currency {
                // Rollover math is still f64; convert the Decimal rate at the boundary
                let xrate_result = ctx.cache.try_get_xrate(
                    ctx.venue,
                    instrument.quote_currency(),
                    base,
                    PriceType::Mid,
                );
                let xrate = match xrate_result {
                    Ok(Some(rate)) => rate.to_f64(),
                    Ok(None) => None,
                    Err(e) => {
                        self.log_calculation_failure(
                            date,
                            instrument_id,
                            RolloverFailureKind::Xrate,
                            &format!(
                                "Cannot calculate rollover for {instrument_id}: exchange rate from {} to {base}: {e}",
                                instrument.quote_currency()
                            ),
                        );
                        return RolloverCalculationOutcome::Retry;
                    }
                };
                let Some(xrate) = xrate else {
                    self.log_calculation_failure(
                        date,
                        instrument_id,
                        RolloverFailureKind::Xrate,
                        &format!(
                            "Cannot calculate rollover for {instrument_id}: no exchange rate from {} to {base}",
                            instrument.quote_currency()
                        ),
                    );
                    return RolloverCalculationOutcome::Retry;
                };
                rollover *= xrate;
                base
            } else {
                instrument.quote_currency()
            };

            let adjustment = match Money::new_checked(rollover, currency) {
                Ok(adjustment) => adjustment,
                Err(e) => {
                    let kind = RolloverFailureKind::Money;
                    self.log_calculation_failure(
                        date,
                        instrument_id,
                        kind,
                        &format!(
                            "Skipping rollover for {instrument_id} on {date}: invalid adjustment: {e}"
                        ),
                    );

                    match kind.disposition() {
                        RolloverFailureDisposition::RetryDay => {
                            return RolloverCalculationOutcome::Retry;
                        }
                        RolloverFailureDisposition::SkipInstrument => continue,
                    }
                }
            };

            adjustments.push(adjustment);
        }

        RolloverCalculationOutcome::Completed(adjustments)
    }
}

impl SimulationModule for FXRolloverInterestModule {
    fn pre_process(&self, _data: &Data) {}

    fn process(&self, ts_now: UnixNanos, ctx: &ExchangeContext) -> SimulationModuleResult {
        let utc_dt = nanos_to_utc_datetime(ts_now);
        let eastern_dt = Eastern.from_utc_datetime(&utc_dt);
        let observed_date = eastern_dt.date_naive();

        let initialize_date = {
            let day = self.rollover_day.borrow();
            match day.as_ref() {
                None => Some(Self::weekday_on_or_before(observed_date)),
                Some(day) if self.rollover_completed.get() && day.date < observed_date => {
                    Some(Self::next_weekday(day.date))
                }
                Some(_) => None,
            }
        };

        if let Some(date) = initialize_date {
            self.initialize_rollover_day(date);
        }

        if self.rollover_completed.get() {
            return SimulationModuleResult::NotReady;
        }

        {
            let mut day = self.rollover_day.borrow_mut();
            let day = day.as_mut().expect("rollover day initialized");
            if let Some(adjustments) = &day.pending_adjustments {
                let adjustments = adjustments
                    .iter()
                    .map(|adjustment| adjustment.amount)
                    .collect();
                day.attempt_time = Some(ts_now);
                return SimulationModuleResult::Completed(adjustments);
            }
        }

        let date = {
            let day = self.rollover_day.borrow();
            let day = day.as_ref().expect("rollover day initialized");
            day.date
        };

        if ts_now.as_u64() < Self::rollover_time_ns(date) {
            return SimulationModuleResult::NotReady;
        }

        // Drain every due weekday so sparse data cannot leave the booking cursor behind.
        // This is complete by booked-day count, but uses the current positions, prices, and
        // exchange rates for every date because historical cutoff snapshots are unavailable.
        // Work is proportional to the gap length times the instrument count. This is a weekday
        // calendar rather than a pair-specific business-day calendar. The existing Wednesday
        // and Friday triple multipliers are retained for parity, even though standard spot FX
        // usually applies the weekend triple on Wednesday only.
        let mut booking_date = date;
        let mut batch = Vec::new();
        let batch_end_date = loop {
            if booking_date > observed_date
                || (booking_date == observed_date
                    && ts_now.as_u64() < Self::rollover_time_ns(booking_date))
            {
                return SimulationModuleResult::NotReady;
            }

            let iso_weekday = booking_date.weekday().number_from_monday();
            match self.calculate_rollover_interest(booking_date, iso_weekday, ctx) {
                RolloverCalculationOutcome::Completed(adjustments) => {
                    batch.extend(adjustments.into_iter().map(|amount| RolloverAdjustment {
                        booking_date,
                        amount,
                    }));
                }
                RolloverCalculationOutcome::Retry => return SimulationModuleResult::NotReady,
            }

            let next = Self::next_weekday(booking_date);
            if next > observed_date
                || (next == observed_date && ts_now.as_u64() < Self::rollover_time_ns(next))
            {
                break booking_date;
            }
            booking_date = next;
        };

        let adjustments = batch.iter().map(|adjustment| adjustment.amount).collect();
        let mut day = self.rollover_day.borrow_mut();
        let day = day.as_mut().expect("rollover day initialized");
        day.pending_adjustments = Some(batch);
        day.pending_end_date = Some(batch_end_date);
        day.attempt_time = Some(ts_now);
        SimulationModuleResult::Completed(adjustments)
    }

    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]) {
        let (adjustments, attempt_time, batch_end_date) = {
            let mut day = self.rollover_day.borrow_mut();
            let day = day.as_mut().expect("rollover day initialized");
            let adjustment_count = day
                .pending_adjustments
                .as_ref()
                .expect("no completed rollover batch to acknowledge")
                .len();
            assert_eq!(
                outcomes.len(),
                adjustment_count,
                "rollover acknowledgement count must match adjustment count"
            );
            let adjustments = day
                .pending_adjustments
                .take()
                .expect("no completed rollover batch to acknowledge");
            (
                adjustments,
                day.attempt_time
                    .take()
                    .expect("rollover attempt time recorded"),
                day.pending_end_date
                    .expect("rollover batch end date recorded"),
            )
        };

        let mut failed = Vec::new();
        {
            let mut totals = self.rollover_totals.borrow_mut();
            let mut unapplied_totals = self.unapplied_rollover_totals.borrow_mut();

            for (adjustment, outcome) in adjustments.into_iter().zip(outcomes) {
                match outcome {
                    AccountAdjustmentOutcome::Applied => {
                        let total = totals.entry(adjustment.amount.currency).or_insert(0.0);
                        *total += adjustment.amount.as_f64();
                    }
                    AccountAdjustmentOutcome::Failed(error) => {
                        let kind = AccountAdjustmentFailureKind::from(error);
                        let first_failure = self
                            .rollover_day
                            .borrow_mut()
                            .as_mut()
                            .expect("rollover day initialized")
                            .warned_adjustment_failures
                            .insert((adjustment.booking_date, adjustment.amount.currency, kind));

                        match kind.disposition() {
                            AccountAdjustmentFailureDisposition::Retry => {
                                if first_failure {
                                    log::warn!(
                                        "Cannot apply rollover adjustment for {} on {}: {error}",
                                        adjustment.amount.currency,
                                        adjustment.booking_date
                                    );
                                } else {
                                    log::debug!(
                                        "Cannot apply rollover adjustment for {} on {}: {error}",
                                        adjustment.amount.currency,
                                        adjustment.booking_date
                                    );
                                }
                                failed.push(adjustment);
                            }
                            AccountAdjustmentFailureDisposition::RecordUnapplied => {
                                if first_failure {
                                    log::warn!(
                                        "Rollover adjustment for {} on {} failed with {kind:?} and is recorded as unapplied: {error}",
                                        adjustment.amount,
                                        adjustment.booking_date
                                    );
                                } else {
                                    log::debug!(
                                        "Rollover adjustment for {} on {} failed with {kind:?} and is recorded as unapplied: {error}",
                                        adjustment.amount,
                                        adjustment.booking_date
                                    );
                                }
                                let total = unapplied_totals
                                    .entry(adjustment.amount.currency)
                                    .or_insert(0.0);
                                *total += adjustment.amount.as_f64();
                            }
                        }
                    }
                }
            }
        }

        if failed.is_empty() {
            self.rollover_completed.set(true);
            let mut day = self.rollover_day.borrow_mut();
            let day = day.as_mut().expect("rollover day initialized");
            day.date = batch_end_date;
            day.pending_end_date = None;
            day.warned_failures.clear();

            let attempt_eastern = Eastern.from_utc_datetime(&nanos_to_utc_datetime(attempt_time));

            if attempt_eastern.date_naive() != batch_end_date {
                log::warn!(
                    "Rollover batch through {batch_end_date}, scheduled through {}, booked late at {attempt_time}",
                    UnixNanos::from(Self::rollover_time_ns(batch_end_date))
                );
            }
        } else {
            self.rollover_day
                .borrow_mut()
                .as_mut()
                .expect("rollover day initialized")
                .pending_adjustments = Some(failed);
        }
    }

    fn log_diagnostics(&self) {
        let totals = self.rollover_totals.borrow();
        let parts: Vec<String> = totals
            .iter()
            .filter_map(|(currency, total)| {
                Money::new_checked(*total, *currency)
                    .map(|money| money.to_string())
                    .map_err(|e| {
                        log::error!("Cannot report rollover total for {currency}: {e}");
                    })
                    .ok()
            })
            .collect();
        log::info!("Rollover interest (totals): {}", parts.join(", "));

        let unapplied_totals = self.unapplied_rollover_totals.borrow();
        let unapplied_parts: Vec<String> = unapplied_totals
            .iter()
            .filter_map(|(currency, total)| {
                Money::new_checked(*total, *currency)
                    .map(|money| money.to_string())
                    .map_err(|e| {
                        log::error!("Cannot report unapplied rollover total for {currency}: {e}");
                    })
                    .ok()
            })
            .collect();
        log::info!(
            "Rollover interest (unapplied totals): {}",
            unapplied_parts.join(", ")
        );
    }

    fn reset(&self) {
        self.rollover_completed.set(false);
        self.rollover_day.replace(None);
        self.rollover_totals.borrow_mut().clear();
        self.unapplied_rollover_totals.borrow_mut().clear();
    }
}

fn nanos_to_utc_datetime(ts: UnixNanos) -> NaiveDateTime {
    let secs = i64::try_from(ts.as_u64() / 1_000_000_000).expect("timestamp seconds fit in i64");
    let nanos =
        u32::try_from(ts.as_u64() % 1_000_000_000).expect("sub-second nanoseconds fit in u32");
    DateTime::from_timestamp(secs, nanos)
        .expect("valid timestamp")
        .naive_utc()
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use nautilus_common::cache::Cache;
    use nautilus_model::identifiers::{InstrumentId, Venue};
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    fn sample_records() -> Vec<InterestRateRecord> {
        vec![
            InterestRateRecord {
                location: "AUS".into(),
                time: "2020-Q1".into(),
                value: 0.75,
            },
            InterestRateRecord {
                location: "USA".into(),
                time: "2020-Q1".into(),
                value: 1.50,
            },
            InterestRateRecord {
                location: "JPN".into(),
                time: "2020-Q1".into(),
                value: -0.10,
            },
            InterestRateRecord {
                location: "USA".into(),
                time: "2020-01".into(),
                value: 1.55,
            },
        ]
    }

    fn rollover_adjustment(booking_date: NaiveDate, amount: &str) -> RolloverAdjustment {
        RolloverAdjustment {
            booking_date,
            amount: Money::from(amount),
        }
    }

    #[rstest]
    fn test_interest_rate_record_serializes_to_json() {
        let record = InterestRateRecord {
            location: "AUS".into(),
            time: "2020-Q1".into(),
            value: 0.75,
        };

        let value = serde_json::to_value(&record).unwrap();

        assert_eq!(
            value,
            json!({
                "location": "AUS",
                "time": "2020-Q1",
                "value": 0.75,
            })
        );
    }

    #[rstest]
    fn test_calculator_quarterly_lookup() {
        let calc = RolloverInterestCalculator::new(sample_records()).unwrap();
        let date = NaiveDate::from_ymd_opt(2020, 2, 15).unwrap();
        let instrument_id = InstrumentId::from("AUDUSD.SIM");

        let rate = calc.calc_overnight_rate(instrument_id, date).unwrap();

        // (0.75 - 1.50) / 365 / 100 = -0.00002054...
        let expected = (0.75 - 1.50) / 365.0 / 100.0;
        assert!((rate - expected).abs() < 1e-12);
    }

    #[rstest]
    fn test_calculator_monthly_preferred_over_quarterly() {
        let calc = RolloverInterestCalculator::new(sample_records()).unwrap();
        let date = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
        let instrument_id = InstrumentId::from("USDJPY.SIM");

        let rate = calc.calc_overnight_rate(instrument_id, date).unwrap();

        // Monthly USD rate (1.55) preferred over quarterly (1.50)
        let expected = (1.55 - (-0.10)) / 365.0 / 100.0;
        assert!((rate - expected).abs() < 1e-12);
    }

    #[rstest]
    fn test_calculator_missing_currency() {
        let calc = RolloverInterestCalculator::new(sample_records()).unwrap();
        let date = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
        let instrument_id = InstrumentId::from("EURGBP.SIM");

        let result = calc.calc_overnight_rate(instrument_id, date);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_module_reset() {
        let module = FXRolloverInterestModule::new(sample_records()).unwrap();
        module.initialize_rollover_day(NaiveDate::from_ymd_opt(2020, 1, 15).unwrap());
        module.rollover_completed.set(true);
        module
            .rollover_totals
            .borrow_mut()
            .insert(Currency::USD(), 100.0);
        module
            .unapplied_rollover_totals
            .borrow_mut()
            .insert(Currency::AUD(), 20.0);

        module.reset();

        assert!(module.rollover_day.borrow().is_none());
        assert!(!module.rollover_completed.get());
        assert!(module.rollover_totals.borrow().is_empty());
        assert!(module.unapplied_rollover_totals.borrow().is_empty());
    }

    #[rstest]
    fn test_calculation_failure_dedupe_is_keyed_per_booking_date() {
        let module = FXRolloverInterestModule::new(sample_records()).unwrap();
        let date = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
        let next_date = NaiveDate::from_ymd_opt(2020, 1, 16).unwrap();
        let instrument_id = InstrumentId::from("AUDUSD.SIM");
        module.initialize_rollover_day(date);

        // A catch-up batch calculates many booking dates before the state is
        // replaced, so a permanent per-instrument skip must stay visible for
        // every date it drops rather than warning only for the first.
        module.log_calculation_failure(date, instrument_id, RolloverFailureKind::Rate, "first");
        module.log_calculation_failure(date, instrument_id, RolloverFailureKind::Rate, "repeat");
        module.log_calculation_failure(next_date, instrument_id, RolloverFailureKind::Rate, "next");

        assert_eq!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .warned_failures,
            AHashSet::from([
                (date, instrument_id, RolloverFailureKind::Rate),
                (next_date, instrument_id, RolloverFailureKind::Rate),
            ])
        );
    }

    #[rstest]
    #[case("CAN", "CADUSD.SIM")]
    #[case("ZAF", "ZARUSD.SIM")]
    fn test_calculator_maps_oecd_location_code(#[case] location: &str, #[case] symbol: &str) {
        let records = vec![
            InterestRateRecord {
                location: location.to_string(),
                time: "2020-Q1".to_string(),
                value: 2.0,
            },
            InterestRateRecord {
                location: "USA".to_string(),
                time: "2020-Q1".to_string(),
                value: 1.5,
            },
        ];
        let calc = RolloverInterestCalculator::new(records).unwrap();
        let date = NaiveDate::from_ymd_opt(2020, 2, 15).unwrap();

        let rate = calc
            .calc_overnight_rate(InstrumentId::from(symbol), date)
            .unwrap();
        let expected = (2.0 - 1.5) / 365.0 / 100.0;

        assert!((rate - expected).abs() < f64::EPSILON);
    }

    #[rstest]
    #[case(f64::NAN)]
    #[case(f64::INFINITY)]
    #[case(f64::NEG_INFINITY)]
    fn test_calculator_rejects_non_finite_rate(#[case] value: f64) {
        let records = vec![InterestRateRecord {
            location: "USA".to_string(),
            time: "2020-Q1".to_string(),
            value,
        }];

        let error = RolloverInterestCalculator::new(records).unwrap_err();

        assert!(error.to_string().contains("must be finite"));
    }

    #[rstest]
    fn test_transient_adjustment_failure_retries_only_failed_adjustments() {
        let module = FXRolloverInterestModule::new(sample_records()).unwrap();
        let date = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
        let attempt_time = UnixNanos::from(
            date.and_hms_opt(22, 1, 0)
                .unwrap()
                .and_utc()
                .timestamp_nanos_opt()
                .unwrap()
                .cast_unsigned(),
        );
        module.initialize_rollover_day(date);
        {
            let mut day = module.rollover_day.borrow_mut();
            let day = day.as_mut().unwrap();
            day.pending_adjustments = Some(vec![
                rollover_adjustment(date, "10.00 USD"),
                rollover_adjustment(date, "20.00 AUD"),
            ]);
            day.pending_end_date = Some(date);
            day.attempt_time = Some(attempt_time);
        }

        module.acknowledge(&[
            AccountAdjustmentOutcome::Applied,
            AccountAdjustmentOutcome::Failed(
                AccountAdjustmentError::TotalOverflow(Currency::AUD()),
            ),
        ]);

        assert!(!module.rollover_completed.get());
        assert_eq!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .pending_adjustments,
            Some(vec![rollover_adjustment(date, "20.00 AUD")])
        );
        assert_eq!(
            module.rollover_totals.borrow().get(&Currency::USD()),
            Some(&10.0)
        );
        assert!(
            !module
                .rollover_totals
                .borrow()
                .contains_key(&Currency::AUD())
        );
        assert_eq!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .warned_adjustment_failures
                .len(),
            1
        );

        let instruments = AHashMap::new();
        let matching_engines = IndexMap::new();
        let cache = Cache::default();
        let ctx = ExchangeContext {
            venue: Venue::new("SIM"),
            base_currency: None,
            instruments: &instruments,
            matching_engines: &matching_engines,
            cache: &cache,
        };
        assert_eq!(
            module.process(attempt_time, &ctx),
            SimulationModuleResult::Completed(vec![Money::from("20.00 AUD")])
        );
        module.acknowledge(&[AccountAdjustmentOutcome::Failed(
            AccountAdjustmentError::TotalOverflow(Currency::AUD()),
        )]);
        assert_eq!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .warned_adjustment_failures
                .len(),
            1
        );
        assert_eq!(
            module.process(attempt_time, &ctx),
            SimulationModuleResult::Completed(vec![Money::from("20.00 AUD")])
        );
        module.acknowledge(&[AccountAdjustmentOutcome::Applied]);

        assert!(module.rollover_completed.get());
        assert_eq!(
            module.rollover_totals.borrow().get(&Currency::USD()),
            Some(&10.0)
        );
        assert_eq!(
            module.rollover_totals.borrow().get(&Currency::AUD()),
            Some(&20.0)
        );
        assert_eq!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .warned_adjustment_failures
                .len(),
            1
        );
    }

    #[rstest]
    fn test_permanent_adjustment_failure_completes_batch() {
        let module = FXRolloverInterestModule::new(sample_records()).unwrap();
        let date = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
        let attempt_time = UnixNanos::from(
            date.and_hms_opt(22, 1, 0)
                .unwrap()
                .and_utc()
                .timestamp_nanos_opt()
                .unwrap()
                .cast_unsigned(),
        );
        module.initialize_rollover_day(date);
        let second_date = date.succ_opt().unwrap();
        {
            let mut day = module.rollover_day.borrow_mut();
            let day = day.as_mut().unwrap();
            day.pending_adjustments = Some(vec![
                rollover_adjustment(date, "20.00 AUD"),
                rollover_adjustment(second_date, "30.00 AUD"),
            ]);
            day.pending_end_date = Some(second_date);
            day.attempt_time = Some(attempt_time);
        }

        module.acknowledge(&[
            AccountAdjustmentOutcome::Failed(AccountAdjustmentError::MissingBalance(
                Currency::AUD(),
            )),
            AccountAdjustmentOutcome::Failed(AccountAdjustmentError::MissingBalance(
                Currency::AUD(),
            )),
        ]);

        assert!(module.rollover_completed.get());
        assert!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .pending_adjustments
                .is_none()
        );
        assert_eq!(
            module
                .unapplied_rollover_totals
                .borrow()
                .get(&Currency::AUD()),
            Some(&50.0)
        );
        assert!(
            !module
                .rollover_totals
                .borrow()
                .contains_key(&Currency::AUD())
        );
        assert_eq!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .warned_adjustment_failures,
            AHashSet::from([
                (
                    date,
                    Currency::AUD(),
                    AccountAdjustmentFailureKind::MissingBalance,
                ),
                (
                    second_date,
                    Currency::AUD(),
                    AccountAdjustmentFailureKind::MissingBalance,
                ),
            ])
        );
        let instruments = AHashMap::new();
        let matching_engines = IndexMap::new();
        let cache = Cache::default();
        let ctx = ExchangeContext {
            venue: Venue::new("SIM"),
            base_currency: None,
            instruments: &instruments,
            matching_engines: &matching_engines,
            cache: &cache,
        };
        let next_attempt = UnixNanos::from(
            second_date
                .succ_opt()
                .unwrap()
                .and_hms_opt(22, 1, 0)
                .unwrap()
                .and_utc()
                .timestamp_nanos_opt()
                .unwrap()
                .cast_unsigned(),
        );
        assert_eq!(
            module.process(next_attempt, &ctx),
            SimulationModuleResult::Completed(Vec::new())
        );
    }

    #[rstest]
    fn test_acknowledgement_count_panic_preserves_pending_batch() {
        let module = FXRolloverInterestModule::new(sample_records()).unwrap();
        let date = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
        module.initialize_rollover_day(date);
        {
            let mut day = module.rollover_day.borrow_mut();
            let day = day.as_mut().unwrap();
            day.pending_adjustments = Some(vec![rollover_adjustment(date, "10.00 USD")]);
            day.attempt_time = Some(UnixNanos::from(1));
        }

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            module.acknowledge(&[]);
        }));

        assert!(result.is_err());
        assert_eq!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .pending_adjustments,
            Some(vec![rollover_adjustment(date, "10.00 USD")])
        );
    }
}
