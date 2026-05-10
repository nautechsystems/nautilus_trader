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

from nautilus_trader.model import FIXED_PRECISION
from nautilus_trader.model import FIXED_SCALAR
from nautilus_trader.model import HIGH_PRECISION
from nautilus_trader.model import PRECISION_BYTES
from nautilus_trader.model import Price


def test_fixed_point_constants_are_consistent():
    if HIGH_PRECISION:
        assert FIXED_PRECISION == 16
        assert FIXED_SCALAR == 1e16
        assert PRECISION_BYTES == 16
    else:
        assert FIXED_PRECISION == 9
        assert FIXED_SCALAR == 1e9
        assert PRECISION_BYTES == 8


def test_nan_raises():
    with pytest.raises(ValueError, match="NaN"):
        Price(math.nan, precision=0)


def test_none_raises():
    with pytest.raises(TypeError):
        Price(None, precision=0)


def test_negative_precision_raises():
    with pytest.raises(OverflowError):
        Price(1.0, precision=-1)


def test_precision_over_max_raises():
    with pytest.raises(ValueError, match="precision"):
        Price(1.0, precision=FIXED_PRECISION + 1)


def test_value_exceeding_positive_limit_raises():
    with pytest.raises(ValueError, match="not in range"):
        Price(1e18, precision=0)


def test_value_exceeding_negative_limit_raises():
    with pytest.raises(ValueError, match="not in range"):
        Price(-1e18, precision=0)


def test_from_int():
    result = Price(1, precision=1)
    assert result.raw == 10**FIXED_PRECISION
    assert str(result) == "1.0"


def test_from_float():
    result = Price(1.12300, precision=5)
    expected_raw = int(1.123 * (10**FIXED_PRECISION))
    assert result.raw == expected_raw
    assert str(result) == "1.12300"


def test_from_decimal():
    result = Price(Decimal("1.23"), precision=1)
    assert str(result) == "1.2"


def test_from_str():
    result = Price.from_str("1.23")
    assert str(result) == "1.23"


def test_from_int_method():
    price = Price.from_int(100)
    assert str(price) == "100"
    assert price.precision == 0


@pytest.mark.parametrize(
    ("value", "string", "precision"),
    [
        ("100.11", "100.11", 2),
        ("1E7", "10000000", 0),
        ("1E-7", "0.0000001", 7),
        ("1e-2", "0.01", 2),
    ],
)
def test_from_str_various(value, string, precision):
    price = Price.from_str(value)
    assert str(price) == string
    assert price.precision == precision


def test_from_raw():
    raw = 1000 * (10**FIXED_PRECISION)
    price = Price.from_raw(raw, 3)
    assert str(price) == "1000.000"
    assert price.precision == 3
    assert price == Price(1000, 3)


def test_from_decimal_infers_precision():
    price = Price.from_decimal(Decimal("123.456"))
    assert price.precision == 3
    assert str(price) == "123.456"


def test_from_decimal_integer():
    price = Price.from_decimal(Decimal(100))
    assert price.precision == 0
    assert str(price) == "100"


def test_from_decimal_high_precision():
    price = Price.from_decimal(Decimal("1.23456789"))
    assert price.precision == 8
    assert str(price) == "1.23456789"


def test_from_decimal_negative():
    price = Price.from_decimal(Decimal("-99.95"))
    assert price.precision == 2
    assert str(price) == "-99.95"


def test_from_decimal_trailing_zeros():
    price = Price.from_decimal(Decimal("1.230"))
    assert price.precision == 3
    assert str(price) == "1.230"


def test_from_decimal_dp():
    price = Price.from_decimal_dp(Decimal("123.456789"), 2)
    assert price.precision == 2
    assert str(price) == "123.46"


def test_from_decimal_dp_bankers_rounding():
    p1 = Price.from_decimal_dp(Decimal("1.005"), 2)
    p2 = Price.from_decimal_dp(Decimal("1.015"), 2)
    assert str(p1) == "1.00"
    assert str(p2) == "1.02"


def test_from_decimal_dp_precision_limits():
    price = Price.from_decimal_dp(Decimal("1.0"), FIXED_PRECISION)
    assert price.precision == FIXED_PRECISION

    with pytest.raises(ValueError, match="precision"):
        Price.from_decimal_dp(Decimal("1.0"), 19)


