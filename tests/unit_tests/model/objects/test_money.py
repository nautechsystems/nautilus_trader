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
