# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Test fixed arithmetic behavior.
"""

import math
import sys
from decimal import Decimal

import pytest

from nautilus_trader.model import FIXED_PRECISION
from nautilus_trader.model import HIGH_PRECISION
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyType
from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity


USD = Currency.from_str("USD")
TST = Currency(
    code="TST",
    precision=FIXED_PRECISION,
    iso4217=0,
    name="Test currency",
    currency_type=CurrencyType.CRYPTO,
)
WEI_PRECISION = 18
WEI = Currency(
    code="WEI",
    precision=WEI_PRECISION,
    iso4217=0,
    name="Wei currency",
    currency_type=CurrencyType.CRYPTO,
)
FRACTIONAL = Currency(
    code="FRC",
    precision=3,
    iso4217=0,
    name="Fractional currency",
    currency_type=CurrencyType.CRYPTO,
)
DECIMAL_MAX = Decimal(79228162514264337593543950335)
DECIMAL_MIN = Decimal(-79228162514264337593543950335)
FLOAT_MIN_SUBNORMAL = float.fromhex("0x0.0000000000001p-1022")

TYPE_CASES = [
    pytest.param(Price, id="price"),
    pytest.param(Quantity, id="quantity"),
    pytest.param(Money, id="money"),
]
OPERAND_CASES = [
    pytest.param("same_type", id="same-type"),
    pytest.param("decimal", id="decimal"),
    pytest.param("float", id="float"),
    pytest.param("int", id="int"),
    pytest.param("str", id="str"),
]
ARITHMETIC_CASES = [
    pytest.param("__add__", 2, 3, 5, id="add"),
    pytest.param("__radd__", 2, 3, 5, id="radd"),
    pytest.param("__sub__", 3, 2, 1, id="sub"),
    pytest.param("__rsub__", 2, 3, 1, id="rsub"),
    pytest.param("__mul__", 2, 3, 6, id="mul"),
    pytest.param("__rmul__", 2, 3, 6, id="rmul"),
    pytest.param("__truediv__", 6, 4, Decimal("1.5"), id="truediv"),
    pytest.param("__rtruediv__", 4, 6, Decimal("1.5"), id="rtruediv"),
    pytest.param("__floordiv__", 7, 3, 2, id="floordiv"),
    pytest.param("__rfloordiv__", 3, 7, 2, id="rfloordiv"),
    pytest.param("__mod__", 7, 3, 1, id="mod"),
    pytest.param("__rmod__", 3, 7, 1, id="rmod"),
]
ARITHMETIC_DUNDERS = [case.values[0] for case in ARITHMETIC_CASES]
ZERO_DIVISOR_DUNDERS = [
    "__truediv__",
    "__rtruediv__",
    "__floordiv__",
    "__rfloordiv__",
    "__mod__",
    "__rmod__",
]
DIRECT_ZERO_DIVISOR_DUNDERS = ["__truediv__", "__floordiv__", "__mod__"]
FLOAT_OVERFLOW_DUNDERS = ["__mul__", "__rmul__"]
FLOAT_DIVISION_OVERFLOW_DUNDERS = ["__truediv__", "__floordiv__"]
FLOAT_REFLECTED_DIVISION_OVERFLOW_DUNDERS = ["__rtruediv__", "__rfloordiv__"]
ADD_SUB_DUNDERS = ["__add__", "__radd__", "__sub__", "__rsub__"]
UNSUPPORTED_OPERAND_CASES = [
    pytest.param(object(), "object", id="object"),
    pytest.param(None, "NoneType", id="none"),
    pytest.param("not-a-number", "str", id="invalid-numeric-string"),
    pytest.param(10**100, "int", id="decimal-overflowing-int"),
]
FRACTIONAL_DECIMAL_CASES = [
    pytest.param("__add__", "1.250", "2.375", "3.625", id="add"),
    pytest.param("__radd__", "1.250", "2.375", "3.625", id="radd"),
    pytest.param("__sub__", "2.375", "1.250", "1.125", id="sub"),
    pytest.param("__rsub__", "1.250", "2.375", "1.125", id="rsub"),
    pytest.param("__mul__", "1.250", "2.4", "3.0000", id="mul"),
    pytest.param("__rmul__", "1.250", "2.4", "3.0000", id="rmul"),
    pytest.param("__truediv__", "3.750", "1.5", "2.50", id="truediv"),
    pytest.param("__rtruediv__", "1.500", "3.75", "2.5", id="rtruediv"),
    pytest.param("__floordiv__", "3.750", "-1.5", "-3", id="floordiv"),
    pytest.param("__rfloordiv__", "1.500", "-3.75", "-3", id="rfloordiv"),
    pytest.param("__mod__", "3.750", "1.4", "0.950", id="mod"),
    pytest.param("__rmod__", "1.400", "3.75", "0.95", id="rmod"),
]
FLOAT_INFINITY_CASES = [
    pytest.param("__add__", math.inf, id="add"),
    pytest.param("__radd__", math.inf, id="radd"),
    pytest.param("__sub__", -math.inf, id="sub"),
    pytest.param("__rsub__", math.inf, id="rsub"),
    pytest.param("__mul__", math.inf, id="mul"),
    pytest.param("__rmul__", math.inf, id="rmul"),
    pytest.param("__truediv__", 0.0, id="truediv"),
    pytest.param("__rtruediv__", math.inf, id="rtruediv"),
    pytest.param("__floordiv__", 0.0, id="floordiv"),
    pytest.param("__rfloordiv__", math.inf, id="rfloordiv"),
    pytest.param("__mod__", 2.0, id="mod"),
    pytest.param("__rmod__", math.nan, id="rmod"),
]


def make_value(
    type_: type,
    value: object,
    *,
    precision: object = 0,
    currency: object = USD,
) -> object:
    """
    Make value.
    """
    if type_ is Money:
        return Money(value, currency)
    return type_(value, precision)


def make_raw_value(
    type_: type,
    raw: object,
    *,
    precision: object = FIXED_PRECISION,
    currency: object = TST,
) -> object:
    """
    Make raw value.
    """
    if type_ is Money:
        return Money.from_raw(raw, currency)
    return type_.from_raw(raw, precision)


def make_operand(type_: type, kind: object, value: object) -> object:
    """
    Make operand.
    """
    if kind == "same_type":
        return make_value(type_, value)
    if kind == "decimal":
        return Decimal(value)
    if kind == "float":
        return float(value)
    if kind == "int":
        return int(value)
    return str(value)


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("operand_kind", OPERAND_CASES)
@pytest.mark.parametrize(
    ("dunder", "receiver_value", "operand_value", "expected_value"),
    ARITHMETIC_CASES,
)
def test_arithmetic_dunder_supported_branches(
    type_: type,
    operand_kind: object,
    dunder: str,
    receiver_value: object,
    operand_value: object,
    expected_value: object,
) -> None:
    """
    Test arithmetic dunder supported branches.
    """
    receiver = make_value(type_, receiver_value)
    operand = make_operand(type_, operand_kind, operand_value)

    result = getattr(receiver, dunder)(operand)

    if operand_kind == "float":
        expected = float(expected_value)
    elif operand_kind == "same_type" and dunder in {
        "__add__",
        "__radd__",
        "__sub__",
        "__rsub__",
    }:
        expected = make_value(type_, expected_value)
    else:
        expected = Decimal(expected_value)
    assert type(result) is type(expected)
    assert result == expected


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize(
    ("dunder", "receiver_value", "operand_value", "expected_value"),
    FRACTIONAL_DECIMAL_CASES,
)
def test_arithmetic_dunder_fractional_decimal_branch(
    type_: type,
    dunder: str,
    receiver_value: object,
    operand_value: object,
    expected_value: object,
) -> None:
    """
    Test arithmetic dunder fractional decimal branch.
    """
    receiver = make_value(
        type_,
        Decimal(receiver_value),
        precision=3,
        currency=FRACTIONAL,
    )

    result = getattr(receiver, dunder)(Decimal(operand_value))

    assert type(result) is Decimal
    assert result == Decimal(expected_value)


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize(
    ("dunder", "expected"),
    [
        pytest.param("__add__", 3, id="add"),
        pytest.param("__radd__", 3, id="radd"),
        pytest.param("__sub__", 1, id="sub"),
        pytest.param("__rsub__", -1, id="rsub"),
        pytest.param("__mul__", 2, id="mul"),
        pytest.param("__rmul__", 2, id="rmul"),
        pytest.param("__truediv__", 2, id="truediv"),
        pytest.param("__rtruediv__", Decimal("0.5"), id="rtruediv"),
        pytest.param("__floordiv__", 2, id="floordiv"),
        pytest.param("__rfloordiv__", 0, id="rfloordiv"),
        pytest.param("__mod__", 0, id="mod"),
        pytest.param("__rmod__", 1, id="rmod"),
    ],
)
def test_arithmetic_dunder_bool_uses_decimal_branch(
    type_: type,
    dunder: str,
    expected: object,
) -> None:
    """
    Test arithmetic dunder bool uses decimal branch.
    """
    result = getattr(make_value(type_, 2), dunder)(True)

    assert type(result) is Decimal
    assert result == Decimal(expected)


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", ["__add__", "__radd__"])
def test_arithmetic_dunder_large_int_uses_decimal_string_fallback(type_: type, dunder: str) -> None:
    """
    Test arithmetic dunder large int uses decimal string fallback.
    """
    result = getattr(make_value(type_, 1), dunder)(10**20)

    assert type(result) is Decimal
    assert result == Decimal(10**20 + 1)


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("operand_kind", OPERAND_CASES)
@pytest.mark.parametrize("dunder", ZERO_DIVISOR_DUNDERS)
def test_arithmetic_dunder_zero_divisor_raises(
    type_: type,
    operand_kind: object,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder zero divisor raises.
    """
    reflected = dunder.startswith("__r")
    receiver = make_value(type_, 0 if reflected else 1)
    operand = make_operand(type_, operand_kind, 1 if reflected else 0)

    with pytest.raises(ZeroDivisionError) as exc_info:
        getattr(receiver, dunder)(operand)

    assert str(exc_info.value) == "Division or modulo by zero"


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", ZERO_DIVISOR_DUNDERS)
def test_arithmetic_dunder_bool_zero_divisor_raises(type_: type, dunder: str) -> None:
    """
    Test arithmetic dunder bool zero divisor raises.
    """
    reflected = dunder.startswith("__r")
    receiver = make_value(type_, 0 if reflected else 1)
    operand = bool(reflected)

    with pytest.raises(ZeroDivisionError) as exc_info:
        getattr(receiver, dunder)(operand)

    assert str(exc_info.value) == "Division or modulo by zero"


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("zero", [-0.0, Decimal("-0"), "-0"])
@pytest.mark.parametrize("dunder", DIRECT_ZERO_DIVISOR_DUNDERS)
def test_arithmetic_dunder_signed_zero_divisor_raises(
    type_: type,
    zero: object,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder signed zero divisor raises.
    """
    with pytest.raises(ZeroDivisionError) as exc_info:
        getattr(make_value(type_, 1), dunder)(zero)

    assert str(exc_info.value) == "Division or modulo by zero"


@pytest.mark.parametrize(("operand", "expected_type"), UNSUPPORTED_OPERAND_CASES)
@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", ARITHMETIC_DUNDERS)
def test_arithmetic_dunder_incompatible_operand_raises(
    type_: type,
    operand: object,
    expected_type: type,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder incompatible operand raises.
    """
    with pytest.raises(TypeError) as exc_info:
        getattr(make_value(type_, 1), dunder)(operand)

    assert str(exc_info.value) == f"Unsupported type for {dunder}, was `{expected_type}`"


@pytest.mark.parametrize(
    ("type_", "operand_type"),
    [
        pytest.param(Price, Quantity, id="price-quantity"),
        pytest.param(Price, Money, id="price-money"),
        pytest.param(Quantity, Price, id="quantity-price"),
        pytest.param(Quantity, Money, id="quantity-money"),
        pytest.param(Money, Price, id="money-price"),
        pytest.param(Money, Quantity, id="money-quantity"),
    ],
)
@pytest.mark.parametrize("dunder", ARITHMETIC_DUNDERS)
def test_arithmetic_dunder_other_fixed_type_raises(
    type_: type,
    operand_type: object,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder other fixed type raises.
    """
    operand = make_value(operand_type, 1)

    with pytest.raises(TypeError) as exc_info:
        getattr(make_value(type_, 1), dunder)(operand)

    assert str(exc_info.value) == (f"Unsupported type for {dunder}, was `{operand_type.__name__}`")


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", ARITHMETIC_DUNDERS)
def test_arithmetic_dunder_float_nan_propagates(type_: type, dunder: str) -> None:
    """
    Test arithmetic dunder float nan propagates.
    """
    result = getattr(make_value(type_, 2), dunder)(math.nan)

    assert type(result) is float
    assert math.isnan(result)


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize(("dunder", "expected"), FLOAT_INFINITY_CASES)
def test_arithmetic_dunder_float_infinity_matches_native_operation(
    type_: type,
    dunder: str,
    expected: object,
) -> None:
    """
    Test arithmetic dunder float infinity matches native operation.
    """
    result = getattr(make_value(type_, 2), dunder)(math.inf)

    assert type(result) is float
    if math.isnan(expected):
        assert math.isnan(result)
    else:
        assert result == expected


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", FLOAT_OVERFLOW_DUNDERS)
def test_arithmetic_dunder_float_multiplication_overflow_returns_infinity(
    type_: type,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder float multiplication overflow returns infinity.
    """
    result = getattr(make_value(type_, 2), dunder)(sys.float_info.max)

    assert type(result) is float
    assert result == math.inf


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", FLOAT_DIVISION_OVERFLOW_DUNDERS)
def test_arithmetic_dunder_float_division_overflow_returns_infinity(
    type_: type,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder float division overflow returns infinity.
    """
    result = getattr(make_value(type_, 2), dunder)(FLOAT_MIN_SUBNORMAL)

    assert type(result) is float
    assert result == math.inf


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", FLOAT_REFLECTED_DIVISION_OVERFLOW_DUNDERS)
def test_arithmetic_dunder_reflected_float_division_overflow_returns_infinity(
    type_: type,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder reflected float division overflow returns infinity.
    """
    result = getattr(make_raw_value(type_, 1), dunder)(sys.float_info.max)

    assert type(result) is float
    assert result == math.inf


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", ARITHMETIC_DUNDERS)
def test_arithmetic_dunder_float_rejects_unsupported_precision(type_: type, dunder: str) -> None:
    """
    Test arithmetic dunder float rejects unsupported precision.
    """
    receiver = make_raw_value(type_, 1, precision=WEI_PRECISION, currency=WEI)

    with pytest.raises(ValueError, match="maximum float precision") as exc_info:
        getattr(receiver, dunder)(1.0)

    assert str(exc_info.value) == ("Fixed-point precision 18 exceeds maximum float precision 16")


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", ["__add__", "__radd__"])
def test_arithmetic_dunder_same_type_overflow_raises(type_: type, dunder: str) -> None:
    """
    Test arithmetic dunder same type overflow raises.
    """
    maximum = 17_014_118_346_046.0 if HIGH_PRECISION else 9_223_372_036.0
    if type_ is Quantity:
        maximum *= 2
    value = make_value(type_, maximum)

    with pytest.raises(OverflowError) as exc_info:
        getattr(value, dunder)(value)

    assert str(exc_info.value) == "Fixed-point arithmetic overflow"


@pytest.mark.parametrize("type_", [pytest.param(Price), pytest.param(Money)])
@pytest.mark.parametrize("dunder", ["__add__", "__radd__"])
def test_arithmetic_dunder_same_type_negative_addition_overflow_raises(
    type_: type,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder same type negative addition overflow raises.
    """
    minimum = -17_014_118_346_046.0 if HIGH_PRECISION else -9_223_372_036.0
    value = make_value(type_, minimum)

    with pytest.raises(OverflowError) as exc_info:
        getattr(value, dunder)(value)

    assert str(exc_info.value) == "Fixed-point arithmetic overflow"


@pytest.mark.parametrize("type_", [pytest.param(Price), pytest.param(Money)])
@pytest.mark.parametrize("dunder", ["__sub__", "__rsub__"])
def test_arithmetic_dunder_same_type_subtraction_overflow_raises(type_: type, dunder: str) -> None:
    """
    Test arithmetic dunder same type subtraction overflow raises.
    """
    minimum = -17_014_118_346_046.0 if HIGH_PRECISION else -9_223_372_036.0
    receiver = make_value(type_, minimum if dunder == "__sub__" else 1)
    operand = make_value(type_, 1 if dunder == "__sub__" else minimum)

    with pytest.raises(OverflowError) as exc_info:
        getattr(receiver, dunder)(operand)

    assert str(exc_info.value) == "Fixed-point arithmetic overflow"


@pytest.mark.parametrize("type_", [pytest.param(Price), pytest.param(Money)])
@pytest.mark.parametrize("dunder", ["__sub__", "__rsub__"])
def test_arithmetic_dunder_same_type_positive_subtraction_overflow_raises(
    type_: type,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder same type positive subtraction overflow raises.
    """
    maximum = 17_014_118_346_046.0 if HIGH_PRECISION else 9_223_372_036.0
    receiver = make_value(type_, maximum if dunder == "__sub__" else -1)
    operand = make_value(type_, -1 if dunder == "__sub__" else maximum)

    with pytest.raises(OverflowError) as exc_info:
        getattr(receiver, dunder)(operand)

    assert str(exc_info.value) == "Fixed-point arithmetic overflow"


@pytest.mark.parametrize(
    ("dunder", "receiver_value", "operand_value"),
    [
        pytest.param("__sub__", 1, 2, id="sub"),
        pytest.param("__rsub__", 2, 1, id="rsub"),
    ],
)
def test_quantity_same_type_negative_subtraction_raises(
    dunder: str,
    receiver_value: object,
    operand_value: object,
) -> None:
    """
    Test quantity same type negative subtraction raises.
    """
    receiver = Quantity(receiver_value, 0)
    operand = Quantity(operand_value, 0)

    with pytest.raises(ValueError, match="negative value") as exc_info:
        getattr(receiver, dunder)(operand)

    assert str(exc_info.value) == ("Quantity subtraction would result in negative value: 1 - 2")


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", ["__mul__", "__rmul__"])
def test_arithmetic_dunder_same_type_maximum_multiplication_succeeds(
    type_: type,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder same type maximum multiplication succeeds.
    """
    maximum = 17_014_118_346_046.0 if HIGH_PRECISION else 9_223_372_036.0
    if type_ is Quantity:
        maximum *= 2
    value = make_value(type_, maximum)

    result = getattr(value, dunder)(value)

    assert type(result) is Decimal
    assert result == value.as_decimal() * value.as_decimal()


@pytest.mark.skipif(not HIGH_PRECISION, reason="same-type division cannot overflow Decimal")
@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize(
    "dunder",
    ["__truediv__", "__rtruediv__", "__floordiv__", "__rfloordiv__"],
)
def test_arithmetic_dunder_same_type_division_overflow_raises(type_: type, dunder: str) -> None:
    """
    Test arithmetic dunder same type division overflow raises.
    """
    maximum = 17_014_118_346_046.0
    if type_ is Quantity:
        maximum *= 2
    maximum_value = make_value(type_, maximum, precision=FIXED_PRECISION, currency=TST)
    minimum_value = make_raw_value(type_, 1)
    receiver = minimum_value if dunder.startswith("__r") else maximum_value
    operand = maximum_value if dunder.startswith("__r") else minimum_value

    with pytest.raises(OverflowError) as exc_info:
        getattr(receiver, dunder)(operand)

    assert str(exc_info.value) == "Fixed-point arithmetic overflow"


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize(
    "dunder",
    ["__add__", "__radd__", "__sub__", "__rsub__", "__mul__", "__rmul__"],
)
def test_arithmetic_dunder_decimal_overflow_raises(type_: type, dunder: str) -> None:
    """
    Test arithmetic dunder decimal overflow raises.
    """
    operand = DECIMAL_MIN if dunder in {"__sub__", "__rsub__"} else DECIMAL_MAX

    with pytest.raises(OverflowError) as exc_info:
        getattr(make_value(type_, 2), dunder)(operand)

    assert str(exc_info.value) == "Fixed-point arithmetic overflow"


@pytest.mark.parametrize("type_", [pytest.param(Price), pytest.param(Money)])
@pytest.mark.parametrize("dunder", ["__add__", "__radd__"])
def test_arithmetic_dunder_negative_decimal_addition_overflow_raises(
    type_: type,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder negative decimal addition overflow raises.
    """
    with pytest.raises(OverflowError) as exc_info:
        getattr(make_value(type_, -2), dunder)(DECIMAL_MIN)

    assert str(exc_info.value) == "Fixed-point arithmetic overflow"


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", ["__mul__", "__rmul__"])
def test_arithmetic_dunder_negative_decimal_multiplication_overflow_raises(
    type_: type,
    dunder: str,
) -> None:
    """
    Test arithmetic dunder negative decimal multiplication overflow raises.
    """
    with pytest.raises(OverflowError) as exc_info:
        getattr(make_value(type_, 2), dunder)(DECIMAL_MIN)

    assert str(exc_info.value) == "Fixed-point arithmetic overflow"


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize(
    ("dunder", "operand", "expected"),
    [
        pytest.param("__mod__", DECIMAL_MAX, Decimal(2), id="mod-max"),
        pytest.param("__rmod__", DECIMAL_MAX, Decimal(1), id="rmod-max"),
        pytest.param("__mod__", DECIMAL_MIN, Decimal(2), id="mod-min"),
        pytest.param("__rmod__", DECIMAL_MIN, Decimal(-1), id="rmod-min"),
    ],
)
def test_arithmetic_dunder_decimal_remainder_boundaries_succeed(
    type_: type,
    dunder: str,
    operand: object,
    expected: object,
) -> None:
    """
    Test arithmetic dunder decimal remainder boundaries succeed.
    """
    result = getattr(make_value(type_, 2), dunder)(operand)

    assert type(result) is Decimal
    assert result == expected


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize(
    "dunder",
    ["__truediv__", "__rtruediv__", "__floordiv__", "__rfloordiv__"],
)
@pytest.mark.parametrize("negative", [False, True], ids=["positive", "negative"])
def test_arithmetic_dunder_decimal_division_overflow_raises(
    type_: type,
    dunder: str,
    negative: object,
) -> None:
    """
    Test arithmetic dunder decimal division overflow raises.
    """
    reflected = dunder.startswith("__r")
    if reflected:
        receiver = Money.from_raw(1, TST) if type_ is Money else type_.from_raw(1, FIXED_PRECISION)
        operand = DECIMAL_MIN if negative else DECIMAL_MAX
    else:
        receiver = make_value(type_, 8)
        operand = Decimal(
            "-0.0000000000000000000000000001" if negative else "0.0000000000000000000000000001",
        )

    with pytest.raises(OverflowError) as exc_info:
        getattr(receiver, dunder)(operand)

    assert str(exc_info.value) == "Fixed-point arithmetic overflow"


@pytest.mark.parametrize("dunder", ARITHMETIC_DUNDERS)
def test_money_arithmetic_dunder_currency_mismatch_raises(dunder: str) -> None:
    """
    Test money arithmetic dunder currency mismatch raises.
    """
    usd = Money(2, USD)
    aud = Money(3, Currency.from_str("AUD"))

    with pytest.raises(ValueError, match="Currency mismatch") as exc_info:
        getattr(usd, dunder)(aud)

    expected = (
        "Currency mismatch: AUD vs USD"
        if dunder.startswith("__r")
        else "Currency mismatch: USD vs AUD"
    )
    assert str(exc_info.value) == expected


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize("dunder", ADD_SUB_DUNDERS)
def test_addition_subtraction_dunder_mixed_scale_raises(type_: type, dunder: str) -> None:
    """
    Test addition subtraction dunder mixed scale raises.
    """
    lhs_currency = TST
    rhs_currency = Currency(
        code=TST.code,
        precision=FIXED_PRECISION + 1,
        iso4217=0,
        name=TST.name,
        currency_type=CurrencyType.CRYPTO,
    )
    lhs = make_raw_value(type_, 2, currency=lhs_currency)
    rhs = make_raw_value(
        type_,
        1,
        precision=FIXED_PRECISION + 1,
        currency=rhs_currency,
    )

    with pytest.raises(ValueError, match="Incompatible fixed-point scales") as exc_info:
        getattr(lhs, dunder)(rhs)

    assert str(exc_info.value) == "Incompatible fixed-point scales"


@pytest.mark.parametrize("type_", TYPE_CASES)
@pytest.mark.parametrize(
    ("dunder", "receiver_raw", "operand_raw"),
    [
        pytest.param("__add__", 2, 1, id="add"),
        pytest.param("__radd__", 2, 1, id="radd"),
        pytest.param("__sub__", 2, 1, id="sub"),
        pytest.param("__rsub__", 1, 2, id="rsub"),
    ],
)
def test_addition_subtraction_dunder_compatible_standard_scales_succeeds(
    type_: type,
    dunder: str,
    receiver_raw: object,
    operand_raw: object,
) -> None:
    """
    Test addition subtraction dunder compatible standard scales succeeds.
    """
    zero_precision_currency = Currency(
        code=TST.code,
        precision=0,
        iso4217=0,
        name=TST.name,
        currency_type=CurrencyType.CRYPTO,
    )
    receiver = make_raw_value(
        type_,
        receiver_raw,
        precision=0,
        currency=zero_precision_currency,
    )
    operand = make_raw_value(type_, operand_raw)

    result = getattr(receiver, dunder)(operand)

    expected_raw = 3 if "add" in dunder else 1
    assert type(result) is type_
    assert result.raw == expected_raw

    if type_ is Money:
        expected_currency = TST if dunder.startswith("__r") else zero_precision_currency
        assert result.currency.code == expected_currency.code
        assert result.currency.precision == expected_currency.precision
    else:
        assert result.precision == FIXED_PRECISION
