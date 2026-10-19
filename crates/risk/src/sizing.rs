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

//! Position sizing calculation functions.
use nautilus_core::correctness::{
    CorrectnessError, CorrectnessResult, check_positive_decimal, check_positive_usize,
    check_predicate_true,
};
use nautilus_model::{
    instruments::{Instrument, InstrumentAny},
    types::{Money, Price, Quantity},
};
use rust_decimal::{Decimal, prelude::FromPrimitive};

const OVERFLOW_MESSAGE: &str = "arithmetic overflow calculating fixed-risk position size";

/// Calculates the position size based on fixed risk parameters.
///
/// # Errors
///
/// Returns an error if an input is invalid, decimal arithmetic overflows, or
/// the final size cannot be represented as a [`Quantity`].
#[expect(
    clippy::too_many_arguments,
    reason = "position sizing API mirrors fixed-risk inputs used by callers"
)]
pub fn calculate_fixed_risk_position_size(
    instrument: &InstrumentAny,
    entry: Price,
    stop_loss: Price,
    equity: Money,
    risk: Decimal,
    commission_rate: Decimal,
    exchange_rate: Decimal,
    hard_limit: Option<Decimal>,
    unit_batch_size: Decimal,
    units: usize,
) -> CorrectnessResult<Quantity> {
    check_positive_decimal(risk, "risk")?;
    check_predicate_true(
        exchange_rate >= Decimal::ZERO,
        "exchange_rate must be non-negative",
    )?;
    check_predicate_true(
        commission_rate >= Decimal::ZERO,
        "commission_rate must be non-negative",
    )?;

    if let Some(hard_limit) = hard_limit {
        check_positive_decimal(hard_limit, "hard_limit")?;
    }
    check_predicate_true(
        unit_batch_size >= Decimal::ZERO,
        "unit_batch_size must be non-negative",
    )?;
    check_positive_usize(units, "units")?;

    if exchange_rate.is_zero() {
        return Ok(Quantity::zero(instrument.size_precision()));
    }

    let risk_points = calculate_risk_ticks(entry, stop_loss, instrument)?;
    let risk_money = calculate_riskable_money(equity.as_decimal(), risk, commission_rate)?;

    if risk_points <= Decimal::ZERO {
        return Ok(Quantity::zero(instrument.size_precision()));
    }

    let mut position_size = risk_money
        .checked_div(exchange_rate)
        .and_then(|value| value.checked_div(risk_points))
        .and_then(|value| value.checked_div(instrument.price_increment().as_decimal()))
        .and_then(|value| value.checked_div(instrument.multiplier().as_decimal()))
        .ok_or_else(position_size_overflow)?;

    if let Some(hard_limit) = hard_limit {
        position_size = position_size.min(hard_limit);
    }

    let units_decimal = Decimal::from_usize(units).ok_or_else(position_size_overflow)?;
    let mut position_size_batched = position_size
        .checked_div(units_decimal)
        .map(|value| value.max(Decimal::ZERO))
        .ok_or_else(position_size_overflow)?;

    if unit_batch_size > Decimal::ZERO {
        position_size_batched = position_size_batched
            .checked_div(unit_batch_size)
            .map(|value| value.floor())
            .and_then(|value| value.checked_mul(unit_batch_size))
            .ok_or_else(position_size_overflow)?;
    }

    let final_size = instrument
        .max_quantity()
        .map_or(position_size_batched, |max_quantity| {
            position_size_batched.min(max_quantity.as_decimal())
        });

    instrument
        .try_make_qty_from_decimal(final_size, None)
        .map_err(|e| CorrectnessError::PredicateViolation {
            message: e.to_string(),
        })
}

fn calculate_risk_ticks(
    entry: Price,
    stop_loss: Price,
    instrument: &InstrumentAny,
) -> CorrectnessResult<Decimal> {
    entry
        .as_decimal()
        .checked_sub(stop_loss.as_decimal())
        .map(|value| value.abs())
        .and_then(|value| value.checked_div(instrument.price_increment().as_decimal()))
        .ok_or_else(position_size_overflow)
}

fn calculate_riskable_money(
    equity: Decimal,
    risk: Decimal,
    commission_rate: Decimal,
) -> CorrectnessResult<Decimal> {
    if equity <= Decimal::ZERO {
        return Ok(Decimal::ZERO);
    }

    let risk_money = equity
        .checked_mul(risk)
        .ok_or_else(position_size_overflow)?;
    let commission = risk_money
        .checked_mul(commission_rate)
        .and_then(|value| value.checked_mul(Decimal::TWO))
        .ok_or_else(position_size_overflow)?;

    risk_money
        .checked_sub(commission)
        .ok_or_else(position_size_overflow)
}