@pytest.mark.parametrize(
    ("value", "precision", "expected"),
    [
        (0.0, 0, Price(0, precision=0)),
        (1.0, 0, Price(1, precision=0)),
        (-1.0, 0, Price(-1, precision=0)),
        (1.123, 3, Price(1.123, precision=3)),
        (-1.123, 3, Price(-1.123, precision=3)),
        (1.155, 2, Price(1.16, precision=2)),
    ],
)
def test_various_precisions(value, precision, expected):
    result = Price(value, precision)
    assert result == expected
    assert result.precision == precision


def test_equality():
    p1 = Price(1.0, precision=1)
    p2 = Price(1.5, precision=1)
    assert p1 == p1
    assert p1 != p2
    assert p2 > p1


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (0, -0, True),
        (-1, -1, True),
        (1, 1, True),
        (1.1, 1.1, True),
        (0, 1, False),
        (-1, 0, False),
        (1.1, 1.12, False),
    ],
)
def test_equality_parametrized(v1, v2, expected):
    assert (Price(v1, 2) == Price(v2, 2)) == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (0, -0, True),
        (1, 1, True),
        (0, 1, False),
        (-1, 0, False),
    ],
)
def test_equality_with_int(v1, v2, expected):
    assert (Price(v1, 0) == v2) == expected
    assert (v2 == Price(v1, 0)) == expected


@pytest.mark.parametrize(
    ("v1", "v2", "gt", "ge", "le", "lt"),
    [
        (0, 0, False, True, True, False),
        (1, 0, True, True, False, False),
        (-1, 0, False, False, True, True),
    ],
)
def test_comparisons(v1, v2, gt, ge, le, lt):
    p1, p2 = Price(v1, precision=0), Price(v2, precision=0)
    assert (p1 > p2) == gt
    assert (p1 >= p2) == ge
    assert (p1 <= p2) == le
    assert (p1 < p2) == lt


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Price(1, 0), Price(-1, 0)),
        (Price(-1, 0), Price(1, 0)),
        (Price(0, 0), Price(0, 0)),
        (Price(-1.5, 1), Price(1.5, 1)),
    ],
)
def test_neg(value, expected):
    result = -value
    assert isinstance(result, Price)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Price(-0, 0), Price(0, 0)),
        (Price(0, 0), Price(0, 0)),
        (Price(1, 0), Price(1, 0)),
        (Price(-1, 0), Price(1, 0)),
        (Price(-1.1, 1), Price(1.1, 1)),
    ],
)
def test_abs(value, expected):
    result = abs(value)
    assert isinstance(result, Price)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "precision", "expected"),
    [
        (Price(2.15, 2), 0, Decimal(2)),
        (Price(2.15, 2), 1, Decimal("2.2")),
        (Price(2.255, 3), 2, Decimal("2.26")),
    ],
)
def test_round(value, precision, expected):
    assert round(value, precision) == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Price(0, 0), Price(0, 0), Price, Price(0, 0)),
        (Price(0, 0), Price(1.1, 1), Price, Price(1.1, 1)),
        (Price(1, 0), Price(1.1, 1), Price, Price(2.1, 1)),
        (Price(0, 0), 0, Decimal, 0),
        (Price(0, 0), 1, Decimal, 1),
        (0, Price(0, 0), Decimal, 0),
        (1, Price(0, 0), Decimal, 1),
        (Price(0, 0), 0.1, float, 0.1),
        (Price(0, 0), 1.1, float, 1.1),
        (1.1, Price(0, 0), float, 1.1),
        (Price(1, 0), Decimal("1.1"), Decimal, Decimal("2.1")),
    ],
)
def test_addition(v1, v2, expected_type, expected):
    result = v1 + v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Price(0, 0), Price(0, 0), Price, Price(0, 0)),
        (Price(0, 0), Price(1.1, 1), Price, Price(-1.1, 1)),
        (Price(1, 0), Price(1.1, 1), Price, Price(-0.1, 1)),
        (Price(0, 0), 0, Decimal, 0),
        (Price(0, 0), 1, Decimal, -1),
        (0, Price(0, 0), Decimal, 0),
        (1, Price(1, 0), Decimal, 0),
        (Price(0, 0), 0.1, float, -0.1),
        (Price(0, 0), 1.1, float, -1.1),
        (Price(1, 0), Decimal("1.1"), Decimal, Decimal("-0.1")),
    ],
)
def test_subtraction(v1, v2, expected_type, expected):
    result = v1 - v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Price(0, 0), 0, Decimal, 0),
        (Price(1, 0), 1, Decimal, 1),
        (1, Price(1, 0), Decimal, 1),
        (2, Price(3, 0), Decimal, 6),
        (Price(2, 0), 1.0, float, 2),
        (1.1, Price(2, 0), float, 2.2),
        (Price(1.1, 1), Price(1.1, 1), Decimal, Decimal("1.21")),
        (Price(1.1, 1), Decimal("1.1"), Decimal, Decimal("1.21")),
    ],
)
def test_multiplication(v1, v2, expected_type, expected):
    result = v1 * v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (1, Price(1, 0), Decimal, 1),
        (Price(0, 0), 1, Decimal, 0),
        (Price(1, 0), 2, Decimal, Decimal("0.5")),
        (2, Price(1, 0), Decimal, Decimal("2.0")),
        (Price(2, 0), 1.1, float, 1.8181818181818181),
        (1.1, Price(2, 0), float, 1.1 / 2),
        (Price(1.1, 1), Price(1.2, 1), Decimal, Decimal("0.9166666666666666666666666667")),
        (Price(1.1, 1), Decimal("1.2"), Decimal, Decimal("0.9166666666666666666666666667")),
    ],
)
def test_division(v1, v2, expected_type, expected):
    result = v1 / v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (1, Price(1, 0), Decimal, 1),
        (Price(0, 0), 1, Decimal, 0),
        (Price(1, 0), 2, Decimal, Decimal(0)),
        (2, Price(1, 0), Decimal, Decimal(2)),
        (2.1, Price(1.1, 1), float, 1),
        (Price(2.1, 1), 1.1, float, 1),
        (Price(1.1, 1), Price(1.2, 1), Decimal, Decimal(0)),
        (Price(1.1, 1), Decimal("1.2"), Decimal, Decimal(0)),
    ],
)
def test_floor_division(v1, v2, expected_type, expected):
    result = v1 // v2
    assert type(result) is expected_type
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (1, Price(1, 0), Decimal, 0),
        (Price(100, 0), 10, Decimal, 0),
        (Price(23, 0), 2, Decimal, 1),
        (2.1, Price(1.1, 1), float, 1.0),
        (Price(2.1, 1), 1.1, float, 1.0),
        (Price(1.1, 1), Price(0.2, 1), Decimal, Decimal("0.1")),
    ],
)
def test_mod(v1, v2, expected_type, expected):
    result = v1 % v2
    assert type(result) is expected_type
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (Price(1, 0), Price(2, 0), Price(2, 0)),
        (Price(1, 0), 2, 2),
        (Price(1, 0), Decimal(2), Decimal(2)),
    ],
)
def test_max(v1, v2, expected):
    assert max(v1, v2) == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (Price(1, 0), Price(2, 0), Price(1, 0)),
        (Price(1, 0), 2, Price(1, 0)),
        (Price(2, 0), Decimal(1), Decimal(1)),
    ],
)
def test_min(v1, v2, expected):
    assert min(v1, v2) == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [("1", 1), ("1.1", 1)],
)
def test_int(value, expected):
    assert int(Price.from_str(value)) == expected


