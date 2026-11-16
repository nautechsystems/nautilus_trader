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
/// `exchange_rate` converts the instrument's quote currency to the equity currency.
/// Inverse futures, perpetuals, and futures spreads use reciprocal-price PnL directly
/// when equity uses the instrument's base currency. Otherwise, they value that PnL in quote currency
/// at the stop-loss price before conversion.
/// Inverse options and option spreads retain premium-based sizing.
/// Reciprocal-price sizing requires positive prices.
/// The returned quantity rounds down within the risk budget and quantity limits.
///
/// # Errors
///
/// Returns an error if an input is invalid, decimal arithmetic overflows, or
/// the final size rounds to zero or cannot be represented as a [`Quantity`].
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

    let inverse = instrument
        .instrument_class()
        .divides_notional_by_price(instrument.is_inverse());

    if inverse {
        check_positive_decimal(entry.as_decimal(), "entry")?;
        check_positive_decimal(stop_loss.as_decimal(), "stop_loss")?;
    }

    let risk_distance = calculate_risk_distance(entry, stop_loss)?;
    let risk_money = calculate_riskable_money(equity.as_decimal(), risk, commission_rate)?;

    if risk_distance.is_zero() {
        return Ok(Quantity::zero(instrument.size_precision()));
    }

    let equity_in_pnl_currency = inverse && equity.currency == instrument.cost_currency();

    let conversion_rate = if equity_in_pnl_currency {
        Decimal::ONE
    } else {
        exchange_rate
    };

    let mut position_size = risk_money
        .checked_div(conversion_rate)
        .ok_or_else(position_size_overflow)?;

    if inverse {
        position_size = position_size
            .checked_mul(entry.as_decimal())
            .ok_or_else(position_size_overflow)?;

        if equity_in_pnl_currency {
            position_size = position_size
                .checked_mul(stop_loss.as_decimal())
                .ok_or_else(position_size_overflow)?;
        }
    }

    position_size = position_size
        .checked_div(risk_distance)
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
        .try_make_qty_from_decimal(final_size, Some(true))
        .map_err(|e| CorrectnessError::PredicateViolation {
            message: e.to_string(),
        })
}

