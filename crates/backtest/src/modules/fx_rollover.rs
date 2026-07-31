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
    rollover_applied: Cell<bool>,
    rollover_day: RefCell<Option<RolloverDayState>>,
    rollover_totals: RefCell<AHashMap<Currency, f64>>,
}

#[derive(Debug, Clone)]
struct RolloverDayState {
    date: NaiveDate,
    scheduled_time_ns: u64,
    warned_failures: AHashSet<(InstrumentId, RolloverFailureKind)>,
    warned_adjustment_failures: AHashSet<(Currency, AccountAdjustmentFailureKind)>,
    pending_adjustments: Option<Vec<Money>>,
    attempt_time: Option<UnixNanos>,
}

enum RolloverCalculationOutcome {
    Completed(Vec<Money>),
    Retry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RolloverFailureKind {
    Engine,
    Price,
    Rate,
    Xrate,
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
            rollover_applied: Cell::new(false),
            rollover_day: RefCell::new(None),
            rollover_totals: RefCell::new(AHashMap::new()),
        })
    }

    fn initialize_rollover_day(&self, date: NaiveDate) {
        let rollover_eastern =
            date.and_time(NaiveTime::from_hms_opt(17, 0, 0).expect("valid rollover time"));
        let rollover_utc = Eastern
            .from_local_datetime(&rollover_eastern)
            .single()
            .expect("unambiguous rollover time")
            .naive_utc();
        let scheduled_time_ns = rollover_utc
            .and_utc()
            .timestamp_nanos_opt()
            .expect("rollover timestamp in range")
            .cast_unsigned();

        self.rollover_day.replace(Some(RolloverDayState {
            date,
            scheduled_time_ns,
            warned_failures: AHashSet::new(),
            warned_adjustment_failures: AHashSet::new(),
            pending_adjustments: None,
            attempt_time: None,
        }));
        self.rollover_applied.set(false);
    }

    /// Logs a calculation failure at warn level once per (instrument, kind)
    /// within the current rollover day, demoting repeats to debug: a `Retry`
    /// outcome re-runs the calculation on every process call until it
    /// completes, and repeating the identical warning per attempt would flood
    /// the log. The set is cleared on a new day, on completion, and on reset.
    fn log_calculation_failure(
        &self,
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
            .insert((instrument_id, kind));

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

            let Some(matching_engine) = ctx.matching_engines.get(&instrument_id) else {
                self.log_calculation_failure(
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
                    instrument_id,
                    RolloverFailureKind::Price,
                    &format!("Cannot calculate rollover for {instrument_id}: no market price"),
                );
                return RolloverCalculationOutcome::Retry;
            };

            let interest_rate = match self.calculator.calc_overnight_rate(instrument_id, date) {
                Ok(rate) => rate,
                Err(e) => {
                    self.log_calculation_failure(
                        instrument_id,
                        RolloverFailureKind::Rate,
                        &format!("Cannot calculate rollover for {instrument_id}: {e}"),
                    );
                    return RolloverCalculationOutcome::Retry;
                }
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

            let Some(adjustment) = rollover_money(instrument_id, rollover, currency) else {
                return RolloverCalculationOutcome::Retry;
            };

            adjustments.push(adjustment);
        }

        RolloverCalculationOutcome::Completed(adjustments)
    }
}

fn rollover_money(instrument_id: InstrumentId, value: f64, currency: Currency) -> Option<Money> {
    Money::new_checked(value, currency)
        .map_err(|e| {
            log::error!("Skipping rollover for {instrument_id}: invalid adjustment: {e}");
        })
        .ok()
}

impl SimulationModule for FXRolloverInterestModule {
    fn pre_process(&self, _data: &Data) {}

