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
"""
Test quantity behavior.
"""

import math
import pickle
from decimal import Decimal

import pytest

from nautilus_trader.model import FIXED_PRECISION
from nautilus_trader.model import HIGH_PRECISION
from nautilus_trader.model import PRECISION_BYTES
from nautilus_trader.model import Quantity


def test_nan_raises() -> None:
    """
    Test nan raises.
    """
    with pytest.raises(ValueError, match="NaN"):
        Quantity(math.nan, precision=0)


def test_none_raises() -> None:
    """
    Test none raises.
    """
    with pytest.raises(TypeError):
        Quantity(None)
    with pytest.raises(TypeError):
        Quantity(None, precision=0)


def test_ordering_with_unsupported_type_raises() -> None:
    """
    Test ordering with unsupported type raises.
    """
    with pytest.raises(TypeError):
        _ = Quantity.from_int(1) < object()


def test_negative_precision_raises() -> None:
    """
    Test negative precision raises.
    """
    with pytest.raises(OverflowError):
        Quantity(1.0, precision=-1)


def test_precision_over_max_raises() -> None:
    """
    Test precision over max raises.
    """
    with pytest.raises(ValueError, match="precision"):
        Quantity(1.0, precision=FIXED_PRECISION + 1)


def test_value_exceeding_limit_raises() -> None:
    """
    Test value exceeding limit raises.
    """
    with pytest.raises(ValueError, match="not in range"):
        Quantity(1e18, precision=0)


def test_from_int() -> None:
    """
    Test from int.
    """
    qty = Quantity(1, precision=1)
    assert str(qty) == "1.0"
    assert qty.precision == 1


def test_from_float() -> None:
    """
    Test from float.
    """
    result = Quantity(1.12300, precision=5)
    expected_raw = int(1.123 * (10**FIXED_PRECISION))
    assert result.raw == expected_raw
    assert str(result) == "1.12300"


def test_from_decimal() -> None:
    """
    Test from decimal.
    """
    result = Quantity(Decimal("1.23"), precision=1)
    assert str(result) == "1.2"


def test_from_str() -> None:
    """
    Test from str.
    """
    result = Quantity.from_str("1.23")
    expected_raw = int(1.23 * (10**FIXED_PRECISION))
    assert result.raw == expected_raw
    assert str(result) == "1.23"


def test_from_int_method() -> None:
    """
    Test from int method.
    """
    qty = Quantity.from_int(1_000)
    assert qty == 1000
    assert str(qty) == "1000"
    assert qty.precision == 0


def test_from_str_method() -> None:
    """
    Test from str method.
    """
    qty = Quantity.from_str("0.511")
    assert qty == Quantity(0.511, precision=3)
    assert str(qty) == "0.511"
    assert qty.precision == 3


def test_zero() -> None:
    """
    Test zero.
    """
    qty = Quantity.zero()
    assert qty == 0
    assert str(qty) == "0"
    assert qty.precision == 0


def test_from_raw() -> None:
    """
    Test from raw.
    """
    raw = 1000 * (10**FIXED_PRECISION)
    qty = Quantity.from_raw(raw, 3)
    assert str(qty) == "1000.000"
    assert qty.precision == 3
    assert qty == Quantity(1000, 3)


def test_from_decimal_infers_precision() -> None:
    """
    Test from decimal infers precision.
    """
    qty = Quantity.from_decimal(Decimal("123.456"))
    assert qty.precision == 3
    assert str(qty) == "123.456"


def test_from_decimal_integer() -> None:
    """
    Test from decimal integer.
    """
    qty = Quantity.from_decimal(Decimal(100))
    assert qty.precision == 0
    assert str(qty) == "100"


def test_from_decimal_high_precision() -> None:
    """
    Test from decimal high precision.
    """
    qty = Quantity.from_decimal(Decimal("1.23456789"))
    assert qty.precision == 8
    assert str(qty) == "1.23456789"


def test_from_decimal_negative_raises() -> None:
    """
    Test from decimal negative raises.
    """
    with pytest.raises(ValueError, match="negative"):
        Quantity.from_decimal(Decimal("-99.95"))


def test_from_decimal_trailing_zeros() -> None:
    """
    Test from decimal trailing zeros.
    """
    qty = Quantity.from_decimal(Decimal("5.670"))
    assert qty.precision == 3
    assert str(qty) == "5.670"


