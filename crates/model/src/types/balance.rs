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

//! Represents an account balance denominated in a particular currency.

use std::fmt::{Debug, Display};

use nautilus_core::correctness::{
    CorrectnessResult, CorrectnessResultExt, FAILED, check_predicate_true,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    identifiers::InstrumentId,
    types::{Currency, Money},
};

/// Represents an account balance denominated in a particular currency.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.model",
        frozen,
        eq,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct AccountBalance {
    /// The account balance currency.
    pub currency: Currency,
    /// The total account balance.
    pub total: Money,
    /// The account balance locked (assigned to pending orders).
    pub locked: Money,
    /// The account balance free for trading.
    pub free: Money,
}

impl AccountBalance {
    /// Creates a new [`AccountBalance`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `total` is not the result of `locked` + `free`.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type that stacktrace can be printed for errors.
    pub fn new_checked(total: Money, locked: Money, free: Money) -> CorrectnessResult<Self> {
        check_predicate_true(
            total.currency == locked.currency,
            &format!(
                "`total` currency ({}) != `locked` currency ({})",
                total.currency, locked.currency
            ),
        )?;
        check_predicate_true(
            total.currency == free.currency,
            &format!(
                "`total` currency ({}) != `free` currency ({})",
                total.currency, free.currency
            ),
        )?;
        check_predicate_true(
            total == locked + free,
            &format!("`total` ({total}) - `locked` ({locked}) != `free` ({free})"),
        )?;
        Ok(Self {
            currency: total.currency,
            total,
            locked,
            free,
        })
    }

    /// Creates a new [`AccountBalance`] instance.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`AccountBalance::new_checked`] for more details.
    #[must_use]
    pub fn new(total: Money, locked: Money, free: Money) -> Self {
        Self::new_checked(total, locked, free).expect_display(FAILED)
    }

    /// Creates a new [`AccountBalance`] from `total` and `locked` decimal amounts,
    /// deriving `free` in fixed-point so the `total == locked + free` invariant
    /// holds by construction at the currency precision.
    ///
    /// When `total` is non-negative, `locked` is clamped into `[0, total]` so
    /// a transient rounding glitch or overshoot cannot leave `free` negative.
    /// When `total` is negative (spot borrow deficit or underwater margin account),
    /// `locked` is passed through verbatim so venue-reported reserved margin is
    /// preserved and `free` carries the shortfall.
    ///
    /// # Errors
    ///
    /// Returns an error if `total` or `locked` cannot be represented at the currency
    /// precision.
    pub fn from_total_and_locked(
        total: Decimal,
        locked: Decimal,
        currency: Currency,
    ) -> CorrectnessResult<Self> {
        let total = Money::from_decimal(total, currency)?;
        let locked = Money::from_decimal(locked, currency)?;
        let locked_raw = if total.raw >= 0 {
            locked.raw.clamp(0, total.raw)
        } else {
            locked.raw
        };
        let clamped_locked = Money::from_raw(locked_raw, currency);
        let free = Money::from_raw(total.raw - clamped_locked.raw, currency);
        Ok(Self::new(total, clamped_locked, free))
    }

    /// Creates a new [`AccountBalance`] from `total` and `free` decimal amounts,
    /// deriving `locked` in fixed-point so the `total == locked + free` invariant
    /// holds by construction at the currency precision.
    ///
    /// When `total` is non-negative, `free` is clamped into `[0, total]` so
    /// a transient PnL overshoot cannot leave `locked` negative. When `total` is
    /// negative, `free` is passed through verbatim so the venue-reported available
    /// amount is preserved and `locked` carries the difference.
    ///
    /// # Errors
    ///
    /// Returns an error if `total` or `free` cannot be represented at the currency
    /// precision.
    pub fn from_total_and_free(
        total: Decimal,
        free: Decimal,
        currency: Currency,
    ) -> CorrectnessResult<Self> {
        let total = Money::from_decimal(total, currency)?;
        let free = Money::from_decimal(free, currency)?;
        let free_raw = if total.raw >= 0 {
            free.raw.clamp(0, total.raw)
        } else {
            free.raw
        };
        let clamped_free = Money::from_raw(free_raw, currency);
        let locked = Money::from_raw(total.raw - clamped_free.raw, currency);
        Ok(Self::new(total, locked, clamped_free))
    }
}

impl PartialEq for AccountBalance {
    fn eq(&self, other: &Self) -> bool {
        self.total == other.total && self.locked == other.locked && self.free == other.free
    }
}

