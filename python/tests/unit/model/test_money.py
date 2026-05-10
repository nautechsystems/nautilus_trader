# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model import HIGH_PRECISION
from nautilus_trader.model import Currency
from nautilus_trader.model import Money


USD = Currency.from_str("USD")
AUD = Currency.from_str("AUD")
USDT = Currency.from_str("USDT")


def test_nan_raises():
    with pytest.raises(ValueError, match="NaN"):
        Money(math.nan, currency=USD)


def test_none_value_raises():
    with pytest.raises(TypeError):
        Money(None, currency=USD)


def test_none_currency_raises():
    with pytest.raises(TypeError):
        Money(1.0, None)


def test_value_exceeding_positive_limit_raises():
    with pytest.raises(ValueError, match="not in range"):
        Money(1e18, currency=USD)


def test_value_exceeding_negative_limit_raises():
    with pytest.raises(ValueError, match="not in range"):
        Money(-1e18, currency=USD)


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (0, Money(0, USD)),
        (1, Money(1, USD)),
        (-1, Money(-1, USD)),
    ],
)
def test_construction(value, expected):
    assert Money(value, USD) == expected


def test_as_double():
    money = Money(1.0, USD)
    assert money.as_double() == 1.0
    assert str(money) == "1.00 USD"


def test_rounds_to_currency_precision():
    r1 = Money(1000.333, USD)
    r2 = Money(5005.556666, USD)
    assert str(r1) == "1000.33 USD"
    assert str(r2) == "5005.56 USD"
    assert r1.to_formatted_str() == "1_000.33 USD"
    assert r2.to_formatted_str() == "5_005.56 USD"


def test_equality_different_currencies_raises():
    with pytest.raises(ValueError, match="Cannot compare Money with different currencies"):
        assert Money(1, USD) != Money(1, AUD)


def test_equality():
    m1 = Money(1, USD)
    m2 = Money(1, USD)
    m3 = Money(2, USD)
    assert m1 == m2
    assert m1 != m3


def test_hash():
    m = Money(0, USD)
    assert isinstance(hash(m), int)
    assert hash(m) == hash(m)


def test_str():
    assert str(Money(0, USD)) == "0.00 USD"
    assert str(Money(1, USD)) == "1.00 USD"
    assert str(Money(1_000_000, USD)) == "1000000.00 USD"
    assert Money(1_000_000, USD).to_formatted_str() == "1_000_000.00 USD"


def test_repr():
    assert repr(Money(1.00, USD)) == "Money(1.00, USD)"


def test_from_raw():
    assert Money.from_raw(0, USDT) == Money(0, USDT)


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("1.00 USDT", Money(1.00, USDT)),
        ("1.00 USD", Money(1.00, USD)),
        ("1.001 AUD", Money(1.00, AUD)),
    ],
)
def test_from_str(value, expected):
    result = Money.from_str(value)
    assert result == expected


def test_from_str_malformed_raises():
    with pytest.raises(ValueError, match="invalid input format"):
        Money.from_str("@")


def test_from_decimal():
    money = Money.from_decimal(Decimal("100.50"), USD)
    assert money == Money(100.50, USD)
    assert str(money) == "100.50 USD"


def test_from_decimal_zero():
    money = Money.from_decimal(Decimal(0), USD)
    assert money.as_double() == 0
    assert str(money) == "0.00 USD"


def test_from_decimal_negative():
    money = Money.from_decimal(Decimal("-50.25"), USD)
    assert money.as_double() == -50.25
    assert str(money) == "-50.25 USD"


def test_from_decimal_rounds():
    money = Money.from_decimal(Decimal("100.123"), USD)
    assert str(money) == "100.12 USD"


def test_from_decimal_high_precision():
    money = Money.from_decimal(Decimal("100.12345678"), USDT)
    assert str(money) == "100.12345678 USDT"


def test_pickle():
    money = Money(1, USD)
    pickled = pickle.dumps(money)
    unpickled = pickle.loads(pickled)  # noqa: S301
    assert unpickled == money


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Money(1.00, USD), Money(2.00, USD), Money, Money(3.00, USD)),
        (Money(0.00, USD), Money(0.00, USD), Money, Money(0.00, USD)),
        (Money(-1.00, USD), Money(1.00, USD), Money, Money(0.00, USD)),
        (Money(1.00, USD), 2, Decimal, Decimal("3.00")),
        (2, Money(1.00, USD), Decimal, Decimal("3.00")),
        (Money(1.00, USD), 2.5, float, 3.5),
        (2.5, Money(1.00, USD), float, 3.5),
    ],
)
def test_addition(v1, v2, expected_type, expected):
    result = v1 + v2
    assert isinstance(result, expected_type)
    assert result == expected