def test_from_decimal_dp() -> None:
    """
    Test from decimal dp.
    """
    qty = Quantity.from_decimal_dp(Decimal("123.456789"), 2)
    assert qty.precision == 2
    assert str(qty) == "123.46"


def test_from_decimal_dp_bankers_rounding() -> None:
    """
    Test from decimal dp bankers rounding.
    """
    q1 = Quantity.from_decimal_dp(Decimal("1.005"), 2)
    q2 = Quantity.from_decimal_dp(Decimal("1.015"), 2)
    assert str(q1) == "1.00"
    assert str(q2) == "1.02"


def test_from_decimal_dp_negative_raises() -> None:
    """
    Test from decimal dp negative raises.
    """
    with pytest.raises(ValueError, match="negative"):
        Quantity.from_decimal_dp(Decimal("-123.45"), 2)


def test_from_decimal_dp_precision_limits() -> None:
    """
    Test from decimal dp precision limits.
    """
    qty = Quantity.from_decimal_dp(Decimal("1.0"), FIXED_PRECISION)
    assert qty.precision == FIXED_PRECISION
    with pytest.raises(ValueError, match="precision"):
        Quantity.from_decimal_dp(Decimal("1.0"), 19)


@pytest.mark.parametrize(
    ("value", "precision", "expected"),
    [
        (0.0, 0, Quantity(0, precision=0)),
        (1.0, 0, Quantity(1, precision=0)),
        (1.123, 3, Quantity(1.123, precision=3)),
        (1.155, 2, Quantity(1.16, precision=2)),
    ],
)
def test_various_precisions(value: object, precision: object, expected: object) -> None:
    """
    Test various precisions.
    """
    result = Quantity(value, precision)
    assert result == expected
    assert result.precision == precision


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (0, -0, True),
        (1, 1, True),
        (1.1, 1.1, True),
        (0, 1, False),
        (1, 2, False),
        (1.1, 1.12, False),
    ],
)
def test_equality(v1: object, v2: object, expected: object) -> None:
    """
    Test equality.
    """
    assert (Quantity(v1, 2) == Quantity(v2, 2)) == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (0, -0, True),
        (1, 1, True),
        (0, 1, False),
        (1, 2, False),
    ],
)
def test_equality_with_int(v1: object, v2: object, expected: object) -> None:
    """
    Test equality with int.
    """
    assert (Quantity(v1, 0) == v2) == expected
    assert (v2 == Quantity(v1, 0)) == expected


@pytest.mark.parametrize(
    ("v1", "v2", "gt", "ge", "le", "lt"),
    [
        (0, 0, False, True, True, False),
        (1, 0, True, True, False, False),
    ],
)
def test_comparisons(
    v1: object,
    v2: object,
    gt: object,
    ge: object,
    le: object,
    lt: object,
) -> None:
    """
    Test comparisons.
    """
    q1, q2 = Quantity(v1, precision=0), Quantity(v2, precision=0)
    assert (q1 > q2) == gt
    assert (q1 >= q2) == ge
    assert (q1 <= q2) == le
    assert (q1 < q2) == lt


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Quantity(0, 0), Quantity(0, 0)),
        (Quantity(1, 0), Quantity(1, 0)),
        (Quantity(1.5, 1), Quantity(1.5, 1)),
    ],
)
def test_abs(value: object, expected: object) -> None:
    """
    Test abs.
    """
    result = abs(value)
    assert isinstance(result, Quantity)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Quantity(0, 0), Quantity(0, 0)),
        (Quantity(1, 0), Quantity(1, 0)),
        (Quantity(1.5, 1), Quantity(1.5, 1)),
    ],
)
def test_pos(value: object, expected: object) -> None:
    """
    Test pos.
    """
    result = +value
    assert isinstance(result, Quantity)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Quantity(1, 0), Decimal(-1)),
        (Quantity(0, 0), Decimal(0)),
    ],
)
def test_neg(value: object, expected: object) -> None:
    """
    Test neg.
    """
    assert -value == expected