impl Debug for AccountBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(total={}, locked={}, free={})",
            stringify!(AccountBalance),
            self.total,
            self.locked,
            self.free,
        )
    }
}

impl Display for AccountBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Copy, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.model",
        frozen,
        eq,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
/// Represents a margin balance.
///
/// Margin entries have two mutually exclusive scopes:
///
/// - Per-instrument: `instrument_id = Some(id)`. Used for isolated margin and
///   for calculated margin in backtest mode where each instrument carries its
///   own reserve.
/// - Account-wide (cross margin): `instrument_id = None`. Used for venues that
///   report a single aggregate margin per collateral currency (most derivatives
///   venues in cross-margin mode).
pub struct MarginBalance {
    pub initial: Money,
    pub maintenance: Money,
    pub currency: Currency,
    pub instrument_id: Option<InstrumentId>,
}

impl MarginBalance {
    /// Creates a new [`MarginBalance`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `initial` and `maintenance` have different currencies.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(
        initial: Money,
        maintenance: Money,
        instrument_id: Option<InstrumentId>,
    ) -> CorrectnessResult<Self> {
        check_predicate_true(
            initial.currency == maintenance.currency,
            &format!(
                "`initial` currency ({}) != `maintenance` currency ({})",
                initial.currency, maintenance.currency
            ),
        )?;
        Ok(Self {
            initial,
            maintenance,
            currency: initial.currency,
            instrument_id,
        })
    }

    /// Creates a new [`MarginBalance`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `initial` and `maintenance` have different currencies.
    #[must_use]
    pub fn new(initial: Money, maintenance: Money, instrument_id: Option<InstrumentId>) -> Self {
        Self::new_checked(initial, maintenance, instrument_id).expect_display(FAILED)
    }
}

impl PartialEq for MarginBalance {
    fn eq(&self, other: &Self) -> bool {
        self.initial == other.initial
            && self.maintenance == other.maintenance
            && self.instrument_id == other.instrument_id
    }
}

impl Debug for MarginBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.instrument_id {
            Some(id) => write!(
                f,
                "{}(initial={}, maintenance={}, instrument_id={})",
                stringify!(MarginBalance),
                self.initial,
                self.maintenance,
                id,
            ),
            None => write!(
                f,
                "{}(initial={}, maintenance={}, currency={})",
                stringify!(MarginBalance),
                self.initial,
                self.maintenance,
                self.currency,
            ),
        }
    }
}