def test_addition_different_currencies_raises():
    with pytest.raises(ValueError, match="Currency mismatch"):
        Money(1.00, USD) + Money(1.00, AUD)


def test_is_positive():
    assert Money(1.0, USD).is_positive()
    assert not Money(0.0, USD).is_positive()
    assert not Money(-1.0, USD).is_positive()


def test_checked_add_within_bounds():
    assert Money(100.0, USD).checked_add(Money(50.0, USD)) == Money(150.0, USD)


def test_checked_add_above_max_returns_none():
    money_max = 17_014_118_346_046.0 if HIGH_PRECISION else 9_223_372_036.0
    near_max = Money(money_max, USD)
    one_billion = Money(1_000_000_000.0, USD)
    assert near_max.checked_add(one_billion) is None


def test_checked_sub_within_bounds():
    assert Money(100.0, USD).checked_sub(Money(40.0, USD)) == Money(60.0, USD)


def test_checked_sub_below_min_returns_none():
    money_min = -17_014_118_346_046.0 if HIGH_PRECISION else -9_223_372_036.0
    near_min = Money(money_min, USD)
    one_billion = Money(1_000_000_000.0, USD)
    assert near_min.checked_sub(one_billion) is None


def test_checked_add_currency_mismatch_raises():
    with pytest.raises(ValueError, match="Currency mismatch"):
        Money(100.0, USD).checked_add(Money(50.0, AUD))


def test_checked_sub_currency_mismatch_raises():
    with pytest.raises(ValueError, match="Currency mismatch"):
        Money(100.0, USD).checked_sub(Money(50.0, AUD))


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Money(3.00, USD), Money(2.00, USD), Money, Money(1.00, USD)),
        (Money(1.00, USD), Money(1.00, USD), Money, Money(0.00, USD)),
        (Money(3.00, USD), 2, Decimal, Decimal("1.00")),
        (3, Money(2.00, USD), Decimal, Decimal("1.00")),
        (Money(3.00, USD), 2.5, float, 0.5),
        (3.5, Money(2.00, USD), float, 1.5),
    ],
)
def test_subtraction(v1, v2, expected_type, expected):
    result = v1 - v2
    assert isinstance(result, expected_type)
    assert result == expected


def test_subtraction_different_currencies_raises():
    with pytest.raises(ValueError, match="Currency mismatch"):
        Money(1.00, USD) - Money(1.00, AUD)


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Money(2.00, USD), 3, Decimal, Decimal("6.00")),
        (3, Money(2.00, USD), Decimal, Decimal("6.00")),
        (Money(2.00, USD), 1.5, float, 3.0),
        (1.5, Money(2.00, USD), float, 3.0),
        (Money(2.00, USD), Money(3.00, USD), Decimal, Decimal("6.00")),
    ],
)
def test_multiplication(v1, v2, expected_type, expected):
    result = v1 * v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Money(6.00, USD), 3, Decimal, Decimal("2.00")),
        (6, Money(3.00, USD), Decimal, Decimal("2.00")),
        (Money(6.00, USD), 2.0, float, 3.0),
        (6.0, Money(2.00, USD), float, 3.0),
        (Money(6.00, USD), Money(3.00, USD), Decimal, Decimal("2.00")),
    ],
)
def test_division(v1, v2, expected_type, expected):
    result = v1 / v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Money(7.00, USD), 3, Decimal, Decimal(2)),
        (7, Money(3.00, USD), Decimal, Decimal(2)),
        (Money(7.00, USD), 3.0, float, 2.0),
        (7.0, Money(3.00, USD), float, 2.0),
        (Money(7.00, USD), Money(3.00, USD), Decimal, Decimal(2)),
    ],
)
def test_floor_division(v1, v2, expected_type, expected):
    result = v1 // v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Money(7.00, USD), 3, Decimal, Decimal("1.00")),
        (Money(7.00, USD), 3.0, float, 1.0),
        (Money(7.00, USD), Money(3.00, USD), Decimal, Decimal("1.00")),
    ],
)
def test_mod(v1, v2, expected_type, expected):
    result = v1 % v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Money(1.00, USD), Money(-1.00, USD)),
        (Money(-1.00, USD), Money(1.00, USD)),
        (Money(0.00, USD), Money(0.00, USD)),
    ],
)
def test_neg(value, expected):
    result = -value
    assert isinstance(result, Money)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Money(1.00, USD), Money(1.00, USD)),
        (Money(-1.00, USD), Money(1.00, USD)),
        (Money(0.00, USD), Money(0.00, USD)),
    ],
)
def test_abs(value, expected):
    result = abs(value)
    assert isinstance(result, Money)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Money(50.25, USD), 50),
        (Money(-50.25, USD), -50),
        (Money(0.00, USD), 0),
    ],
)
def test_int(value, expected):
    assert int(value) == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Money(1.00, USD), Money(1.00, USD)),
        (Money(-1.00, USD), Money(-1.00, USD)),
        (Money(0.00, USD), Money(0.00, USD)),
    ],
)
def test_pos(value, expected):
    result = +value
    assert isinstance(result, Money)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Money(0, USD), Decimal("0.00")),
        (Money(1.50, USD), Decimal("1.50")),
        (Money(-1.50, USD), Decimal("-1.50")),
    ],
)
def test_as_decimal(value, expected):
    assert value.as_decimal() == expected