def test_hash():
    p1 = Price(1.1, 1)
    p2 = Price(1.1, 1)
    assert isinstance(hash(p1), int)
    assert hash(p1) == hash(p2)


@pytest.mark.parametrize(
    ("value", "precision", "expected"),
    [
        (0, 0, "0"),
        (-0, 0, "0"),
        (-1, 0, "-1"),
        (1, 0, "1"),
        (1.1, 1, "1.1"),
        (-1.1, 1, "-1.1"),
    ],
)
def test_str(value, precision, expected):
    assert str(Price(value, precision=precision)) == expected


def test_repr():
    assert repr(Price(1.1, 1)) == "Price(1.1)"
    assert repr(Price(1.00000, 5)) == "Price(1.00000)"


@pytest.mark.parametrize(
    ("value", "expected"),
    [(0, 0), (-0, 0), (-1, -1), (1, 1), (1.1, 1.1), (-1.1, -1.1)],
)
def test_as_double(value, expected):
    assert Price(value, 1).as_double() == expected


def test_pickle():
    price = Price(1.2000, 2)
    pickled = pickle.dumps(price)
    assert pickle.loads(pickled) == price  # noqa: S301


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Price(1, 0), Price(1, 0)),
        (Price(-1, 0), Price(-1, 0)),
        (Price(0, 0), Price(0, 0)),
        (Price(1.5, 1), Price(1.5, 1)),
    ],
)
def test_pos(value, expected):
    result = +value
    assert isinstance(result, Price)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Price(0, 0), Decimal(0)),
        (Price(1, 0), Decimal(1)),
        (Price(-1, 0), Decimal(-1)),
        (Price(1.1, 1), Decimal("1.1")),
    ],
)
def test_as_decimal(value, expected):
    assert value.as_decimal() == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (Price(1.1, 1), Decimal("1.1"), True),
        (Price(1.1, 1), Decimal("1.2"), False),
        (Price(0, 0), Decimal(0), True),
    ],
)
def test_equality_with_decimal(v1, v2, expected):
    assert (v1 == v2) == expected