@pytest.mark.parametrize(
    ("value", "precision", "expected"),
    [
        (Quantity(2.15, 2), 0, Decimal(2)),
        (Quantity(2.15, 2), 1, Decimal("2.2")),
        (Quantity(2.255, 3), 2, Decimal("2.26")),
    ],
)
def test_round(value: object, precision: object, expected: object) -> None:
    """
    Test round.
    """
    assert round(value, precision) == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Quantity(0, 0), Quantity(0, 0), Quantity, Quantity(0, 0)),
        (Quantity(0, 0), Quantity(1.1, 1), Quantity, Quantity(1.1, 1)),
        (Quantity(1, 0), Quantity(1.1, 1), Quantity, Quantity(2.1, 1)),
        (Quantity(0, 0), 0, Decimal, 0),
        (Quantity(0, 0), 1, Decimal, 1),
        (0, Quantity(0, 0), Decimal, 0),
        (1, Quantity(0, 0), Decimal, 1),
        (Quantity(0, 0), 0.1, float, 0.1),
        (Quantity(0, 0), 1.1, float, 1.1),
        (1.1, Quantity(0, 0), float, 1.1),
        (Quantity(1, 0), Decimal("1.1"), Decimal, Decimal("2.1")),
    ],
)
def test_addition(v1: object, v2: object, expected_type: type, expected: object) -> None:
    """
    Test addition.
    """
    result = v1 + v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Quantity(0, 0), Quantity(0, 0), Quantity, Quantity(0, 0)),
        (Quantity(2.0, 1), Quantity(1.0, 1), Quantity, Quantity(1.0, 1)),
        (Quantity(5.5, 1), Quantity(2.2, 1), Quantity, Quantity(3.3, 1)),
        (Quantity(0, 0), 0, Decimal, 0),
        (Quantity(0, 0), 1, Decimal, -1),
        (0, Quantity(0, 0), Decimal, 0),
        (1, Quantity(1, 0), Decimal, 0),
        (Quantity(0, 0), 0.1, float, -0.1),
        (Quantity(0, 0), 1.1, float, -1.1),
        (Quantity(1, 0), Decimal("1.1"), Decimal, Decimal("-0.1")),
    ],
)
def test_subtraction(v1: object, v2: object, expected_type: type, expected: object) -> None:
    """
    Test subtraction.
    """
    result = v1 - v2
    assert isinstance(result, expected_type)
    assert result == expected


def test_subtraction_negative_result_raises() -> None:
    """
    Test subtraction negative result raises.
    """
    with pytest.raises(ValueError, match="negative"):
        Quantity(1.0, 1) - Quantity(2.0, 1)


def test_saturating_sub_clamps_to_zero() -> None:
    """
    Test saturating sub clamps to zero.
    """
    result = Quantity(1.0, 1).saturating_sub(Quantity(2.0, 1))
    assert result == Quantity.zero(1)


def test_saturating_sub_positive() -> None:
    """
    Test saturating sub positive.
    """
    result = Quantity(5.0, 1).saturating_sub(Quantity(2.0, 1))
    assert result == Quantity(3.0, 1)


def test_checked_add_within_bounds() -> None:
    """
    Test checked add within bounds.
    """
    assert Quantity(10.0, 2).checked_add(Quantity(5.0, 2)) == Quantity(15.0, 2)


def test_checked_sub_within_bounds() -> None:
    """
    Test checked sub within bounds.
    """
    assert Quantity(10.0, 2).checked_sub(Quantity(3.0, 2)) == Quantity(7.0, 2)


def test_checked_sub_to_zero() -> None:
    """
    Test checked sub to zero.
    """
    qty = Quantity(5.0, 2)
    assert qty.checked_sub(qty) == Quantity.zero(2)


def test_checked_sub_underflow_returns_none() -> None:
    """
    Test checked sub underflow returns none.
    """
    assert Quantity(3.0, 2).checked_sub(Quantity(10.0, 2)) is None


def test_checked_add_above_max_returns_none() -> None:
    """
    Test checked add above max returns none.
    """
    qty_max = 34_028_236_692_093.0 if HIGH_PRECISION else 18_446_744_073.0
    near_max = Quantity(qty_max, 0)
    one_billion = Quantity(1_000_000_000.0, 0)
    assert near_max.checked_add(one_billion) is None


