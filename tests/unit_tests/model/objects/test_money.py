# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import math
import pickle
from decimal import Decimal

import pytest

from nautilus_trader.model import convert_to_raw_int
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import MONEY_MAX
from nautilus_trader.model.objects import MONEY_MIN
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money


class TestMoney:
    def test_instantiate_with_nan_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(math.nan, currency=USD)

    def test_instantiate_with_none_currency_raises_type_error(self) -> None:
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            Money(1.0, None)

    def test_instantiate_with_value_exceeding_positive_limit_raises_value_error(self) -> None:
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(MONEY_MAX + 1, currency=USD)

    def test_instantiate_with_value_exceeding_negative_limit_raises_value_error(self) -> None:
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(MONEY_MIN - 1, currency=USD)

    def test_instantiate_with_none_value_returns_money_with_zero_amount(self) -> None:
        # Arrange, Act
        money_zero = Money(None, currency=USD)

        # Assert
        assert money_zero.as_decimal() == 0

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, Money(0, USD)],
            [1, Money(1, USD)],
            [-1, Money(-1, USD)],
            ["0", Money(0, USD)],
            ["0.0", Money(0, USD)],
            ["-0.0", Money(0, USD)],
            ["1.0", Money(1, USD)],
            ["-1.0", Money(-1, USD)],
            [Decimal(0), Money(0, USD)],
            [Decimal("1.1"), Money(1.1, USD)],
            [Decimal("-1.1"), Money(-1.1, USD)],
        ],
    )
    def test_instantiate_with_various_valid_inputs_returns_expected_money(
        self,
        value,
        expected,
    ) -> None:
        # Arrange, Act
        money = Money(value, USD)

        # Assert
        assert money == expected

    def test_pickling(self):
        # Arrange
        money = Money(1, USD)

        # Act
        pickled = pickle.dumps(money)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert unpickled == money

    def test_as_double_returns_expected_result(self) -> None:
        # Arrange, Act
        amount = 1.0
        money = Money(amount, USD)

        # Assert
        assert money.as_double() == amount
        assert money.raw == convert_to_raw_int(amount, USD.precision)
        assert str(money) == "1.00 USD"

    def test_initialized_with_many_decimals_rounds_to_currency_precision(self) -> None:
        # Arrange, Act
        amount1 = 1000.333
        amount2 = 5005.556666
        result1 = Money(amount1, USD)
        result2 = Money(amount2, USD)

        # Assert
        assert result1.raw == convert_to_raw_int(amount1, USD.precision)
        assert result2.raw == convert_to_raw_int(amount2, USD.precision)
        assert str(result1) == "1000.33 USD"
        assert str(result2) == "5005.56 USD"
        assert result1.to_formatted_str() == "1_000.33 USD"
        assert result2.to_formatted_str() == "5_005.56 USD"

    def test_equality_with_different_currencies_raises_value_error(self) -> None:
        # Arrange
        money1 = Money(1, USD)
        money2 = Money(1, AUD)

        # Act, Assert
        with pytest.raises(ValueError):
            assert money1 != money2

    def test_equality(self) -> None:
        # Arrange
        money1 = Money(1, USD)
        money2 = Money(1, USD)
        money3 = Money(2, USD)

        # Act, Assert
        assert money1 == money2
        assert money1 != money3

    def test_hash(self) -> None:
        # Arrange
        money0 = Money(0, USD)

        # Act, Assert
        assert isinstance(hash(money0), int)
        assert hash(money0) == hash(money0)

    def test_str(self) -> None:
        # Arrange
        money0 = Money(0, USD)
        money1 = Money(1, USD)
        money2 = Money(1_000_000, USD)

        # Act, Assert
        assert str(money0) == "0.00 USD"
        assert str(money1) == "1.00 USD"
        assert str(money2) == "1000000.00 USD"
        assert money2.to_formatted_str() == "1_000_000.00 USD"

    def test_repr(self) -> None:
        # Arrange
        money = Money(1.00, USD)

        # Act
        result = repr(money)

        # Assert
        assert result == "Money(1.00, USD)"

    def test_from_str_when_malformed_raises_value_error(self) -> None:
        # Arrange
        value = "@"

        # Act, Assert
        with pytest.raises(ValueError):
            Money.from_str(value)

    @pytest.mark.parametrize(
        ("value", "currency", "expected"),
        [
            [0, USDT, Money(0, USDT)],
            [convert_to_raw_int(1, USD.precision), USD, Money(1.00, USD)],
            [convert_to_raw_int(10, AUD.precision), AUD, Money(10.00, AUD)],
        ],
    )
    def test_from_raw_given_valid_values_returns_expected_result(
        self,
        value: str,
        currency: Currency,
        expected: Money,
    ) -> None:
        # Arrange, Act
        result = Money.from_raw(value, currency)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            ["1.00 USDT", Money(1.00, USDT)],
            ["1.00 USD", Money(1.00, USD)],
            ["1.001 AUD", Money(1.00, AUD)],
            ["10_001.01 AUD", Money(10001.01, AUD)],
        ],
    )
    def test_from_str_given_valid_strings_returns_expected_result(
        self,
        value: str,
        expected: Money,
    ) -> None:
        # Arrange, Act
        result1 = Money.from_str(value)
        result2 = Money.from_str(value)

        # Assert
        assert result1 == result2
        assert result1 == expected

    @pytest.mark.parametrize(
        ("value", "expected_amount", "expected_currency"),
        [
            # Scientific notation tests
            ["1e6 USD", "1000000.00", USD],
            ["1E6 USD", "1000000.00", USD],
            ["2.5e4 USD", "25000.00", USD],
            ["3.5E-2 USD", "0.04", USD],
            ["1.23456e-1 AUD", "0.12", AUD],
            ["7.89E1 USDT", "78.90000000", USDT],
            # Underscore handling
            ["1_000 USD", "1000.00", USD],
            ["1_000.25 USD", "1000.25", USD],
            ["9_876_543.21 USD", "9876543.21", USD],
            ["0.000_123 USDT", "0.00012300", USDT],
            # Combined underscores and scientific notation
            ["1_000e2 USD", "100000.00", USD],
            ["2_345.6e-3 USDT", "2.34560000", USDT],
            # Negative values
            ["-1e6 USD", "-1000000.00", USD],
            ["-2.5E-2 USD", "-0.02", USD],
            ["-1_000.50 USD", "-1000.50", USD],
            # Zero representations
            ["0e0 USD", "0.00", USD],
            ["0.0e5 USD", "0.00", USD],
            ["0E-3 USDT", "0.00000000", USDT],
            # Edge cases with precision
            ["0.125 USD", "0.12", USD],
            ["0.135 USD", "0.14", USD],
            ["0.145 USD", "0.14", USD],
            ["0.155 USD", "0.16", USD],
            ["0.165 USD", "0.16", USD],
            # Small numbers with high precision currency
            ["1e-6 USDT", "0.00000100", USDT],
            ["1.234567e-3 USDT", "0.00123457", USDT],
        ],
    )
    def test_from_str_comprehensive(self, value, expected_amount, expected_currency):
        # Arrange, Act
        money = Money.from_str(value)

        # Assert
        assert str(money) == f"{expected_amount} {expected_currency.code}"
        assert money.currency == expected_currency

    @pytest.mark.parametrize(
        "invalid_input",
        [
            "not_a_number USD",
            "1.2.3 USD",
            "++1 USD",
            "--1 USD",
            "1e USD",
            "e10 USD",
            "1e1e1 USD",
            "",
            "USD",  # No amount
            "100",  # No currency
            "nan USD",
            "inf USD",
            "-inf USD",
            "1e1000 USD",  # Overflow
            "1.23",  # Missing currency
            "1.23 ",  # Missing currency
            " USD",  # Missing amount
        ],
    )
    def test_from_str_invalid_input_raises_value_error(self, invalid_input):
        # Arrange, Act, Assert
        with pytest.raises(Exception):  # Various exceptions can be raised for invalid input
            Money.from_str(invalid_input)

    def test_from_str_precision_handling(self):
        # Test that precision is correctly handled for different currencies

        # USD has 2 decimal places
        money_usd = Money.from_str("100.123 USD")
        assert str(money_usd) == "100.12 USD"  # Rounded to 2 decimal places

        # USDT has 8 decimal places
        money_usdt = Money.from_str("100.1234567 USDT")
        assert str(money_usdt) == "100.12345670 USDT"  # USDT has 8 decimal places

        # Scientific notation with precision
        money_sci = Money.from_str("1.23456789e2 USD")
        assert str(money_sci) == "123.46 USD"  # Rounded to 2 decimal places

        # Underscores don't affect precision
        money_under = Money.from_str("1_000.123456 USDT")
        assert str(money_under) == "1000.12345600 USDT"

    @pytest.mark.parametrize(
        ("input_val", "expected"),
        [
            ("1.115 USD", "1.12 USD"),  # Round up
            ("1.125 USD", "1.12 USD"),  # Round to even (down)
            ("1.135 USD", "1.14 USD"),  # Round up
            ("1.145 USD", "1.14 USD"),  # Round to even (down)
            ("1.155 USD", "1.16 USD"),  # Round up
            ("1.165 USD", "1.16 USD"),  # Round to even (down)
            ("1.175 USD", "1.18 USD"),  # Round up
            ("1.185 USD", "1.18 USD"),  # Round to even (down)
            ("1.195 USD", "1.20 USD"),  # Round up
        ],
    )
    def test_from_str_rounding_behavior(self, input_val, expected):
        # Test banker's rounding (ROUND_HALF_EVEN) is applied correctly
        # Arrange, Act
        money = Money.from_str(input_val)

        # Assert
        assert str(money) == expected

    def test_from_str_boundary_values(self):
        # Test values near the boundaries of the Money type

        # Test reasonable large values
        money_large = Money.from_str("1000000000 USD")
        assert str(money_large) == "1000000000.00 USD"

        # Test negative values
        money_neg = Money.from_str("-1000000 USD")
        assert str(money_neg) == "-1000000.00 USD"

        # Zero (should work)
        money_zero = Money.from_str("0 USD")
        assert money_zero.as_double() == 0
        assert str(money_zero) == "0.00 USD"


