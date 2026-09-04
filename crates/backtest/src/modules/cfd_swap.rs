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

//! CFD overnight swap simulation module.

use std::cell::{Cell, RefCell};

use ahash::{AHashMap, AHashSet};
use jiff::{
    civil::{Date, Time, Weekday},
    tz::TimeZone,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::Data,
    enums::{InstrumentClass, PriceType},
    identifiers::InstrumentId,
    instruments::Instrument,
    types::{Currency, Money, Price},
};
use rust_decimal::Decimal;

use super::{AccountAdjustmentOutcome, ExchangeContext, SimulationModule, SimulationModuleResult};
#[cfg(feature = "python")]
use crate::python::modules::PySimulationModule;

/// Daily long and short swap rates for a CFD instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.backtest", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
pub struct CfdSwapRate {
    /// The instrument ID the rates apply to.
    pub instrument_id: InstrumentId,
    /// The signed daily rate applied to long positions.
    pub long_rate: Decimal,
    /// The signed daily rate applied to short positions.
    pub short_rate: Decimal,
}

impl CfdSwapRate {
    /// Creates a new [`CfdSwapRate`] instance.
    #[must_use]
    pub const fn new(instrument_id: InstrumentId, long_rate: Decimal, short_rate: Decimal) -> Self {
        Self {
            instrument_id,
            long_rate,
            short_rate,
        }
    }
}

/// Simulates daily CFD swap adjustments at a configurable UTC rollover time.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.backtest",
        extends = PySimulationModule,
        unsendable,
        skip_from_py_object
    )
)]
pub struct CfdSwapModule {
    rates: AHashMap<InstrumentId, CfdSwapRate>,
    rollover_time: Time,
    triple_roll_weekday: Weekday,
    rollover_completed: Cell<bool>,
    rollover_day: RefCell<Option<RolloverDayState>>,
    swap_totals: RefCell<AHashMap<Currency, Decimal>>,
    unapplied_swap_totals: RefCell<AHashMap<Currency, Decimal>>,
}

#[derive(Debug, Clone)]
struct RolloverDayState {
    date: Date,
    warned_failures: AHashSet<(Date, InstrumentId, CfdSwapFailureKind)>,
    pending_adjustments: Option<Vec<SwapAdjustment>>,
    pending_end_date: Option<Date>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CfdSwapFailureKind {
    Engine,
    Price,
    Xrate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SwapAdjustment {
    booking_date: Date,
    amount: Money,
}

impl CfdSwapModule {
    /// Creates a new CFD swap module.
    ///
    /// Rates are signed daily fractions of settlement notional. Later entries replace earlier
    /// entries for the same instrument.
    #[must_use]
    pub fn new(rates: Vec<CfdSwapRate>, rollover_time: Time, triple_roll_weekday: Weekday) -> Self {
        Self {
            rates: rates
                .into_iter()
                .map(|rate| (rate.instrument_id, rate))
                .collect(),
            rollover_time,
            triple_roll_weekday,
            rollover_completed: Cell::new(false),
            rollover_day: RefCell::new(None),
            swap_totals: RefCell::new(AHashMap::new()),
            unapplied_swap_totals: RefCell::new(AHashMap::new()),
        }
    }

    fn initialize_rollover_day(&self, date: Date) {
        self.rollover_day.replace(Some(RolloverDayState {
            date,
            warned_failures: AHashSet::new(),
            pending_adjustments: None,
            pending_end_date: None,
        }));
        self.rollover_completed.set(false);
    }

    fn rollover_time_ns(&self, date: Date) -> anyhow::Result<u64> {
        let timestamp = date
            .to_datetime(self.rollover_time)
            .to_zoned(TimeZone::UTC)?
            .timestamp()
            .as_nanosecond();
        Ok(u64::try_from(timestamp)?)
    }

    fn weekday_on_or_before(mut date: Date) -> anyhow::Result<Date> {
        while date.weekday().to_monday_one_offset() > 5 {
            date = date.yesterday()?;
        }
        Ok(date)
    }

    fn next_weekday(mut date: Date) -> anyhow::Result<Date> {
        loop {
            date = date.tomorrow()?;
            if date.weekday().to_monday_one_offset() <= 5 {
                return Ok(date);
            }
        }
    }

    fn settlement_price(
        ctx: &ExchangeContext,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Option<Price>> {
        let Some(matching_engine) = ctx.matching_engines.get(&instrument_id) else {
            return Ok(None);
        };
        let book = matching_engine.get_book();

        match (book.best_bid_price(), book.best_ask_price()) {
            (Some(bid), Some(ask)) => {
                let midpoint = bid
                    .as_decimal()
                    .checked_add(ask.as_decimal())
                    .and_then(|sum| sum.checked_div(Decimal::TWO))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "cannot calculate CFD swap for {instrument_id}: midpoint overflow"
                        )
                    })?;
                Ok(Some(Price::from_decimal(midpoint)?))
            }
            (Some(price), None) | (None, Some(price)) => Ok(Some(price)),
            (None, None) => Ok(None),
        }
    }

