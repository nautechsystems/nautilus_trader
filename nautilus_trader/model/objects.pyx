# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
Defines domain value objects.

The `BaseDecimal` class is intended to be used as the base class for fundamental
domain model value types. The specification of precision is more straight
forward than providing a decimal.Context. Also this type is able to be used as
an operand for mathematical ops with `float` objects.

The fundamental value objects for the trading domain are defined here.

References
----------
https://docs.python.org/3.9/library/decimal.html

"""

import decimal

from cpython.object cimport PyObject_RichCompareBool
from cpython.object cimport Py_EQ
from cpython.object cimport Py_GE
from cpython.object cimport Py_GT
from cpython.object cimport Py_LE
from cpython.object cimport Py_LT
from cpython.object cimport Py_NE

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.currency cimport Currency


cdef str ROUND_HALF_EVEN = decimal.ROUND_HALF_EVEN


cdef class BaseDecimal:
    """
    The abstract base class for all domain objects.

    Represents a decimal number with a specified precision.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        value=0,
        precision=None,
        str rounding not None=decimal.ROUND_HALF_EVEN,
    ):
        """
        Initialize a new instance of the `BaseDecimal` class.

        Parameters
        ----------
        value : integer, float, string, Decimal or BaseDecimal
            The value of the decimal. If value is a float, then a precision must
            be specified.
        precision : int, optional
            The precision for the decimal. If a precision is specified then the
            value will be rounded to the precision. Else the precision will be
            inferred from the given value.
        rounding : str, optional
            The rounding mode to apply to the decimal. Must be a constant from
            the decimal module. Only applicable if precision is specified.

        Raises
        ------
        TypeError
            If value is a float and precision is not specified.
        ValueError
            If precision is negative (< 0).
        TypeError
            If rounding is invalid.

        """
        Condition.not_none(value, "value")

        if precision is None:  # Infer precision
            if isinstance(value, float):
                raise TypeError("precision cannot be inferred from a float, "
                                "please specify a precision when passing a float")
            elif isinstance(value, BaseDecimal):
                self._value = value.as_decimal()
            else:
                self._value = decimal.Decimal(value)
        else:
            Condition.not_negative_int(precision, "precision")

            if rounding == ROUND_HALF_EVEN:
                if not isinstance(value, float):
                    value = float(value)
                self._value = self._make_decimal(value, precision)
            else:
                self._value = self._make_decimal_with_rounding(value, precision, rounding)

    cdef inline object _make_decimal_with_rounding(self, value, int precision, str rounding):
        exponent = decimal.Decimal(f"{1.0 / 10 ** precision:.{precision}f}")
        return decimal.Decimal(value).quantize(exp=exponent, rounding=rounding)

    cdef inline object _make_decimal(self, double value, int precision):
        return decimal.Decimal(f'{value:.{precision}f}')

    def __eq__(self, other) -> bool:
        return BaseDecimal._compare(self, other, Py_EQ)

    def __ne__(self, other) -> bool:
        return BaseDecimal._compare(self, other, Py_NE)

    def __lt__(self, other) -> bool:
        return BaseDecimal._compare(self, other, Py_LT)

    def __le__(self, other) -> bool:
        return BaseDecimal._compare(self, other, Py_LE)

    def __gt__(self, other) -> bool:
        return BaseDecimal._compare(self, other, Py_GT)

    def __ge__(self, other) -> bool:
        return BaseDecimal._compare(self, other, Py_GE)

    def __add__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return float(self) + other
        else:
            return BaseDecimal._extract_value(self) + BaseDecimal._extract_value(other)

    def __radd__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return other + float(self)
        else:
            return BaseDecimal._extract_value(other) + BaseDecimal._extract_value(self)

    def __sub__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return float(self) - other
        else:
            return BaseDecimal._extract_value(self) - BaseDecimal._extract_value(other)

    def __rsub__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return other - float(self)
        else:
            return BaseDecimal._extract_value(other) - BaseDecimal._extract_value(self)

    def __mul__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return float(self) * other
        else:
            return BaseDecimal._extract_value(self) * BaseDecimal._extract_value(other)

    def __rmul__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return other * float(self)
        else:
            return BaseDecimal._extract_value(other) * BaseDecimal._extract_value(self)

    def __truediv__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return float(self) / other
        else:
            return BaseDecimal._extract_value(self) / BaseDecimal._extract_value(other)

    def __rtruediv__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return other / float(self)
        else:
            return BaseDecimal._extract_value(other) / BaseDecimal._extract_value(self)

    def __floordiv__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return float(self) // other
        else:
            return BaseDecimal._extract_value(self) // BaseDecimal._extract_value(other)

    def __rfloordiv__(self, other) -> decimal.Decimal or float:
        if isinstance(other, float):
            return other // float(self)
        else:
            return BaseDecimal._extract_value(other) // BaseDecimal._extract_value(self)

    def __mod__(self, other) -> decimal.Decimal:
        if isinstance(other, float):
            return float(self) % other
        else:
            return BaseDecimal._extract_value(self) % BaseDecimal._extract_value(other)

    def __rmod__(self, other) -> decimal.Decimal:
        if isinstance(other, float):
            return other % float(self)
        else:
            return BaseDecimal._extract_value(other) % BaseDecimal._extract_value(self)

    def __neg__(self) -> decimal.Decimal:
        return self._value.__neg__()

    def __pos__(self) -> decimal.Decimal:
        return self._value.__pos__()

    def __abs__(self) -> decimal.Decimal:
        return abs(self._value)

    def __round__(self, ndigits=None) -> decimal.Decimal:
        return round(self._value, ndigits)

    def __float__(self) -> float:
        return float(self._value)

    def __int__(self) -> int:
        return int(self._value)

    def __hash__(self) -> int:
        return hash(self._value)

    def __str__(self) -> str:
        return str(self._value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self}')"

    @staticmethod
    cdef inline object _extract_value(object obj):
        if isinstance(obj, BaseDecimal):
            return obj.as_decimal()
        return obj

    @staticmethod
    cdef inline bint _compare(a, b, int op) except *:
        if isinstance(a, BaseDecimal):
            a = <BaseDecimal>a.as_decimal()
        if isinstance(b, BaseDecimal):
            b = <BaseDecimal>b.as_decimal()

        return PyObject_RichCompareBool(a, b, op)

    @property
    def precision(self):
        """
        The precision of the value.

        Returns
        -------
        int

        """
        return self.precision_c()

    cdef inline int precision_c(self) except *:
        return abs(self._value.as_tuple().exponent)

    cpdef object as_decimal(self):
        """
        Return the value as a built-in `Decimal`.

        Returns
        -------
        Decimal

        """
        return self._value

    cpdef double as_double(self) except *:
        """
        Return the value as a `double`.

        Returns
        -------
        double

        """
        return float(self._value)


