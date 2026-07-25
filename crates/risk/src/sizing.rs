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
use nautilus_core::correctness::{CorrectnessError, Result};
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
/// Returns an error if `Decimal` arithmetic overflows or the final size cannot
/// be represented as a [`Quantity`].
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
) -> Result<Quantity> {
    if exchange_rate.is_zero() {
        return Ok(Quantity::zero(instrument.size_precision()));
    }

    let risk_points = calculate_risk_ticks(entry, stop_loss, instrument)?;
    let risk_money = calculate_riskable_money(equity.as_decimal(), risk, commission_rate)?;

    if risk_points <= Decimal::ZERO {
        return Ok(Quantity::zero(instrument.size_precision()));
    }

    let price_increment = instrument.price_increment().as_decimal();

    let mut position_size = risk_money
        .checked_div(exchange_rate)
        .and_then(|v| v.checked_div(risk_points))
        .and_then(|v| v.checked_div(price_increment))
        .ok_or_else(|| CorrectnessError::PredicateViolation {
            message: OVERFLOW_MESSAGE.into(),
        })?;

    if let Some(hard_limit) = hard_limit {
        position_size = position_size.min(hard_limit);
    }

    let units_decimal =
        Decimal::from_usize(units).ok_or_else(|| CorrectnessError::PredicateViolation {
            message: OVERFLOW_MESSAGE.into(),
        })?;

    let mut position_size_batched = position_size
        .checked_div(units_decimal)
        .map(|v| v.max(Decimal::ZERO))
        .ok_or_else(|| CorrectnessError::PredicateViolation {
            message: OVERFLOW_MESSAGE.into(),
        })?;

    if unit_batch_size > Decimal::ZERO {
        position_size_batched = position_size_batched
            .checked_div(unit_batch_size)
            .map(|v| v.floor())
            .and_then(|v| v.checked_mul(unit_batch_size))
            .ok_or_else(|| CorrectnessError::PredicateViolation {
                message: OVERFLOW_MESSAGE.into(),
            })?;
    }

    let final_size = instrument
        .max_quantity()
        .map_or(position_size_batched, |max_quantity| {
            position_size_batched.min(max_quantity.as_decimal())
        });

    Quantity::from_decimal_dp(final_size, instrument.size_precision())
}

// Helper functions
fn calculate_risk_ticks(
    entry: Price,
    stop_loss: Price,
    instrument: &InstrumentAny,
) -> Result<Decimal> {
    let diff = entry
        .as_decimal()
        .checked_sub(stop_loss.as_decimal())
        .ok_or_else(|| CorrectnessError::PredicateViolation {
            message: OVERFLOW_MESSAGE.into(),
        })?;
    let ticks = diff
        .abs()
        .checked_div(instrument.price_increment().as_decimal())
        .ok_or_else(|| CorrectnessError::PredicateViolation {
            message: OVERFLOW_MESSAGE.into(),
        })?;
    Ok(ticks)
}

fn calculate_riskable_money(
    equity: Decimal,
    risk: Decimal,
    commission_rate: Decimal,
) -> Result<Decimal> {
    if equity <= Decimal::ZERO {
        return Ok(Decimal::ZERO);
    }

    let risk_money =
        equity
            .checked_mul(risk)
            .ok_or_else(|| CorrectnessError::PredicateViolation {
                message: OVERFLOW_MESSAGE.into(),
            })?;
    let commission = risk_money
        .checked_mul(commission_rate)
        .and_then(|v| v.checked_mul(Decimal::TWO)) // round turn
        .ok_or_else(|| CorrectnessError::PredicateViolation {
            message: OVERFLOW_MESSAGE.into(),
        })?;
    let net =
        risk_money
            .checked_sub(commission)
            .ok_or_else(|| CorrectnessError::PredicateViolation {
                message: OVERFLOW_MESSAGE.into(),
            })?;

    Ok(net)
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        identifiers::Symbol, instruments::stubs::default_fx_ccy, types::Currency,
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
    #[case::large_risk(dec!(1e23), Decimal::ZERO, EXCHANGE_RATE)]
    #[case::large_commission_rate(Decimal::new(1, 3), dec!(1e26), EXCHANGE_RATE)]
    #[case::tiny_exchange_rate(Decimal::new(1, 3), Decimal::ZERO, dec!(1e-28))]
    fn test_calculate_returns_error_on_arithmetic_overflow(
        #[case] risk: Decimal,
        #[case] commission_rate: Decimal,
        #[case] exchange_rate: Decimal,
        instrument_gbpusd: InstrumentAny,
    ) {
        let equity = Money::new(1_000_000.0, instrument_gbpusd.quote_currency());
        let entry = Price::new(1.00100, instrument_gbpusd.price_precision());
        let stop_loss = Price::new(1.00000, instrument_gbpusd.price_precision());

        let result = calculate_fixed_risk_position_size(
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
        );

        assert!(result.is_err());
        assert!(format!("{result:?}").contains("overflow"));
    }
}