    fn process(&self, ts_now: UnixNanos, ctx: &ExchangeContext) -> SimulationModuleResult {
        let utc_dt = nanos_to_utc_datetime(ts_now);
        let eastern_dt = Eastern.from_utc_datetime(&utc_dt);
        let observed_date = eastern_dt.date_naive();

        let initialize = {
            let day = self.rollover_day.borrow();
            day.as_ref()
                .is_none_or(|day| self.rollover_applied.get() && day.date != observed_date)
        };

        if initialize {
            self.initialize_rollover_day(observed_date);
        }

        if self.rollover_applied.get() {
            return SimulationModuleResult::NotReady;
        }

        {
            let mut day = self.rollover_day.borrow_mut();
            let day = day.as_mut().expect("rollover day initialized");
            if let Some(adjustments) = &day.pending_adjustments {
                let adjustments = adjustments.clone();
                day.attempt_time = Some(ts_now);
                return SimulationModuleResult::Completed(adjustments);
            }
        }

        let (date, scheduled_time_ns) = {
            let day = self.rollover_day.borrow();
            let day = day.as_ref().expect("rollover day initialized");
            (day.date, day.scheduled_time_ns)
        };

        if ts_now.as_u64() < scheduled_time_ns {
            return SimulationModuleResult::NotReady;
        }

        let iso_weekday = date.weekday().number_from_monday();
        match self.calculate_rollover_interest(date, iso_weekday, ctx) {
            RolloverCalculationOutcome::Completed(adjustments) => {
                let mut day = self.rollover_day.borrow_mut();
                let day = day.as_mut().expect("rollover day initialized");
                day.pending_adjustments = Some(adjustments.clone());
                day.attempt_time = Some(ts_now);
                SimulationModuleResult::Completed(adjustments)
            }
            RolloverCalculationOutcome::Retry => SimulationModuleResult::NotReady,
        }
    }

    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]) {
        let (adjustments, attempt_time, date, scheduled_time_ns) = {
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
                day.date,
                day.scheduled_time_ns,
            )
        };

        let mut failed = Vec::new();
        {
            let mut totals = self.rollover_totals.borrow_mut();

            for (adjustment, outcome) in adjustments.into_iter().zip(outcomes) {
                match outcome {
                    AccountAdjustmentOutcome::Applied => {
                        let total = totals.entry(adjustment.currency).or_insert(0.0);
                        *total += adjustment.as_f64();
                        self.rollover_day
                            .borrow_mut()
                            .as_mut()
                            .expect("rollover day initialized")
                            .warned_adjustment_failures
                            .retain(|(currency, _)| *currency != adjustment.currency);
                    }
                    AccountAdjustmentOutcome::Failed(error) => {
                        let kind = AccountAdjustmentFailureKind::from(error);
                        let first_failure = self
                            .rollover_day
                            .borrow_mut()
                            .as_mut()
                            .expect("rollover day initialized")
                            .warned_adjustment_failures
                            .insert((adjustment.currency, kind));

                        if first_failure {
                            log::warn!(
                                "Cannot apply rollover adjustment for {}: {error}",
                                adjustment.currency
                            );
                        } else {
                            log::debug!(
                                "Cannot apply rollover adjustment for {}: {error}",
                                adjustment.currency
                            );
                        }
                        failed.push(adjustment);
                    }
                }
            }
        }

        if failed.is_empty() {
            self.rollover_applied.set(true);
            self.rollover_day
                .borrow_mut()
                .as_mut()
                .expect("rollover day initialized")
                .warned_failures
                .clear();
            self.rollover_day
                .borrow_mut()
                .as_mut()
                .expect("rollover day initialized")
                .warned_adjustment_failures
                .clear();

            let attempt_eastern = Eastern.from_utc_datetime(&nanos_to_utc_datetime(attempt_time));

            if attempt_eastern.date_naive() != date {
                log::warn!(
                    "Rollover for {date}, scheduled at {}, booked late at {attempt_time}",
                    UnixNanos::from(scheduled_time_ns)
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
    }

    fn reset(&self) {
        self.rollover_applied.set(false);
        self.rollover_day.replace(None);
        self.rollover_totals.borrow_mut().clear();
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
        module.rollover_applied.set(true);
        module
            .rollover_totals
            .borrow_mut()
            .insert(Currency::USD(), 100.0);

        module.reset();

        assert!(module.rollover_day.borrow().is_none());
        assert!(!module.rollover_applied.get());
        assert!(module.rollover_totals.borrow().is_empty());
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
    fn test_rollover_money_rejects_unrepresentable_adjustment() {
        let adjustment =
            rollover_money(InstrumentId::from("AUDUSD.SIM"), f64::MAX, Currency::USD());

        assert_eq!(adjustment, None);
    }

    #[rstest]
    fn test_partial_acknowledgement_retries_only_failed_adjustments() {
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
            day.pending_adjustments =
                Some(vec![Money::from("10.00 USD"), Money::from("20.00 AUD")]);
            day.attempt_time = Some(attempt_time);
        }

        module.acknowledge(&[
            AccountAdjustmentOutcome::Applied,
            AccountAdjustmentOutcome::Failed(AccountAdjustmentError::MissingBalance(
                Currency::AUD(),
            )),
        ]);

        assert!(!module.rollover_applied.get());
        assert_eq!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .pending_adjustments,
            Some(vec![Money::from("20.00 AUD")])
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
            AccountAdjustmentError::MissingBalance(Currency::AUD()),
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

        assert!(module.rollover_applied.get());
        assert_eq!(
            module.rollover_totals.borrow().get(&Currency::USD()),
            Some(&10.0)
        );
        assert_eq!(
            module.rollover_totals.borrow().get(&Currency::AUD()),
            Some(&20.0)
        );
        assert!(
            module
                .rollover_day
                .borrow()
                .as_ref()
                .unwrap()
                .warned_adjustment_failures
                .is_empty()
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
            day.pending_adjustments = Some(vec![Money::from("10.00 USD")]);
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
            Some(vec![Money::from("10.00 USD")])
        );
    }
}