cdef class Quantity(BaseDecimal):
    """
    Represents a quantity with a non-negative value.

    Capable of storing either a whole number (no decimal places) of “shares”
    (securities denominated in whole units) or a decimal value containing
    decimal places for non-share quantity asset classes (securities denominated
    in fractional units).

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/index.html#Qty

    """

    def __init__(
        self,
        value=0,
        precision=None,
        str rounding not None=decimal.ROUND_HALF_EVEN,
    ):
        """
        Initialize a new instance of the `Quantity` class.

        Parameters
        ----------
        value : integer, float, string, Decimal or BaseDecimal
            The value of the quantity. If value is a float, then a precision must
            be specified.
        precision : int, optional
            The precision for the quantity. If a precision is specified then the
            value will be rounded to the precision. Else the precision will be
            inferred from the given value.
        rounding : str, optional
            The rounding mode to apply. Must be a constant from the decimal
            module. Only applicable if precision is specified.

        Raises
        ------
        TypeError
            If value is a float and precision is not specified.
        ValueError
            If value is negative (< 0).
        ValueError
            If precision is negative (< 0).
        TypeError
            If rounding is invalid.

        """
        super().__init__(value, precision, rounding)

        # Post-condition
        Condition.true(self._value >= 0, f"quantity not negative, was {self._value}")

    cpdef str to_str(self):
        """
        Return the formatted string representation of the quantity.

        Returns
        -------
        str

        """
        return f"{self._value:,}"


cdef class Price(BaseDecimal):
    """
    Represents a price in a financial market.

    The number of decimal places may vary. For certain asset classes prices may
    be negative values. For example, prices for options strategies can be
    negative under certain market conditions.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/index.html#Qty

    """

    def __init__(
        self,
        value=0,
        precision=None,
        str rounding not None=decimal.ROUND_HALF_EVEN,
    ):
        """
        Initialize a new instance of the `Price` class.

        Parameters
        ----------
        value : integer, float, string, Decimal or BaseDecimal
            The value of the price. If value is a float, then a precision must
            be specified.
        precision : int, optional
            The precision for the price. If a precision is specified then the
            value will be rounded to the precision. Else the precision will be
            inferred from the given value.
        rounding : str, optional
            The rounding mode to apply. Must be a constant from the decimal
            module. Only applicable if precision is specified.

        Raises
        ------
        ValueError
            If precision is negative (< 0).
        TypeError
            If rounding is invalid.

        """
        super().__init__(value, precision, rounding)


cdef class Money(BaseDecimal):
    """
    Represents an amount of money including currency type.
    """

    def __init__(
        self,
        value,
        Currency currency not None,
        str rounding not None=decimal.ROUND_HALF_EVEN,
    ):
        """
        Initialize a new instance of the `Money` class.

        Parameters
        ----------
        value : integer, float, string, Decimal or BaseDecimal
            The value of the money.
        currency : Currency
            The currency of the money.
        rounding : str, optional
            The rounding mode to apply. Must be a constant from the decimal
            module.

        Raises
        ------
        TypeError
            If rounding is invalid.

        """
        if value is None:
            value = 0
        super().__init__(value, currency.precision, rounding)

        self.currency = currency

    def __eq__(self, Money other) -> bool:
        return self.currency == other.currency and self._value == other.as_decimal()

    def __ne__(self, Money other) -> bool:
        return not self == other

    def __lt__(self, Money other) -> bool:
        return self.currency == other.currency and self._value < other.as_decimal()

    def __le__(self, Money other) -> bool:
        return self.currency == other.currency and self._value <= other.as_decimal()

    def __gt__(self, Money other) -> bool:
        return self.currency == other.currency and self._value > other.as_decimal()

    def __ge__(self, Money other) -> bool:
        return self.currency == other.currency and self._value >= other.as_decimal()

    def __hash__(self) -> int:
        return hash((self.currency, self._value))

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self._value}', {self.currency})"

    cpdef str to_str(self):
        """
        Return the formatted string representation of the money.

        Returns
        -------
        str

        """
        return f"{self._value:,} {self.currency}"