fn calculate_risk_distance(entry: Price, stop_loss: Price) -> CorrectnessResult<Decimal> {
    entry
        .as_decimal()
        .checked_sub(stop_loss.as_decimal())
        .map(|value| value.abs())
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
        instruments::stubs::{
            btcusd_bybit, crypto_option_btc_deribit, default_fx_ccy, futures_contract_es,
        },
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
    #[case::risk_budget("160 USD", None)]
    #[case::hard_limit("260 USD", Some(dec!(1.6)))]
    fn test_calculate_rounds_down_within_limits(
        #[case] equity: &str,
        #[case] hard_limit: Option<Decimal>,
    ) {
        let instrument = InstrumentAny::FuturesContract(futures_contract_es(None, None));

        let quantity = calculate_fixed_risk_position_size(
            &instrument,
            Price::from("100.00"),
            Price::from("99.00"),
            Money::from(equity),
            dec!(0.01),
            Decimal::ZERO,
            Decimal::ONE,
            hard_limit,
            Decimal::ZERO,
            1,
        )
        .unwrap();

        assert_eq!(quantity.as_decimal(), Decimal::ONE);
    }

    #[rstest]
    fn test_calculate_rejects_size_below_one_increment() {
        let instrument = InstrumentAny::FuturesContract(futures_contract_es(None, None));

        let error = calculate_fixed_risk_position_size(
            &instrument,
            Price::from("100.00"),
            Price::from("99.00"),
            Money::from("60 USD"),
            dec!(0.01),
            Decimal::ZERO,
            Decimal::ONE,
            Some(dec!(0.6)),
            Decimal::ZERO,
            1,
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "value rounded to zero for quantity");
    }

    #[rstest]
    #[case::long("20000.0", "19000.0", "1 BTC", dec!(1), dec!(3800))]
    #[case::quote_rate("20000.0", "19000.0", "1 BTC", Decimal::ONE / dec!(19000), dec!(3800))]
    #[case::entry_rate("20000.0", "19000.0", "1 BTC", Decimal::ONE / dec!(20000), dec!(3800))]
    #[case::short("19000.0", "20000.0", "1 BTC", dec!(1), dec!(3800))]
    #[case::account_currency("20000.0", "19000.0", "1000 USD", dec!(1), dec!(200))]
    #[case::third_currency("20000.0", "19000.0", "1000 EUR", dec!(0.8), dec!(250))]
    #[case::zero_rate("20000.0", "19000.0", "1 BTC", dec!(0), dec!(0))]
    fn test_calculate_inverse_contract_size(
        #[case] entry: &str,
        #[case] stop_loss: &str,
        #[case] equity: &str,
        #[case] exchange_rate: Decimal,
        #[case] expected: Decimal,
        #[values(1_u32, 10_u32)] multiplier: u32,
    ) {
        let mut contract = btcusd_bybit();
        contract.multiplier = Quantity::from_decimal_dp(Decimal::from(multiplier), 0).unwrap();
        let instrument = InstrumentAny::CryptoPerpetual(contract);

        let quantity = calculate_fixed_risk_position_size(
            &instrument,
            Price::from(entry),
            Price::from(stop_loss),
            Money::from(equity),
            dec!(0.01),
            Decimal::ZERO,
            exchange_rate,
            None,
            Decimal::ONE,
            1,
        )
        .unwrap();

        assert_eq!(quantity.as_decimal(), expected / Decimal::from(multiplier));
    }

    #[rstest]
    #[case::zero_entry("0.0", "19000.0", "invalid Decimal for 'entry' not positive, was 0.0")]
    #[case::zero_stop(
        "20000.0",
        "0.0",
        "invalid Decimal for 'stop_loss' not positive, was 0.0"
    )]
    #[case::negative_entry(
        "-1.0",
        "19000.0",
        "invalid Decimal for 'entry' not positive, was -1.0"
    )]
    #[case::negative_stop(
        "20000.0",
        "-1.0",
        "invalid Decimal for 'stop_loss' not positive, was -1.0"
    )]
    fn test_calculate_inverse_rejects_nonpositive_prices(
        #[case] entry: &str,
        #[case] stop_loss: &str,
        #[case] expected: &str,
    ) {
        let instrument = InstrumentAny::CryptoPerpetual(btcusd_bybit());

        let error = calculate_fixed_risk_position_size(
            &instrument,
            Price::from(entry),
            Price::from(stop_loss),
            Money::from("1 BTC"),
            dec!(0.01),
            Decimal::ZERO,
            Decimal::ONE,
            None,
            Decimal::ONE,
            1,
        )
        .unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_calculate_inverse_option_uses_premium_distance() {
        let mut option =
            crypto_option_btc_deribit(3, 1, Price::from("0.001"), Quantity::from("0.1"));
        option.is_inverse = true;
        option.quote_currency = Currency::BTC();
        let instrument = InstrumentAny::CryptoOption(option);

        let quantity = calculate_fixed_risk_position_size(
            &instrument,
            Price::from("0.100"),
            Price::from("0.090"),
            Money::from("1 BTC"),
            dec!(0.01),
            Decimal::ZERO,
            Decimal::ONE,
            None,
            Decimal::ONE,
            1,
        )
        .unwrap();

        assert_eq!(quantity.as_decimal(), Decimal::ONE);
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
    #[case::single(1, dec!(500000))]
    #[case::split(3, dec!(166000))]
    fn test_impose_hard_limit(
        #[case] units: usize,
        #[case] expected: Decimal,
        instrument_gbpusd: InstrumentAny,
    ) {
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
            units,
        )
        .unwrap();

        assert_eq!(result.as_decimal(), expected);
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