    fn log_calculation_failure(
        &self,
        booking_date: Date,
        instrument_id: InstrumentId,
        kind: CfdSwapFailureKind,
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

    fn calculate_adjustments(
        &self,
        booking_date: Date,
        ctx: &ExchangeContext,
    ) -> anyhow::Result<Option<Vec<Money>>> {
        let mut instrument_ids = self.rates.keys().copied().collect::<Vec<_>>();
        instrument_ids.sort_unstable();
        let mut adjustments = Vec::new();

        for instrument_id in instrument_ids {
            let Some(instrument) = ctx.instruments.get(&instrument_id) else {
                continue;
            };

            if instrument.instrument_class() != InstrumentClass::Cfd {
                continue;
            }

            let mut positions =
                ctx.cache
                    .positions_open(Some(&ctx.venue), Some(&instrument_id), None, None, None);
            positions.sort_unstable_by_key(|position| position.id);
            if positions.is_empty() {
                continue;
            }

            let Some(settlement_price) = Self::settlement_price(ctx, instrument_id)? else {
                let (kind, message) = if ctx.matching_engines.contains_key(&instrument_id) {
                    (
                        CfdSwapFailureKind::Price,
                        format!(
                            "Cannot calculate CFD swap for {instrument_id}: no settlement price"
                        ),
                    )
                } else {
                    (
                        CfdSwapFailureKind::Engine,
                        format!(
                            "Cannot calculate CFD swap for {instrument_id}: no matching engine"
                        ),
                    )
                };
                self.log_calculation_failure(booking_date, instrument_id, kind, &message);
                return Ok(None);
            };
            let rate = self.rates[&instrument_id];
            let multiplier = if booking_date.weekday() == self.triple_roll_weekday {
                Decimal::from(3)
            } else {
                Decimal::ONE
            };

            for position in positions {
                let daily_rate = if position.is_long() {
                    rate.long_rate
                } else if position.is_short() {
                    rate.short_rate
                } else {
                    continue;
                };
                let notional = position.try_notional_value(settlement_price)?;
                let amount = notional
                    .as_decimal()
                    .checked_mul(daily_rate)
                    .and_then(|value| value.checked_mul(multiplier))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "cannot calculate CFD swap for position {}: adjustment overflow",
                            position.id
                        )
                    })?;
                let (amount, currency) = if let Some(base_currency) = ctx.base_currency {
                    let xrate = match ctx.cache.try_get_xrate(
                        ctx.venue,
                        notional.currency,
                        base_currency,
                        PriceType::Mid,
                    ) {
                        Ok(Some(xrate)) => xrate,
                        Ok(None) => {
                            self.log_calculation_failure(
                                booking_date,
                                instrument_id,
                                CfdSwapFailureKind::Xrate,
                                &format!(
                                    "Cannot calculate CFD swap for {instrument_id}: no exchange rate from {} to {base_currency}",
                                    notional.currency
                                ),
                            );
                            return Ok(None);
                        }
                        Err(e) => {
                            self.log_calculation_failure(
                                booking_date,
                                instrument_id,
                                CfdSwapFailureKind::Xrate,
                                &format!(
                                    "Cannot calculate CFD swap for {instrument_id}: exchange rate from {} to {base_currency}: {e}",
                                    notional.currency
                                ),
                            );
                            return Ok(None);
                        }
                    };
                    let amount = amount.checked_mul(xrate).ok_or_else(|| {
                        anyhow::anyhow!(
                            "cannot calculate CFD swap for position {}: currency conversion overflow",
                            position.id
                        )
                    })?;
                    (amount, base_currency)
                } else {
                    (amount, notional.currency)
                };
                adjustments.push(Money::from_decimal(amount, currency)?);
            }
        }

        Ok(Some(adjustments))
    }

    fn log_totals(label: &str, totals: &AHashMap<Currency, Decimal>) -> anyhow::Result<()> {
        let mut currencies = totals.keys().copied().collect::<Vec<_>>();
        currencies.sort_unstable_by_key(|currency| currency.code);
        let parts = currencies
            .into_iter()
            .map(|currency| Money::from_decimal(totals[&currency], currency))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|money| money.to_string())
            .collect::<Vec<_>>();
        log::info!("CFD swap ({label}): {}", parts.join(", "));
        Ok(())
    }
}

impl SimulationModule for CfdSwapModule {
    fn pre_process(&self, _data: &Data) -> anyhow::Result<()> {
        Ok(())
    }

    fn process(
        &self,
        ts_now: UnixNanos,
        ctx: &ExchangeContext,
    ) -> anyhow::Result<SimulationModuleResult> {
        let observed_date = ts_now.to_datetime_utc().to_zoned(TimeZone::UTC).date();
        let initialize_date = {
            let day = self.rollover_day.borrow();
            match day.as_ref() {
                None => Some(Self::weekday_on_or_before(observed_date)?),
                Some(day) if self.rollover_completed.get() && day.date < observed_date => {
                    Some(Self::next_weekday(day.date)?)
                }
                Some(_) => None,
            }
        };

        if let Some(date) = initialize_date {
            self.initialize_rollover_day(date);
        }

        if self.rollover_completed.get() {
            return Ok(SimulationModuleResult::NotReady);
        }

        {
            let day = self.rollover_day.borrow();
            let day = day
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("CFD swap rollover day is not initialized"))?;
            if let Some(adjustments) = &day.pending_adjustments {
                return Ok(SimulationModuleResult::Completed(
                    adjustments
                        .iter()
                        .map(|adjustment| adjustment.amount)
                        .collect(),
                ));
            }
        }

