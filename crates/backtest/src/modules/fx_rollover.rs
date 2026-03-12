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

use ahash::AHashMap;
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use chrono_tz::US::Eastern;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::Data,
    enums::{AssetClass, PriceType},
    identifiers::InstrumentId,
    instruments::Instrument,
    position::Position,
    types::{Currency, Money},
};

use super::{ExchangeContext, SimulationModule};

const LOCATION_CURRENCY_MAP: &[(&str, &str)] = &[
    ("AUS", "AUD"),
    ("CAD", "CAD"),
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
    ("ZAR", "ZAR"),
];

/// A single interest rate data entry.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.backtest", from_py_object)
)]
pub struct InterestRateRecord {
    /// OECD location code (e.g., "AUS", "USA").
    pub location: String,
    /// Time period key (e.g., "2024-01" for monthly, "2024-Q1" for quarterly).
    pub time: String,
    /// Interest rate value as a percentage (e.g., 5.25 means 5.25%).
    pub value: f64,
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
    pub fn new(records: Vec<InterestRateRecord>) -> Self {
        let location_to_currency: AHashMap<&str, &str> =
            LOCATION_CURRENCY_MAP.iter().copied().collect();

        let mut rates: AHashMap<String, AHashMap<String, f64>> = AHashMap::new();

        for record in records {
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

        Self { rates }
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
pub struct FXRolloverInterestModule {
    calculator: RolloverInterestCalculator,
    rollover_time_ns: Cell<u64>,
    rollover_applied: Cell<bool>,
    day_number: Cell<u32>,
    rollover_totals: RefCell<AHashMap<Currency, f64>>,
}

impl FXRolloverInterestModule {
    /// Creates a new FX rollover interest module.
    pub fn new(records: Vec<InterestRateRecord>) -> Self {
        Self {
            calculator: RolloverInterestCalculator::new(records),
            rollover_time_ns: Cell::new(0),
            rollover_applied: Cell::new(false),
            day_number: Cell::new(0),
            rollover_totals: RefCell::new(AHashMap::new()),
        }
    }

    fn apply_rollover_interest(
        &self,
        date: NaiveDate,
        iso_weekday: u32,
        ctx: &ExchangeContext,
    ) -> Vec<Money> {
        let mut adjustments = Vec::new();

        let mut mid_prices: AHashMap<InstrumentId, f64> = AHashMap::new();

        for (instrument_id, instrument) in ctx.instruments {
            if instrument.asset_class() != AssetClass::FX {
                continue;
            }

            let matching_engine = match ctx.matching_engines.get(instrument_id) {
                Some(engine) => engine,
                None => continue,
            };

            let book = matching_engine.get_book();
            let mid = if let Some(m) = book.midpoint() {
                m
            } else if let Some(p) = book.best_bid_price() {
                p.as_f64()
            } else if let Some(p) = book.best_ask_price() {
                p.as_f64()
            } else {
                continue;
            };
            mid_prices.insert(*instrument_id, mid);
        }

        for (instrument_id, &mid) in &mid_prices {
            let positions: Vec<&Position> =
                ctx.cache
                    .positions_open(Some(&ctx.venue), Some(instrument_id), None, None, None);

            if positions.is_empty() {
                continue;
            }

            let interest_rate = match self.calculator.calc_overnight_rate(*instrument_id, date) {
                Ok(rate) => rate,
                Err(e) => {
                    log::warn!("Skipping rollover for {instrument_id}: {e}");
                    continue;
                }
            };

            let net_qty: f64 = positions.iter().map(|p| p.signed_qty).sum();

            let mut rollover = net_qty * mid * interest_rate;

            // Triple for Wednesday (T+2 settlement) and Friday (weekend)
            if iso_weekday == 3 || iso_weekday == 5 {
                rollover *= 3.0;
            }

            let instrument = &ctx.instruments[instrument_id];
            let currency = if let Some(base) = ctx.base_currency {
                let xrate = ctx
                    .cache
                    .get_xrate(ctx.venue, instrument.quote_currency(), base, PriceType::Mid)
                    .unwrap_or(0.0);
                rollover *= xrate;
                base
            } else {
                instrument.quote_currency()
            };

            {
                let mut totals = self.rollover_totals.borrow_mut();
                let total = totals.entry(currency).or_insert(0.0);
                *total += rollover;
            }

            adjustments.push(Money::new(rollover, currency));
        }

        adjustments
    }
}

impl SimulationModule for FXRolloverInterestModule {
    fn pre_process(&self, _data: &Data) {}

    fn process(&self, ts_now: UnixNanos, ctx: &ExchangeContext) -> Vec<Money> {
        let utc_dt = nanos_to_utc_datetime(ts_now);
        let eastern_dt = Eastern.from_utc_datetime(&utc_dt);
        let eastern_day = eastern_dt.ordinal();

        if self.day_number.get() != eastern_day {
            self.day_number.set(eastern_day);
            self.rollover_applied.set(false);

            let rollover_eastern = eastern_dt
                .date_naive()
                .and_time(NaiveTime::from_hms_opt(17, 0, 0).unwrap());
            let rollover_utc = Eastern
                .from_local_datetime(&rollover_eastern)
                .single()
                .unwrap()
                .naive_utc();
            let rollover_ns = rollover_utc.and_utc().timestamp_nanos_opt().unwrap() as u64;
            self.rollover_time_ns.set(rollover_ns);
        }

        if !self.rollover_applied.get() && ts_now.as_u64() >= self.rollover_time_ns.get() {
            let iso_weekday = eastern_dt.weekday().number_from_monday();
            self.rollover_applied.set(true);
            return self.apply_rollover_interest(eastern_dt.date_naive(), iso_weekday, ctx);
        }

        Vec::new()
    }

    fn log_diagnostics(&self) {
        let totals = self.rollover_totals.borrow();
        let parts: Vec<String> = totals
            .iter()
            .map(|(currency, total)| {
                let money = Money::new(*total, *currency);
                money.to_string()
            })
            .collect();
        log::info!("Rollover interest (totals): {}", parts.join(", "));
    }

    fn reset(&self) {
        self.rollover_time_ns.set(0);
        self.rollover_applied.set(false);
        self.day_number.set(0);
        self.rollover_totals.borrow_mut().clear();
    }
}

fn nanos_to_utc_datetime(ts: UnixNanos) -> NaiveDateTime {
    let secs = (ts.as_u64() / 1_000_000_000) as i64;
    let nanos = (ts.as_u64() % 1_000_000_000) as u32;
    DateTime::from_timestamp(secs, nanos)
        .expect("valid timestamp")
        .naive_utc()
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

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
    fn test_calculator_quarterly_lookup() {
        let calc = RolloverInterestCalculator::new(sample_records());
        let date = NaiveDate::from_ymd_opt(2020, 2, 15).unwrap();
        let instrument_id = InstrumentId::from("AUDUSD.SIM");

        let rate = calc.calc_overnight_rate(instrument_id, date).unwrap();

        // (0.75 - 1.50) / 365 / 100 = -0.00002054...
        let expected = (0.75 - 1.50) / 365.0 / 100.0;
        assert!((rate - expected).abs() < 1e-12);
    }

    #[rstest]
    fn test_calculator_monthly_preferred_over_quarterly() {
        let calc = RolloverInterestCalculator::new(sample_records());
        let date = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
        let instrument_id = InstrumentId::from("USDJPY.SIM");

        let rate = calc.calc_overnight_rate(instrument_id, date).unwrap();

        // Monthly USD rate (1.55) preferred over quarterly (1.50)
        let expected = (1.55 - (-0.10)) / 365.0 / 100.0;
        assert!((rate - expected).abs() < 1e-12);
    }

    #[rstest]
    fn test_calculator_missing_currency() {
        let calc = RolloverInterestCalculator::new(sample_records());
        let date = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
        let instrument_id = InstrumentId::from("EURGBP.SIM");

        let result = calc.calc_overnight_rate(instrument_id, date);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_module_reset() {
        let module = FXRolloverInterestModule::new(sample_records());
        module.day_number.set(15);
        module.rollover_applied.set(true);
        module
            .rollover_totals
            .borrow_mut()
            .insert(Currency::USD(), 100.0);

        module.reset();

        assert_eq!(module.day_number.get(), 0);
        assert!(!module.rollover_applied.get());
        assert!(module.rollover_totals.borrow().is_empty());
    }
}