def test_equality_with_none():
    assert Price(1.0, 1) != None  # noqa: E711


@pytest.mark.parametrize(
    "value",
    ["not_a_number", "1.2.3", "++1", "--1", "1e", "e10", "1e1e1", "", "nan", "inf", "-inf"],
)
def test_from_str_invalid_raises(value):
    with pytest.raises((ValueError, OverflowError)):
        Price.from_str(value)


@pytest.mark.parametrize(
    ("value", "expected_str", "expected_precision"),
    [
        ("1e7", "10000000", 0),
        ("1E7", "10000000", 0),
        ("1.5e6", "1500000", 0),
        ("2.5E-3", "0.0025", 4),
        ("9.876E2", "987.6", 1),
        ("1_000", "1000", 0),
        ("1_000.50", "1000.50", 2),
        ("1_234_567.89", "1234567.89", 2),
        ("0.000_001", "0.000001", 6),
        ("1_000e3", "1000000", 0),
        ("-1e7", "-10000000", 0),
        ("-2.5E-3", "-0.0025", 4),
        ("0e0", "0", 0),
        ("0E-5", "0.00000", 5),
    ],
)
def test_from_str_comprehensive(value, expected_str, expected_precision):
    price = Price.from_str(value)
    assert str(price) == expected_str
    assert price.precision == expected_precision


@pytest.mark.parametrize(
    ("value", "expected_str", "expected_precision"),
    [
        ("0", "0", 0),
        ("0.0", "0.0", 1),
        ("0.00", "0.00", 2),
        ("-0.0", "0.0", 1),
    ],
)
def test_from_str_zero_values(value, expected_str, expected_precision):
    price = Price.from_str(value)
    assert str(price) == expected_str
    assert price.precision == expected_precision
    assert price.as_double() == 0


def test_from_str_boundary_values():
    large = Price.from_str("1000000000")
    assert str(large) == "1000000000"

    neg = Price.from_str("-1000000")
    assert str(neg) == "-1000000"

    with pytest.raises(ValueError, match="outside valid range"):
        Price.from_str("999999999999999999")


def test_from_str_precision_preservation():
    assert Price.from_str("100").precision == 0
    assert Price.from_str("1000000").precision == 0
    assert Price.from_str("100.0").precision == 1
    assert Price.from_str("100.00").precision == 2
    assert Price.from_str("100.12345").precision == 5
    assert Price.from_str("1_000.123").precision == 3
    assert Price.from_str("1_000").precision == 0

    price = Price.from_str("1.23e-2")
    assert str(price) == "0.0123"
    assert price.precision == 4


@pytest.mark.parametrize(
    ("input_val", "expected"),
    [
        ("1.115", "1.115"),
        ("1.125", "1.125"),
        ("1.135", "1.135"),
        ("1.145", "1.145"),
        ("1.155", "1.155"),
        ("0.9999999999999999", "0.9999999999999999"),
        ("1.0000000000000001", "1.0000000000000001"),
    ],
)
def test_from_str_rounding_behavior(input_val, expected):
    price = Price.from_str(input_val)
    assert str(price) == expected


def test_from_decimal_zero():
    p1 = Price.from_decimal(Decimal(0))
    assert str(p1) == "0"
    assert p1.precision == 0

    p2 = Price.from_decimal(Decimal("0.00"))
    assert str(p2) == "0.00"
    assert p2.precision == 2


@pytest.mark.parametrize(
    ("value", "expected_str", "expected_precision"),
    [
        (Decimal("1E-4"), "0.0001", 4),
        (Decimal("1E2"), "100", 0),
        (Decimal("1e-2"), "0.01", 2),
        (Decimal("1.23e1"), "12.3", 1),
        (Decimal("5e-5"), "0.00005", 5),
    ],
)
def test_from_decimal_scientific_notation(value, expected_str, expected_precision):
    price = Price.from_decimal(value)
    assert str(price) == expected_str
    assert price.precision == expected_precision