fn position_size_overflow() -> CorrectnessError {
    CorrectnessError::PredicateViolation {
        message: OVERFLOW_MESSAGE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        identifiers::Symbol,
        instruments::stubs::{default_fx_ccy, futures_contract_es},
        types::Currency,
    };
    use rstest::*;
    use rust_decimal_macros::dec;

    use super::*;

    const EXCHANGE_RATE: Decimal = Decimal::ONE;

    #[fixture]
    fn instrument_gbpusd() -> InstrumentAny {
        InstrumentAny::CurrencyPair(default_fx_ccy(Symbol::from_str_unchecked("GBP/USD"), None))
    }

    #[fixture]
    fn instrument_futures_with_multiplier() -> InstrumentAny {
        // A futures contract whose multiplier scales the per-unit dollar risk,
        // as it does for position P&L (e.g. ES, CL, NG).
        let mut instrument = futures_contract_es(None, None);
        instrument.multiplier = Quantity::from(1000);
        InstrumentAny::FuturesContract(instrument)
    }

    #[fixture]
    fn instrument_gbpusd_without_max_quantity() -> InstrumentAny {
        let mut instrument = default_fx_ccy(Symbol::from_str_unchecked("GBP/USD"), None);
        instrument.max_quantity = None;
        InstrumentAny::CurrencyPair(instrument)
    }

    #[rstest]
    fn test_calculate_with_zero_equity_returns_quantity_zero(instrument_gbpusd: InstrumentAny) {
        let equity = Money::zero(instrument_gbpusd.quote_currency());
        let entry = Price::new(1.00100, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 3), // 0.001%
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::from(1000),
            1,
        )
        .unwrap();

        assert_eq!(result, Quantity::from("0.0"));
    }

    #[rstest]
    fn test_calculate_with_zero_exchange_rate(instrument_gbpusd: InstrumentAny) {
        let equity = Money::new(100_000.0, instrument_gbpusd.quote_currency());
        let entry = Price::new(1.00100, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 3), // 0.001%
            Decimal::ZERO,
            Decimal::ZERO, // Zero exchange rate
            None,
            Decimal::from(1000),
            1,
        )
        .unwrap();

        assert_eq!(result, Quantity::from("0.0"));
    }

    #[rstest]
    fn test_calculate_with_zero_risk(instrument_gbpusd: InstrumentAny) {
        let equity = Money::new(100_000.0, instrument_gbpusd.quote_currency());
        let price = Price::new(1.00100, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            price,
            price, // Same price = no risk
            equity,
            Decimal::new(1, 3), // 0.001%
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::from(1000),
            1,
        )
        .unwrap();

        assert_eq!(result, Quantity::from("0.0"));
    }

    #[rstest]
    fn test_calculate_single_unit_size(instrument_gbpusd: InstrumentAny) {
        let equity = Money::new(1_000_000.0, instrument_gbpusd.quote_currency());
        let entry = Price::new(1.00100, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 3), // 0.001%
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::from(1000),
            1,
        )
        .unwrap();

        assert_eq!(result, Quantity::from("1000000.0"));
    }

    #[rstest]
    fn test_calculate_accounts_for_instrument_multiplier(
        instrument_futures_with_multiplier: InstrumentAny,
    ) {
        // Multiplier = 1000: a $1.00 stop risks $1,000/contract, so $10,000 of
        // risk targets 10 contracts.
        let equity = Money::new(1_000_000.0, Currency::USD());
        let entry = Price::new(100.00, instrument_futures_with_multiplier.price_precision());
        let stop_loss = Price::new(99.00, instrument_futures_with_multiplier.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_futures_with_multiplier,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 2), // 1%
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::from(1),
            1,
        )
        .unwrap();

        assert_eq!(result.as_decimal(), dec!(10));
    }

    #[rstest]
    fn test_calculate_single_unit_with_exchange_rate(instrument_gbpusd: InstrumentAny) {
        let equity = Money::new(1_000_000.0, Currency::USD());
        let entry = Price::new(110.010, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(110.000, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 3), // 0.1%
            Decimal::ZERO,
            Decimal::from_f64(0.00909).unwrap(), // 1/110
            None,
            Decimal::from(1),
            1,
        )
        .unwrap();

        assert_eq!(result, Quantity::from("1000000.0"));
    }

    #[rstest]
    fn test_calculate_single_unit_size_when_risk_too_high(instrument_gbpusd: InstrumentAny) {
        let equity = Money::new(100_000.0, Currency::USD());
        let entry = Price::new(3.00000, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 2), // 1%
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::from(1000),
            1,
        )
        .unwrap();

        assert_eq!(result, Quantity::from("0.0"));
    }

    #[rstest]
    fn test_impose_hard_limit(instrument_gbpusd: InstrumentAny) {
        let equity = Money::new(1_000_000.0, instrument_gbpusd.quote_currency());
        let entry = Price::new(1.00010, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 2), // 1%
            Decimal::ZERO,
            EXCHANGE_RATE,
            Some(Decimal::from(500_000)),
            Decimal::from(1000),
            1,
        )
        .unwrap();

        assert_eq!(result, Quantity::from("500000.0"));
    }

    #[rstest]
    fn test_calculate_without_max_quantity_leaves_size_uncapped(
        instrument_gbpusd_without_max_quantity: InstrumentAny,
    ) {
        let equity = Money::from("1000000 USD");
        let entry = Price::from("1.00010");
        let stop_loss = Price::from("1.00000");

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd_without_max_quantity,
            entry,
            stop_loss,
            equity,
            dec!(0.01),
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::from(1000),
            1,
        )
        .unwrap();

        assert_eq!(result.as_decimal(), dec!(100000000));
    }

    #[rstest]
    fn test_calculate_multiple_unit_size(instrument_gbpusd: InstrumentAny) {
        let equity = Money::new(1_000_000.0, instrument_gbpusd.quote_currency());
        let entry = Price::new(1.00010, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 3), // 0.1%
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::from(1000),
            3, // 3 units
        )
        .unwrap();

        assert_eq!(result, Quantity::from("1000000.0"));
    }

    #[rstest]
    fn test_calculate_multiple_unit_size_larger_batches(instrument_gbpusd: InstrumentAny) {
        let equity = Money::new(1_000_000.0, instrument_gbpusd.quote_currency());
        let entry = Price::new(1.00087, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 3), // 0.1%
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::from(25000),
            4, // 4 units
        )
        .unwrap();

        assert_eq!(result, Quantity::from("275000.0"));
    }

    #[rstest]
    fn test_calculate_for_gbpusd_with_commission(instrument_gbpusd: InstrumentAny) {
        let equity = Money::new(1_000_000.0, instrument_gbpusd.quote_currency());
        let entry = Price::new(107.703, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(107.403, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            Decimal::new(1, 2),                    // 1%
            Decimal::new(2, 4),                    // 0.0002
            Decimal::from_f64(0.009_931).unwrap(), // 1/107.403
            None,
            Decimal::from(1000),
            1,
        )
        .unwrap();

        assert_eq!(result, Quantity::from("1000000.0"));
    }

    #[rstest]
    #[case(
        (
            Decimal::ZERO,
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::ONE,
            1,
        ),
        "invalid Decimal for 'risk' not positive, was 0"
    )]
    #[case(
        (
            dec!(0.001),
            Decimal::ZERO,
            dec!(-1),
            None,
            Decimal::ONE,
            1,
        ),
        "exchange_rate must be non-negative"
    )]
    #[case(
        (
            dec!(0.001),
            dec!(-0.001),
            EXCHANGE_RATE,
            None,
            Decimal::ONE,
            1,
        ),
        "commission_rate must be non-negative"
    )]
    #[case(
        (
            dec!(0.001),
            Decimal::ZERO,
            EXCHANGE_RATE,
            Some(Decimal::ZERO),
            Decimal::ONE,
            1,
        ),
        "invalid Decimal for 'hard_limit' not positive, was 0"
    )]
    #[case(
        (
            dec!(0.001),
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            dec!(-1),
            1,
        ),
        "unit_batch_size must be non-negative"
    )]
    #[case(
        (
            dec!(0.001),
            Decimal::ZERO,
            EXCHANGE_RATE,
            None,
            Decimal::ONE,
            0,
        ),
        "invalid usize for 'units' not positive, was 0"
    )]
    fn test_calculate_rejects_invalid_inputs(
        #[case] inputs: (Decimal, Decimal, Decimal, Option<Decimal>, Decimal, usize),
        #[case] expected: &str,
        instrument_gbpusd: InstrumentAny,
    ) {
        let (risk, commission_rate, exchange_rate, hard_limit, unit_batch_size, units) = inputs;
        let equity = Money::new(1_000_000.0, instrument_gbpusd.quote_currency());
        let entry = Price::new(1.00100, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let error = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            risk,
            commission_rate,
            exchange_rate,
            hard_limit,
            unit_batch_size,
            units,
        )
        .unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::large_risk(dec!(1e23), Decimal::ZERO, EXCHANGE_RATE)]
    #[case::large_commission_rate(dec!(0.001), dec!(1e26), EXCHANGE_RATE)]
    #[case::tiny_exchange_rate(dec!(0.001), Decimal::ZERO, dec!(1e-28))]
    fn test_calculate_returns_error_on_arithmetic_overflow(
        #[case] risk: Decimal,
        #[case] commission_rate: Decimal,
        #[case] exchange_rate: Decimal,
        instrument_gbpusd: InstrumentAny,
    ) {
        let equity = Money::new(1_000_000.0, instrument_gbpusd.quote_currency());
        let entry = Price::new(1.00100, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let error = calculate_fixed_risk_position_size(
            &instrument_gbpusd,
            entry,
            stop_loss,
            equity,
            risk,
            commission_rate,
            exchange_rate,
            None,
            Decimal::from(1000),
            1,
        )
        .unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::PredicateViolation {
                message: OVERFLOW_MESSAGE.to_string(),
            }
        );
    }
}