class TestAccountBalance:
    def test_equality(self):
        # Arrange, Act
        balance1 = AccountBalance(
            total=Money(1, USD),
            locked=Money(0, USD),
            free=Money(1, USD),
        )

        balance2 = AccountBalance(
            total=Money(1, USD),
            locked=Money(0, USD),
            free=Money(1, USD),
        )

        balance3 = AccountBalance(
            total=Money(2, USD),
            locked=Money(0, USD),
            free=Money(2, USD),
        )

        # Act, Assert
        assert balance1 == balance1
        assert balance1 == balance2
        assert balance1 != balance3

    def test_instantiate_str_repr(self):
        # Arrange, Act
        balance = AccountBalance(
            total=Money(1_525_000, USD),
            locked=Money(25_000, USD),
            free=Money(1_500_000, USD),
        )

        # Assert
        assert (
            str(balance)
            == "AccountBalance(total=1_525_000.00 USD, locked=25_000.00 USD, free=1_500_000.00 USD)"
        )
        assert (
            repr(balance)
            == "AccountBalance(total=1_525_000.00 USD, locked=25_000.00 USD, free=1_500_000.00 USD)"
        )


class TestMarginBalance:
    def test_equality(self):
        # Arrange, Act
        margin1 = MarginBalance(
            initial=Money(5_000, USD),
            maintenance=Money(25_000, USD),
        )
        margin2 = MarginBalance(
            initial=Money(5_000, USD),
            maintenance=Money(25_000, USD),
        )
        margin3 = MarginBalance(
            initial=Money(10_000, USD),
            maintenance=Money(50_000, USD),
        )

        # Assert
        assert margin1 == margin1
        assert margin1 == margin2
        assert margin1 != margin3

    def test_instantiate_str_repr_with_instrument_id(self):
        # Arrange, Act
        margin = MarginBalance(
            initial=Money(5_000, USD),
            maintenance=Money(25_000, USD),
            instrument_id=InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO")),
        )

        # Assert
        assert (
            str(margin)
            == "MarginBalance(initial=5_000.00 USD, maintenance=25_000.00 USD, instrument_id=AUD/USD.IDEALPRO)"
        )
        assert (
            repr(margin)
            == "MarginBalance(initial=5_000.00 USD, maintenance=25_000.00 USD, instrument_id=AUD/USD.IDEALPRO)"
        )

    def test_instantiate_str_repr_without_instrument_id(self):
        # Arrange, Act
        margin = MarginBalance(
            initial=Money(5_000, USD),
            maintenance=Money(25_000, USD),
        )

        # Assert
        assert (
            str(margin)
            == "MarginBalance(initial=5_000.00 USD, maintenance=25_000.00 USD, instrument_id=None)"
        )
        assert (
            repr(margin)
            == "MarginBalance(initial=5_000.00 USD, maintenance=25_000.00 USD, instrument_id=None)"
        )