        let date = self
            .rollover_day
            .borrow()
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("CFD swap rollover day is not initialized"))?
            .date;
        if ts_now.as_u64() < self.rollover_time_ns(date)? {
            return Ok(SimulationModuleResult::NotReady);
        }

        let mut booking_date = date;
        let mut batch = Vec::new();
        let batch_end_date = loop {
            if booking_date > observed_date
                || (booking_date == observed_date
                    && ts_now.as_u64() < self.rollover_time_ns(booking_date)?)
            {
                return Ok(SimulationModuleResult::NotReady);
            }

            let Some(adjustments) = self.calculate_adjustments(booking_date, ctx)? else {
                return Ok(SimulationModuleResult::NotReady);
            };
            batch.extend(adjustments.into_iter().map(|amount| SwapAdjustment {
                booking_date,
                amount,
            }));

            let next = Self::next_weekday(booking_date)?;
            if next > observed_date
                || (next == observed_date && ts_now.as_u64() < self.rollover_time_ns(next)?)
            {
                break booking_date;
            }
            booking_date = next;
        };

        let adjustments = batch.iter().map(|adjustment| adjustment.amount).collect();
        let mut day = self.rollover_day.borrow_mut();
        let day = day
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("CFD swap rollover day is not initialized"))?;
        day.pending_adjustments = Some(batch);
        day.pending_end_date = Some(batch_end_date);
        Ok(SimulationModuleResult::Completed(adjustments))
    }

    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
        let (adjustments, batch_end_date) = {
            let mut day = self.rollover_day.borrow_mut();
            let day = day
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("CFD swap rollover day is not initialized"))?;
            let adjustment_count = day
                .pending_adjustments
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("no completed CFD swap batch to acknowledge"))?
                .len();
            anyhow::ensure!(
                outcomes.len() == adjustment_count,
                "CFD swap acknowledgement count {}, expected {}",
                outcomes.len(),
                adjustment_count
            );
            let adjustments = day
                .pending_adjustments
                .take()
                .ok_or_else(|| anyhow::anyhow!("no completed CFD swap batch to acknowledge"))?;
            let end_date = day
                .pending_end_date
                .ok_or_else(|| anyhow::anyhow!("CFD swap batch end date was not recorded"))?;
            (adjustments, end_date)
        };

        let mut failed = Vec::new();

        for (adjustment, outcome) in adjustments.into_iter().zip(outcomes) {
            match outcome {
                AccountAdjustmentOutcome::Applied => {
                    let mut totals = self.swap_totals.borrow_mut();
                    let total = totals.entry(adjustment.amount.currency).or_default();
                    *total = total
                        .checked_add(adjustment.amount.as_decimal())
                        .ok_or_else(|| anyhow::anyhow!("CFD swap diagnostic total overflow"))?;
                }
                AccountAdjustmentOutcome::Failed(error) if error.is_retryable() => {
                    log::warn!(
                        "Cannot apply CFD swap adjustment for {} on {}: {error}",
                        adjustment.amount.currency,
                        adjustment.booking_date
                    );
                    failed.push(adjustment);
                }
                AccountAdjustmentOutcome::Failed(error) => {
                    log::warn!(
                        "CFD swap adjustment {} on {} is recorded as unapplied: {error}",
                        adjustment.amount,
                        adjustment.booking_date
                    );
                    let mut totals = self.unapplied_swap_totals.borrow_mut();
                    let total = totals.entry(adjustment.amount.currency).or_default();
                    *total = total
                        .checked_add(adjustment.amount.as_decimal())
                        .ok_or_else(|| anyhow::anyhow!("unapplied CFD swap total overflow"))?;
                }
            }
        }

        let mut day = self.rollover_day.borrow_mut();
        let day = day
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("CFD swap rollover day is not initialized"))?;
        if failed.is_empty() {
            day.date = batch_end_date;
            day.pending_end_date = None;
            day.warned_failures.clear();
            self.rollover_completed.set(true);
        } else {
            day.pending_adjustments = Some(failed);
        }
        Ok(())
    }

    fn log_diagnostics(&self) -> anyhow::Result<()> {
        Self::log_totals("totals", &self.swap_totals.borrow())?;
        Self::log_totals("unapplied totals", &self.unapplied_swap_totals.borrow())
    }

    fn reset(&self) -> anyhow::Result<()> {
        self.rollover_completed.set(false);
        self.rollover_day.replace(None);
        self.swap_totals.borrow_mut().clear();
        self.unapplied_swap_totals.borrow_mut().clear();
        Ok(())
    }
}