def test_checked_arith_rejects_undef_sentinel() -> None:
    """
    Test checked arith rejects undef sentinel.
    """
    # QUANTITY_UNDEF == QuantityRaw::MAX (u128 or u64 max depending on feature flag)
    raw_undef = (1 << (PRECISION_BYTES * 8)) - 1
    undef = Quantity.from_raw(raw_undef, 0)
    one = Quantity(1.0, 0)
    assert undef.checked_add(one) is None
    assert one.checked_add(undef) is None
    assert undef.checked_sub(one) is None
    assert one.checked_sub(undef) is None


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Quantity(0, 0), 0, Decimal, 0),
        (Quantity(1, 0), 1, Decimal, 1),
        (1, Quantity(1, 0), Decimal, 1),
        (2, Quantity(3, 0), Decimal, 6),
        (Quantity(2, 0), 1.0, float, 2),
        (1.1, Quantity(2, 0), float, 2.2),
        (Quantity(1.1, 1), Quantity(1.1, 1), Decimal, Decimal("1.21")),
        (Quantity(1.1, 1), Decimal("1.1"), Decimal, Decimal("1.21")),
    ],
)
def test_multiplication(v1: object, v2: object, expected_type: type, expected: object) -> None:
    """
    Test multiplication.
    """
    result = v1 * v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (1, Quantity(1, 0), Decimal, 1),
        (1.1, Quantity(1.1, 1), float, 1),
        (Quantity(0, 0), 1, Decimal, 0),
        (Quantity(1, 0), 2, Decimal, Decimal("0.5")),
        (2, Quantity(1, 0), Decimal, Decimal("2.0")),
        (Quantity(2, 0), 1.1, float, 1.8181818181818181),
        (Quantity(1.1, 1), Quantity(1.2, 1), Decimal, Decimal("0.9166666666666666666666666667")),
        (Quantity(1.1, 1), Decimal("1.2"), Decimal, Decimal("0.9166666666666666666666666667")),
    ],
)
def test_division(v1: object, v2: object, expected_type: type, expected: object) -> None:
    """
    Test division.
    """
    result = v1 / v2
    assert isinstance(result, expected_type)
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (1, Quantity(1, 0), Decimal, 1),
        (Quantity(0, 0), 1, Decimal, 0),
        (Quantity(1, 0), 2, Decimal, Decimal(0)),
        (2, Quantity(1, 0), Decimal, Decimal(2)),
        (2.1, Quantity(1.1, 1), float, 1),
        (Quantity(2.1, 1), 1.1, float, 1),
        (Quantity(1.1, 1), Quantity(1.2, 1), Decimal, Decimal(0)),
        (Quantity(1.1, 1), Decimal("1.2"), Decimal, Decimal(0)),
    ],
)
def test_floor_division(v1: object, v2: object, expected_type: type, expected: object) -> None:
    """
    Test floor division.
    """
    result = v1 // v2
    assert type(result) is expected_type
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected_type", "expected"),
    [
        (Quantity(1, 0), 1, Decimal, 0),
        (Quantity(100, 0), 10, Decimal, 0),
        (Quantity(23, 0), 2, Decimal, 1),
        (2.1, Quantity(1.1, 1), float, 1.0),
        (Quantity(2.1, 1), 1.1, float, 1.0),
        (Quantity(1.1, 1), Decimal("0.2"), Decimal, Decimal("0.1")),
    ],
)
def test_mod(v1: object, v2: object, expected_type: type, expected: object) -> None:
    """
    Test mod.
    """
    result = v1 % v2
    assert type(result) is expected_type
    assert result == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (Quantity(1, 0), Quantity(2, 0), Quantity(2, 0)),
        (Quantity(1, 0), 2, 2),
        (Quantity(1, 0), Decimal(2), Decimal(2)),
    ],
)
def test_max(v1: object, v2: object, expected: object) -> None:
    """
    Test max.
    """
    assert max(v1, v2) == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (Quantity(1, 0), Quantity(2, 0), Quantity(1, 0)),
        (Quantity(1, 0), 2, Quantity(1, 0)),
        (Quantity(2, 0), Decimal(1), Decimal(1)),
    ],
)
def test_min(v1: object, v2: object, expected: object) -> None:
    """
    Test min.
    """
    assert min(v1, v2) == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("0", 0),
        ("0.000000001", 0),
        ("1.999999999", 1),
        ("50.25", 50),
        ("9007199253.999999999", 9_007_199_253),
    ],
)
def test_int(value: object, expected: object) -> None:
    """
    Test int.
    """
    assert int(Quantity.from_str(value)) == expected


def test_hash() -> None:
    """
    Test hash.
    """
    q1 = Quantity(1.1, 1)
    q2 = Quantity(1.1, 1)
    assert isinstance(hash(q1), int)
    assert hash(q1) == hash(q2)