def test_equality_with_none():
    assert Money(1.00, USD) != None  # noqa: E711


@pytest.mark.parametrize("value", ["", "USD", "1.00", "@", "abc USD"])
def test_from_str_invalid_raises(value):
    with pytest.raises(ValueError, match=r"(invalid|Invalid|Error)"):
        Money.from_str(value)


def test_from_str_rounding():
    money = Money.from_str("1.999 USD")
    assert str(money) == "2.00 USD"


def test_from_str_boundary_values():
    large = Money.from_str("1000000000.00 USD")
    assert str(large) == "1000000000.00 USD"

    neg = Money.from_str("-1000000.00 USD")
    assert str(neg) == "-1000000.00 USD"


def test_from_decimal_integer():
    money = Money.from_decimal(Decimal(100), USD)
    assert money == Money(100, USD)
    assert str(money) == "100.00 USD"


@pytest.mark.parametrize(
    ("decimal_val", "currency", "expected_str"),
    [
        (Decimal("1e-2"), USD, "0.01 USD"),
        (Decimal("1.23e1"), USD, "12.30 USD"),
        (Decimal("5e-5"), USDT, "0.00005000 USDT"),
    ],
)
def test_from_decimal_scientific_notation(decimal_val, currency, expected_str):
    money = Money.from_decimal(decimal_val, currency)
    assert str(money) == expected_str


def test_from_decimal_respects_currency_precision():
    money_usd = Money.from_decimal(Decimal("100.123"), USD)
    assert str(money_usd) == "100.12 USD"

    money_usdt = Money.from_decimal(Decimal("100.1234567"), USDT)
    assert str(money_usdt) == "100.12345670 USDT"


def test_from_decimal_high_precision_rounds_to_currency():
    money = Money.from_decimal(Decimal("1.01234567890123456"), USD)
    assert str(money) == "1.01 USD"

    money_usdt = Money.from_decimal(Decimal("100.123456789012345"), USDT)
    assert str(money_usdt) == "100.12345679 USDT"


def test_from_decimal_different_currencies():
    money_usd = Money.from_decimal(Decimal("100.50"), USD)
    money_aud = Money.from_decimal(Decimal("100.50"), AUD)
    assert money_usd.currency == USD
    assert money_aud.currency == AUD

    with pytest.raises(ValueError, match="Cannot compare"):
        _ = money_usd == money_aud


def test_from_decimal_equivalent_to_from_str():
    from_str = Money.from_str("100.50 USD")
    from_dec = Money.from_decimal(Decimal("100.50"), USD)
    assert from_str == from_dec


def test_ordering_with_none_raises():
    money = Money(100.0, USD)
    with pytest.raises(TypeError):
        _ = money < None
    with pytest.raises(TypeError):
        _ = money > None


def test_zero():
    m = Money.zero(USD)
    assert m.is_zero()
    assert str(m) == "0.00 USD"


def test_is_zero():
    assert Money(0, USD).is_zero()
    assert not Money(1, USD).is_zero()


def test_float():
    assert float(Money(1.50, USD)) == 1.5
    assert float(Money(0, USD)) == 0.0
    assert float(Money(-1.50, USD)) == -1.5


def test_round():
    assert round(Money(1.555, USD)) == Decimal(2)
    assert round(Money(1.555, USD), 1) == Decimal("1.6")