impl Display for MarginBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use crate::{
        identifiers::InstrumentId,
        types::{
            AccountBalance, Currency, MarginBalance, Money,
            stubs::{stub_account_balance, stub_margin_balance},
        },
    };

    #[rstest]
    fn test_account_balance_equality() {
        let account_balance_1 = stub_account_balance();
        let account_balance_2 = stub_account_balance();
        assert_eq!(account_balance_1, account_balance_2);
    }

    #[rstest]
    fn test_account_balance_debug(stub_account_balance: AccountBalance) {
        let result = format!("{stub_account_balance:?}");
        let expected =
            "AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)";
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_account_balance_display(stub_account_balance: AccountBalance) {
        let result = format!("{stub_account_balance}");
        let expected =
            "AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)";
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_account_balance_new_checked_with_currency_mismatch_returns_error() {
        let usd = Currency::USD();
        let eur = Currency::EUR();
        let result = AccountBalance::new_checked(
            Money::new(1000.0, usd),
            Money::new(250.0, eur),
            Money::new(750.0, usd),
        );
        assert!(result.is_err());
    }

    #[rstest]
    #[should_panic(expected = "`total` currency (USD) != `locked` currency (EUR)")]
    fn test_account_balance_new_with_currency_mismatch_panics() {
        let usd = Currency::USD();
        let eur = Currency::EUR();
        let _ = AccountBalance::new(
            Money::new(1000.0, usd),
            Money::new(250.0, eur),
            Money::new(750.0, usd),
        );
    }

    fn parse_dec(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[rstest]
    #[case::zero_zero_usd("0", "0")]
    #[case::total_zero_positive_locked_usd("0", "5")]
    #[case::round_usd("1000", "250")]
    #[case::free_is_zero_usd("1000", "1000")]
    #[case::locked_is_zero_usd("1000", "0")]
    #[case::fractional_usd("1234.56", "789.01")]
    #[case::fractional_btc("10.12345678", "2.87654321")]
    #[case::small_btc("0.00000001", "0")]
    #[case::large_usd("1000000000.00", "123.45")]
    #[case::drift_af_btc("10.000000035", "10.000000031")]
    #[case::drift_locked_over_precision_btc("10.000000034999", "0.000000004999")]
    #[case::locked_above_total_usd("100", "150")]
    #[case::locked_above_total_btc("1.50000000", "5.00000000")]
    #[case::negative_locked_usd("100", "-5")]
    #[case::negative_locked_btc("0.50000000", "-0.00000001")]
    #[case::negative_total_with_reserved("-10", "5")]
    #[case::negative_total_negative_locked("-10", "-5")]
    #[case::deep_underwater_with_reserved("-100", "50")]
    fn test_from_total_and_locked_preserves_invariant(
        #[case] total_str: &str,
        #[case] locked_str: &str,
    ) {
        for currency in [Currency::USD(), Currency::BTC()] {
            let total = parse_dec(total_str);
            let locked = parse_dec(locked_str);
            let balance = AccountBalance::from_total_and_locked(total, locked, currency).unwrap();

            assert_eq!(
                balance.total.raw,
                balance.locked.raw + balance.free.raw,
                "invariant violated for total={total}, locked={locked}, currency={}",
                currency.code,
            );
            // When total is non-negative, locked must also be non-negative; when total is
            // negative the helper passes venue values through so locked may be negative too.
            if balance.total.raw >= 0 {
                assert!(
                    balance.locked.raw >= 0,
                    "locked must be non-negative for non-negative total (found raw={})",
                    balance.locked.raw,
                );
            }
            assert_eq!(balance.total.currency, currency);
            assert_eq!(balance.locked.currency, currency);
            assert_eq!(balance.free.currency, currency);
        }
    }

    #[rstest]
    #[case::zero_zero_usd("0", "0")]
    #[case::round_usd("1000", "750")]
    #[case::free_equals_total_usd("1000", "1000")]
    #[case::free_is_zero_usd("1000", "0")]
    #[case::fractional_usd("1234.56", "444.55")]
    #[case::fractional_btc("10.12345678", "7.24691356")]
    #[case::drift_over_precision_btc("10.000000034999", "9.999999994999")]
    #[case::free_above_total_usd("100", "120")]
    #[case::free_above_total_btc("0.50000000", "0.99999999")]
    #[case::negative_free_usd("100", "-5")]
    #[case::negative_total_usd("-10", "0")]
    #[case::negative_total_positive_free("-10", "5")]
    fn test_from_total_and_free_preserves_invariant(
        #[case] total_str: &str,
        #[case] free_str: &str,
    ) {
        for currency in [Currency::USD(), Currency::BTC()] {
            let total = parse_dec(total_str);
            let free = parse_dec(free_str);
            let balance = AccountBalance::from_total_and_free(total, free, currency).unwrap();

            assert_eq!(
                balance.total.raw,
                balance.locked.raw + balance.free.raw,
                "invariant violated for total={total}, free={free}, currency={}",
                currency.code,
            );

            if balance.total.raw >= 0 {
                assert!(
                    balance.free.raw >= 0,
                    "free must be non-negative for non-negative total (found raw={})",
                    balance.free.raw,
                );
            }
            assert_eq!(balance.total.currency, currency);
            assert_eq!(balance.locked.currency, currency);
            assert_eq!(balance.free.currency, currency);
        }
    }

    #[rstest]
    #[case::usd_basic(dec!(1000.00), dec!(250.00), dec!(1000.00), dec!(250.00), dec!(750.00))]
    #[case::usd_all_free(dec!(500.00), dec!(0.00), dec!(500.00), dec!(0.00), dec!(500.00))]
    #[case::usd_all_locked(dec!(500.00), dec!(500.00), dec!(500.00), dec!(500.00), dec!(0.00))]
    #[case::usd_clamp_above(dec!(100.00), dec!(150.00), dec!(100.00), dec!(100.00), dec!(0.00))]
    #[case::usd_clamp_negative(dec!(100.00), dec!(-5.00), dec!(100.00), dec!(0.00), dec!(100.00))]
    fn test_from_total_and_locked_exact_usd(
        #[case] total_in: Decimal,
        #[case] locked_in: Decimal,
        #[case] expected_total: Decimal,
        #[case] expected_locked: Decimal,
        #[case] expected_free: Decimal,
    ) {
        let usd = Currency::USD();
        let balance = AccountBalance::from_total_and_locked(total_in, locked_in, usd).unwrap();

        assert_eq!(
            balance.total,
            Money::from_decimal(expected_total, usd).unwrap()
        );
        assert_eq!(
            balance.locked,
            Money::from_decimal(expected_locked, usd).unwrap()
        );
        assert_eq!(
            balance.free,
            Money::from_decimal(expected_free, usd).unwrap()
        );
    }

    #[rstest]
    #[case::usd_basic(dec!(1000.00), dec!(750.00), dec!(1000.00), dec!(250.00), dec!(750.00))]
    #[case::usd_all_free(dec!(500.00), dec!(500.00), dec!(500.00), dec!(0.00), dec!(500.00))]
    #[case::usd_all_locked(dec!(500.00), dec!(0.00), dec!(500.00), dec!(500.00), dec!(0.00))]
    #[case::usd_clamp_above(dec!(100.00), dec!(120.00), dec!(100.00), dec!(0.00), dec!(100.00))]
    #[case::usd_clamp_negative(dec!(100.00), dec!(-5.00), dec!(100.00), dec!(100.00), dec!(0.00))]
    fn test_from_total_and_free_exact_usd(
        #[case] total_in: Decimal,
        #[case] free_in: Decimal,
        #[case] expected_total: Decimal,
        #[case] expected_locked: Decimal,
        #[case] expected_free: Decimal,
    ) {
        let usd = Currency::USD();
        let balance = AccountBalance::from_total_and_free(total_in, free_in, usd).unwrap();

        assert_eq!(
            balance.total,
            Money::from_decimal(expected_total, usd).unwrap()
        );
        assert_eq!(
            balance.locked,
            Money::from_decimal(expected_locked, usd).unwrap()
        );
        assert_eq!(
            balance.free,
            Money::from_decimal(expected_free, usd).unwrap()
        );
    }

    // Reproducer for issue #3867: three independent `Money::new` calls at currency
    // precision 8 rounded `(total, locked=amount-af, free=af)` to `1_000_000_003`,
    // `1_000_000_000`, `4` respectively, violating `total == locked + free`.
    #[rstest]
    fn test_from_total_and_locked_issue_3867_drift() {
        let btc = Currency::BTC();
        let af = parse_dec("0.000000035");
        let amount = parse_dec("10") + af;
        let locked = amount - af;

        let balance = AccountBalance::from_total_and_locked(amount, locked, btc).unwrap();

        assert_eq!(balance.total.raw, balance.locked.raw + balance.free.raw);
    }

    #[rstest]
    #[case(dec!(0), dec!(100))]
    #[case(dec!(1), dec!(1000000))]
    #[case(dec!(500), dec!(500000))]
    fn test_from_total_and_locked_non_negative_total_never_leaves_free_negative(
        #[case] total: Decimal,
        #[case] locked: Decimal,
    ) {
        let usd = Currency::USD();
        let balance = AccountBalance::from_total_and_locked(total, locked, usd).unwrap();
        assert!(
            balance.free.raw >= 0,
            "free went negative: total={total}, locked={locked}"
        );
        assert_eq!(balance.total.raw, balance.locked.raw + balance.free.raw);
    }

    #[rstest]
    #[case(dec!(1000.00), dec!(250.00), dec!(750.00))]
    #[case(dec!(0.00), dec!(0.00), dec!(0.00))]
    #[case(dec!(500.00), dec!(500.00), dec!(0.00))]
    #[case(dec!(500.00), dec!(0.00), dec!(500.00))]
    fn test_locked_and_free_forms_agree_when_consistent(
        #[case] total: Decimal,
        #[case] locked: Decimal,
        #[case] free: Decimal,
    ) {
        let usd = Currency::USD();
        let from_locked = AccountBalance::from_total_and_locked(total, locked, usd).unwrap();
        let from_free = AccountBalance::from_total_and_free(total, free, usd).unwrap();
        assert_eq!(from_locked, from_free);
    }

    #[rstest]
    #[case::borrow_deficit(dec!(-100), dec!(50), dec!(-100), dec!(50), dec!(-150))]
    #[case::underwater_no_reserve(dec!(-10), dec!(0), dec!(-10), dec!(0), dec!(-10))]
    #[case::negative_locked_passed_through(dec!(-10), dec!(-5), dec!(-10), dec!(-5), dec!(-5))]
    fn test_from_total_and_locked_preserves_reserved_on_negative_total(
        #[case] total_in: Decimal,
        #[case] locked_in: Decimal,
        #[case] expected_total: Decimal,
        #[case] expected_locked: Decimal,
        #[case] expected_free: Decimal,
    ) {
        let usd = Currency::USD();
        let balance = AccountBalance::from_total_and_locked(total_in, locked_in, usd).unwrap();

        assert_eq!(
            balance.total,
            Money::from_decimal(expected_total, usd).unwrap()
        );
        assert_eq!(
            balance.locked,
            Money::from_decimal(expected_locked, usd).unwrap()
        );
        assert_eq!(
            balance.free,
            Money::from_decimal(expected_free, usd).unwrap()
        );
        assert_eq!(balance.total.raw, balance.locked.raw + balance.free.raw);
    }

    #[rstest]
    #[case::available_below_total(dec!(-100), dec!(-150), dec!(-100), dec!(50), dec!(-150))]
    #[case::available_zero_preserved(dec!(-100), dec!(0), dec!(-100), dec!(-100), dec!(0))]
    fn test_from_total_and_free_preserves_available_on_negative_total(
        #[case] total_in: Decimal,
        #[case] free_in: Decimal,
        #[case] expected_total: Decimal,
        #[case] expected_locked: Decimal,
        #[case] expected_free: Decimal,
    ) {
        let usd = Currency::USD();
        let balance = AccountBalance::from_total_and_free(total_in, free_in, usd).unwrap();

        assert_eq!(
            balance.total,
            Money::from_decimal(expected_total, usd).unwrap()
        );
        assert_eq!(
            balance.locked,
            Money::from_decimal(expected_locked, usd).unwrap()
        );
        assert_eq!(
            balance.free,
            Money::from_decimal(expected_free, usd).unwrap()
        );
        assert_eq!(balance.total.raw, balance.locked.raw + balance.free.raw);
    }

    #[rstest]
    fn test_from_total_and_locked_invalid_decimal_returns_error() {
        let btc = Currency::BTC();
        // 28 leading digits scaled to BTC precision 8 exceeds MoneyRaw bounds, so
        // `Money::from_decimal` rejects it and the error propagates.
        let too_large: Decimal = "79228162514264337593543950335".parse().unwrap();
        let result = AccountBalance::from_total_and_locked(too_large, dec!(0), btc);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_margin_balance_equality() {
        let margin_balance_1 = stub_margin_balance();
        let margin_balance_2 = stub_margin_balance();
        assert_eq!(margin_balance_1, margin_balance_2);
    }

    #[rstest]
    fn test_margin_balance_debug(stub_margin_balance: MarginBalance) {
        let display = format!("{stub_margin_balance:?}");
        assert_eq!(
            "MarginBalance(initial=5000.00 USD, maintenance=20000.00 USD, instrument_id=BTCUSDT.COINBASE)",
            display
        );
    }

    #[rstest]
    fn test_margin_balance_display(stub_margin_balance: MarginBalance) {
        let display = format!("{stub_margin_balance}");
        assert_eq!(
            "MarginBalance(initial=5000.00 USD, maintenance=20000.00 USD, instrument_id=BTCUSDT.COINBASE)",
            display
        );
    }

    #[rstest]
    fn test_margin_balance_new_checked_with_currency_mismatch_returns_error() {
        let usd = Currency::USD();
        let eur = Currency::EUR();
        let instrument_id = InstrumentId::from("BTCUSDT.COINBASE");
        let result = MarginBalance::new_checked(
            Money::new(5000.0, usd),
            Money::new(20000.0, eur),
            Some(instrument_id),
        );
        assert!(result.is_err());
    }

    #[rstest]
    #[should_panic(expected = "`initial` currency (USD) != `maintenance` currency (EUR)")]
    fn test_margin_balance_new_with_currency_mismatch_panics() {
        let usd = Currency::USD();
        let eur = Currency::EUR();
        let instrument_id = InstrumentId::from("BTCUSDT.COINBASE");
        let _ = MarginBalance::new(
            Money::new(5000.0, usd),
            Money::new(20000.0, eur),
            Some(instrument_id),
        );
    }

    #[rstest]
    fn test_margin_balance_account_scope_display() {
        let usd = Currency::USD();
        let balance = MarginBalance::new(Money::new(500.0, usd), Money::new(200.0, usd), None);
        assert_eq!(
            "MarginBalance(initial=500.00 USD, maintenance=200.00 USD, currency=USD)",
            format!("{balance}")
        );
    }
}