def test_from_decimal_very_small_values():
    price = Price.from_decimal(Decimal("0.0000000000000001"))
    assert str(price) == "0.0000000000000001"
    assert price.precision == 16


def test_from_decimal_precision_preservation():
    assert Price.from_decimal(Decimal(100)).precision == 0
    assert Price.from_decimal(Decimal(1000000)).precision == 0
    assert Price.from_decimal(Decimal("100.0")).precision == 1
    assert Price.from_decimal(Decimal("100.00")).precision == 2
    assert Price.from_decimal(Decimal("100.12345")).precision == 5


def test_from_decimal_equivalent_to_from_str():
    for value in ["1.23", "100.00", "0.001", "99999.9", "0.5", "1234.5678", "-99.99"]:
        from_str = Price.from_str(value)
        from_dec = Price.from_decimal(Decimal(value))
        assert from_str == from_dec
        assert from_str.precision == from_dec.precision


def test_zero():
    p = Price.zero(2)
    assert str(p) == "0.00"
    assert p.precision == 2
    assert p.is_zero()


def test_is_zero():
    assert Price(0, 2).is_zero()
    assert not Price(1.0, 1).is_zero()


def test_is_positive():
    assert Price(1.0, 1).is_positive()
    assert not Price(-1.0, 1).is_positive()
    assert not Price(0, 0).is_positive()


def test_checked_add_within_bounds():
    assert Price(10.0, 2).checked_add(Price(5.0, 2)) == Price(15.0, 2)
    assert Price(10.0, 2).checked_add(Price(-3.0, 2)) == Price(7.0, 2)


def test_checked_sub_within_bounds():
    assert Price(10.0, 2).checked_sub(Price(3.0, 2)) == Price(7.0, 2)
    assert Price(3.0, 2).checked_sub(Price(10.0, 2)) == Price(-7.0, 2)


def test_checked_arith_uses_max_precision():
    sum_ = Price(10.5, 1).checked_add(Price(5.25, 2))
    assert sum_ is not None
    assert sum_.precision == 2
    assert float(sum_) == 15.75


def test_checked_add_above_max_returns_none():
    price_max = 17_014_118_346_046.0 if HIGH_PRECISION else 9_223_372_036.0
    near_max = Price(price_max, 0)
    assert near_max.checked_add(Price(1_000_000_000.0, 0)) is None


def test_checked_sub_below_min_returns_none():
    price_min = -17_014_118_346_046.0 if HIGH_PRECISION else -9_223_372_036.0
    near_min = Price(price_min, 0)
    assert near_min.checked_sub(Price(1_000_000_000.0, 0)) is None


def test_checked_arith_rejects_undef_sentinel():
    # PRICE_UNDEF == PriceRaw::MAX (i128 or i64 max depending on feature flag)
    raw_undef = (1 << (PRECISION_BYTES * 8 - 1)) - 1
    undef = Price.from_raw(raw_undef, 0)
    one = Price(1.0, 0)
    assert undef.checked_add(one) is None
    assert one.checked_add(undef) is None
    assert undef.checked_sub(one) is None
    assert one.checked_sub(undef) is None


def test_checked_arith_rejects_error_sentinel():
    # PRICE_ERROR == PriceRaw::MIN (i128 or i64 min depending on feature flag)
    raw_error = -(1 << (PRECISION_BYTES * 8 - 1))
    error = Price.from_raw(raw_error, 0)
    one = Price(1.0, 0)
    assert error.checked_add(one) is None
    assert one.checked_sub(error) is None


def test_float():
    assert float(Price(1.5, 1)) == 1.5
    assert float(Price(0, 0)) == 0.0
    assert float(Price(-1.5, 1)) == -1.5


def test_to_formatted_str():
    assert Price.from_str("1000000.50").to_formatted_str() == "1_000_000.50"
    assert Price.from_str("999.99").to_formatted_str() == "999.99"
    assert Price.from_str("0").to_formatted_str() == "0"


def test_round_no_ndigits():
    result = round(Price(1.6, 1))
    assert result == Decimal(2)


def test_from_mantissa_exponent():
    p = Price.from_mantissa_exponent(12345, -2, 2)
    assert str(p) == "123.45"
    assert p.precision == 2

    p2 = Price.from_mantissa_exponent(100, 0, 0)
    assert str(p2) == "100"