@pytest.mark.parametrize(
    ("value", "precision", "expected"),
    [
        (0, 0, "0"),
        (-0, 0, "0"),
        (1, 0, "1"),
        (1.1, 1, "1.1"),
    ],
)
def test_str(value: object, precision: object, expected: object) -> None:
    """
    Test str.
    """
    assert str(Quantity(value, precision=precision)) == expected


def test_repr() -> None:
    """
    Test repr.
    """
    assert repr(Quantity(1.1, 1)) == "Quantity(1.1)"
    qty = Quantity(2100.1666666, 6)
    assert str(qty) == "2100.166667"
    assert repr(qty) == "Quantity(2100.166667)"


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("0", "0"),
        ("10.05", "10.05"),
        ("1000", "1_000"),
        ("1112", "1_112"),
        ("120100", "120_100"),
        ("200000", "200_000"),
        ("1000000", "1_000_000"),
        ("2500000", "2_500_000"),
        ("1111111", "1_111_111"),
        ("2523000", "2_523_000"),
        ("100000000", "100_000_000"),
    ],
)
def test_to_formatted_str(value: object, expected: object) -> None:
    """
    Test to formatted str.
    """
    assert Quantity.from_str(value).to_formatted_str() == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [(0, 0), (-0, 0), (1, 1), (1.1, 1.1)],
)
def test_as_double(value: object, expected: object) -> None:
    """
    Test as double.
    """
    assert Quantity(value, 1).as_double() == expected


def test_pickle() -> None:
    """
    Test pickle.
    """
    qty = Quantity(1.2000, 2)
    pickled = pickle.dumps(qty)
    assert pickle.loads(pickled) == qty


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Quantity(0, 0), Decimal(0)),
        (Quantity(1, 0), Decimal(1)),
        (Quantity(1.1, 1), Decimal("1.1")),
        (Quantity(1.23, 2), Decimal("1.23")),
    ],
)
def test_as_decimal(value: object, expected: object) -> None:
    """
    Test as decimal.
    """
    assert value.as_decimal() == expected


@pytest.mark.parametrize(
    ("v1", "v2", "expected"),
    [
        (Quantity(1.1, 1), Decimal("1.1"), True),
        (Quantity(1.1, 1), Decimal("1.2"), False),
        (Quantity(0, 0), Decimal(0), True),
    ],
)
def test_equality_with_decimal(v1: object, v2: object, expected: object) -> None:
    """
    Test equality with decimal.
    """
    assert (v1 == v2) == expected


def test_equality_with_none() -> None:
    """
    Test equality with none.
    """
    assert Quantity(1.0, 1) != None


@pytest.mark.parametrize(
    "value",
    ["not_a_number", "1.2.3", "++1", "--1", "1e", "e10", "1e1e1", "", "nan", "inf", "-inf"],
)
def test_from_str_invalid_raises(value: object) -> None:
    """
    Test from str invalid raises.
    """
    with pytest.raises(ValueError, match=r"(Error parsing|negative|exceeds)"):
        Quantity.from_str(value)


@pytest.mark.parametrize("value", ["-1", "-0.5", "-1e3", "-1.0", "-0.001"])
def test_from_str_negative_raises(value: object) -> None:
    """
    Test from str negative raises.
    """
    with pytest.raises(ValueError, match="negative"):
        Quantity.from_str(value)


@pytest.mark.parametrize(
    ("value", "expected_str", "expected_precision"),
    [
        ("1e6", "1000000", 0),
        ("1E6", "1000000", 0),
        ("2.5e4", "25000", 0),
        ("3.5E-2", "0.035", 3),
        ("7.89E1", "78.9", 1),
        ("1_000", "1000", 0),
        ("1_000.25", "1000.25", 2),
        ("9_876_543.21", "9876543.21", 2),
        ("0.000_123", "0.000123", 6),
        ("1_000e2", "100000", 0),
        ("0e0", "0", 0),
        ("0E-3", "0.000", 3),
        ("2.115", "2.115", 3),
        ("2.125", "2.125", 3),
        ("2.155", "2.155", 3),
    ],
)
def test_from_str_comprehensive(
    value: object,
    expected_str: object,
    expected_precision: object,
) -> None:
    """
    Test from str comprehensive.
    """
    qty = Quantity.from_str(value)
    assert str(qty) == expected_str
    assert qty.precision == expected_precision


