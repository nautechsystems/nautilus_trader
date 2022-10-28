# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import pickle
from decimal import Decimal

import pytest

from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money


class TestMoney:
    def test_instantiate_with_none_currency_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            Money(1.0, None)

    def test_instantiate_with_value_exceeding_positive_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(9_223_372_036 + 1, currency=USD)

    def test_instantiate_with_value_exceeding_negative_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(-9_223_372_036 - 1, currency=USD)

    def test_instantiate_with_none_value_returns_money_with_zero_amount(self):
        # Arrange, Act
        money_zero = Money(None, currency=USD)

        # Assert
        assert 0 == money_zero.as_decimal()

    @pytest.mark.parametrize(
        "value, expected",
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
    def test_instantiate_with_various_valid_inputs_returns_expected_money(self, value, expected):
        # Arrange, Act
        money = Money(value, USD)

        # Assert
        assert money == expected

    def test_pickling(self):
        # Arrange
        money = Money(1, USD)

        # Act
        pickled = pickle.dumps(money)
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

        # Assert
        assert unpickled == money

    def test_as_double_returns_expected_result(self):
        # Arrange, Act
        money = Money(1, USD)

        # Assert
        assert 1.0 == money.as_double()
        assert "1.00" == str(money)

    def test_initialized_with_many_decimals_rounds_to_currency_precision(self):
        # Arrange, Act
        result1 = Money(1000.333, USD)
        result2 = Money(5005.556666, USD)

        # Assert
        assert "1_000.33 USD" == result1.to_str()
        assert "5_005.56 USD" == result2.to_str()

    def test_equality_with_different_currencies_raises_value_error(self):
        # Arrange
        money1 = Money(1, USD)
        money2 = Money(1, AUD)

        # Act, Assert
        with pytest.raises(ValueError):
            assert money1 != money2

    def test_equality(self):
        # Arrange
        money1 = Money(1, USD)
        money2 = Money(1, USD)
        money3 = Money(2, USD)

        # Act, Assert
        assert money1 == money2
        assert money1 != money3

    def test_hash(self):
        # Arrange
        money0 = Money(0, USD)

        # Act, Assert
        assert isinstance(hash(money0), int)
        assert hash(money0) == hash(money0)

    def test_str(self):
        # Arrange
        money0 = Money(0, USD)
        money1 = Money(1, USD)
        money2 = Money(1_000_000, USD)

        # Act, Assert
        assert "0.00" == str(money0)
        assert "1.00" == str(money1)
        assert "1000000.00" == str(money2)
        assert "1_000_000.00 USD" == money2.to_str()

    def test_repr(self):
        # Arrange
        money = Money(1.00, USD)

        # Act
        result = repr(money)

        # Assert
        assert "Money('1.00', USD)" == result

    def test_from_str_when_malformed_raises_value_error(self):
        # Arrange
        value = "@"

        # Act, Assert
        with pytest.raises(ValueError):
            Money.from_str(value)

    @pytest.mark.parametrize(
        "value, expected",
        [
            ["1.00 USD", Money(1.00, USD)],
            ["1.001 AUD", Money(1.00, AUD)],
        ],
    )
    def test_from_str_given_valid_strings_returns_expected_result(
        self,
        value,
        expected,
    ):
        # Arrange, Act
        result = Money.from_str(value)

        # Assert
        assert result == expected


class TestAccountBalance:
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