@pytest.mark.parametrize(
    ("value", "expected_str", "expected_precision"),
    [
        ("0", "0", 0),
        ("0.0", "0.0", 1),
        ("0.00", "0.00", 2),
    ],
)
def test_from_str_zero_values(
    value: object,
    expected_str: object,
    expected_precision: object,
) -> None:
    """
    Test from str zero values.
    """
    qty = Quantity.from_str(value)
    assert str(qty) == expected_str
    assert qty.precision == expected_precision


def test_from_str_boundary_values() -> None:
    """
    Test from str boundary values.
    """
    large = Quantity.from_str("1000000000")
    assert str(large) == "1000000000"

    with pytest.raises(ValueError, match="exceeds"):
        Quantity.from_str("999999999999999999")


def test_from_str_precision_preservation() -> None:
    """
    Test from str precision preservation.
    """
    assert Quantity.from_str("100").precision == 0
    assert Quantity.from_str("1000000").precision == 0
    assert Quantity.from_str("100.0").precision == 1
    assert Quantity.from_str("100.00").precision == 2
    assert Quantity.from_str("100.12345").precision == 5
    assert Quantity.from_str("1_000.123").precision == 3
    assert Quantity.from_str("1_000").precision == 0

    qty = Quantity.from_str("1.23e-2")
    assert str(qty) == "0.0123"
    assert qty.precision == 4


def test_from_decimal_zero() -> None:
    """
    Test from decimal zero.
    """
    q1 = Quantity.from_decimal(Decimal(0))
    assert str(q1) == "0"
    assert q1.precision == 0

    q2 = Quantity.from_decimal(Decimal("0.00"))
    assert str(q2) == "0.00"
    assert q2.precision == 2


@pytest.mark.parametrize(
    ("value", "expected_str", "expected_precision"),
    [
        (Decimal("1E-4"), "0.0001", 4),
        (Decimal("1E2"), "100", 0),
        (Decimal("1e-2"), "0.01", 2),
        (Decimal("5e-5"), "0.00005", 5),
    ],
)
def test_from_decimal_scientific_notation(
    value: object,
    expected_str: object,
    expected_precision: object,
) -> None:
    """
    Test from decimal scientific notation.
    """
    qty = Quantity.from_decimal(value)
    assert str(qty) == expected_str
    assert qty.precision == expected_precision


def test_from_decimal_precision_preservation() -> None:
    """
    Test from decimal precision preservation.
    """
    assert Quantity.from_decimal(Decimal(100)).precision == 0
    assert Quantity.from_decimal(Decimal(1000000)).precision == 0
    assert Quantity.from_decimal(Decimal("100.0")).precision == 1
    assert Quantity.from_decimal(Decimal("100.00")).precision == 2
    assert Quantity.from_decimal(Decimal("100.12345")).precision == 5


def test_from_decimal_equivalent_to_from_str() -> None:
    """
    Test from decimal equivalent to from str.
    """
    for value in ["1.23", "100.00", "0.001", "99999.9", "0.5", "1234.5678"]:
        from_str = Quantity.from_str(value)
        from_dec = Quantity.from_decimal(Decimal(value))
        assert from_str == from_dec
        assert from_str.precision == from_dec.precision


def test_is_zero() -> None:
    """
    Test is zero.
    """
    assert Quantity.zero().is_zero()
    assert Quantity(0, 2).is_zero()
    assert not Quantity(1, 0).is_zero()


def test_is_positive() -> None:
    """
    Test is positive.
    """
    assert Quantity(1, 0).is_positive()
    assert not Quantity(0, 0).is_positive()


def test_float() -> None:
    """
    Test float.
    """
    assert float(Quantity(1.5, 1)) == 1.5
    assert float(Quantity(0, 0)) == 0.0


def test_round_no_ndigits() -> None:
    """
    Test round no ndigits.
    """
    result = round(Quantity(1.6, 1))
    assert result == Decimal(2)


def test_from_mantissa_exponent() -> None:
    """
    Test from mantissa exponent.
    """
    q = Quantity.from_mantissa_exponent(12345, -2, 2)
    assert str(q) == "123.45"
    assert q.precision == 2

    q2 = Quantity.from_mantissa_exponent(100, 0, 0)
    assert str(q2) == "100"
