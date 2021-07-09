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
Defines fundamental value objects for the trading domain.

The `BaseDecimal` class is intended to be used as the base class for fundamental
domain model value types. The specification of precision is more explicit and
straight forward than providing a decimal.Context. The `BaseDecimal` type and its
subclasses are also able to be used as operands for mathematical operations with
`float` objects. Return values are floats if one of the operands is a float, else
a decimal.Decimal.


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
from libc.stdint cimport uint8_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport precision_from_str
from nautilus_trader.model.currency cimport Currency


cdef class BaseDecimal:
    """
    The abstract base class for all domain value objects.

    Represents a decimal number with a specified precision.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, value, uint8_t precision):
        """
        Initialize a new instance of the ``BaseDecimal`` class.

        Use a precision of 0 for whole numbers (no fractional units).

        Parameters
        ----------
        value : integer, float, string or Decimal
            The value of the decimal.
        precision : uint8
            The precision for the decimal.

        Raises
        ------
        OverflowError
            If precision is negative (< 0).

        """
        if isinstance(value, decimal.Decimal):
            self._value = round(value, precision)
        else:
            self._value = decimal.Decimal(f'{float(value):.{precision}f}')

        self.precision = precision

    def __eq__(self, other) -> bool:
        return BaseDecimal._compare(self, other, Py_EQ)

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
    cdef object _extract_value(object obj):
        if isinstance(obj, BaseDecimal):
            return obj.as_decimal()
        return obj

    @staticmethod
    cdef bint _compare(a, b, int op) except *:
        if isinstance(a, BaseDecimal):
            a = <BaseDecimal>a.as_decimal()
        if isinstance(b, BaseDecimal):
            b = <BaseDecimal>b.as_decimal()

        return PyObject_RichCompareBool(a, b, op)

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
    https://www.onixs.biz/fix-dictionary/5.0.SP2/index.html#Qty
    """

    def __init__(self, value, uint8_t precision):
        """
        Initialize a new instance of the ``Quantity`` class.

        Use a precision of 0 for whole numbers (no fractional units).

        Parameters
        ----------
        value : integer, float, string, Decimal
            The value of the quantity.
        precision : uint8
            The precision for the quantity.

        Raises
        ------
        ValueError
            If value is negative (< 0).
        OverflowError
            If precision is negative (< 0).

        """
        super().__init__(value, precision)

        # Post-condition
        Condition.true(self._value >= 0, f"quantity negative, was {self._value}")

    @staticmethod
    cdef Quantity zero_c(uint8_t precision):
        return Quantity(0, precision)

    @staticmethod
    cdef Quantity from_str_c(str value):
        return Quantity(value, precision=precision_from_str(value))

    @staticmethod
    cdef Quantity from_int_c(int value):
        return Quantity(value, precision=0)

    @staticmethod
    def zero(uint8_t precision=0) -> Quantity:
        """
        Return a quantity with a value of zero.

        precision : uint8, optional
            The precision for the quantity.

        Returns
        -------
        Quantity

        Raises
        ------
        OverflowError
            If precision is negative (< 0).

        Warnings
        --------
        The default precision is zero.

        """
        return Quantity.zero_c(precision)

    @staticmethod
    def from_str(str value) -> Quantity:
        """
        Return a quantity parsed from the given string.

        Parameters
        ----------
        value : str
            The value to parse.

        Returns
        -------
        Quantity

        Warning
        -------
        The decimal precision will be inferred from the number of digits
        following the '.' point (if no point then precision zero).

        """
        Condition.not_none(value, "value")

        return Quantity.from_str_c(value)

    @staticmethod
    def from_int(int value) -> Quantity:
        """
        Return a quantity from the given integer value.

        A precision of zero will be inferred.

        Parameters
        ----------
        value : int
            The value for the quantity.

        Returns
        -------
        Quantity

        """
        Condition.not_none(value, "value")

        return Quantity.from_int_c(value)

    cpdef str to_str(self):
        """
        Return the formatted string representation of the quantity.

        Returns
        -------
        str

        """
        return f"{self.as_decimal():,}".replace(",", "_")


cdef class Price(BaseDecimal):
    """
    Represents a price in a financial market.

    The number of decimal places may vary. For certain asset classes prices may
    be negative values. For example, prices for options strategies can be
    negative under certain market conditions.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/index.html#Price
    """

    def __init__(self, value, uint8_t precision):
        """
        Initialize a new instance of the ``Price`` class.

        Use a precision of 0 for whole numbers (no fractional units).

        Parameters
        ----------
        value : integer, float, string or Decimal
            The value of the price.
        precision : uint8
            The precision for the price.

        Raises
        ------
        OverflowError
            If precision is negative (< 0).

        """
        super().__init__(value, precision)

    @staticmethod
    cdef Price from_str_c(str value):
        return Price(value, precision=precision_from_str(value))

    @staticmethod
    cdef Price from_int_c(int value):
        return Price(value, precision=0)

    @staticmethod
    def from_str(str value) -> Price:
        """
        Return a price parsed from the given string.

        Parameters
        ----------
        value : str
            The value to parse.

        Returns
        -------
        Price

        """
        Condition.not_none(value, "value")

        return Price.from_str_c(value)

    @staticmethod
    def from_int(int value) -> Price:
        """
        Return a price from the given integer value.

        A precision of zero will be inferred.

        Parameters
        ----------
        value : int
            The value for the price.

        Returns
        -------
        Price

        """
        Condition.not_none(value, "value")

        return Price.from_int_c(value)


cdef class Money(BaseDecimal):
    """
    Represents an amount of money including currency type.
    """

    def __init__(self, value, Currency currency not None):
        """
        Initialize a new instance of the ``Money`` class.

        Parameters
        ----------
        value : integer, float, string or Decimal
            The amount of money in the currency denomination.
        currency : Currency
            The currency of the money.

        """
        if value is None:
            value = 0
        super().__init__(value, currency.precision)

        self.currency = currency

    def __eq__(self, Money other) -> bool:
        return self.currency == other.currency and self._value == other.as_decimal()

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

    @staticmethod
    cdef Money from_str_c(str value):
        cdef list pieces = value.split(' ', maxsplit=1)

        if len(pieces) != 2:
            raise ValueError(f"The `Money` string value was malformed, was {value}")

        return Money(pieces[0], Currency.from_str_c(pieces[1]))

    @staticmethod
    def from_str(str value) -> Money:
        """
        Return money parsed from the given string.

        Must be correctly formatted with a value and currency separated by a
        whitespace delimiter.

        Example: "1000000.00 USD".

        Parameters
        ----------
        value : str
            The value to parse.

        Returns
        -------
        Money

        Raises
        ------
        ValueError
            If the value is malformed.

        """
        Condition.not_none(value, "value")

        cdef tuple pieces = value.partition(' ')

        if len(pieces) != 3:
            raise ValueError(f"The `Money` string value was malformed, was {value}")

        return Money.from_str_c(value)

    cpdef str to_str(self):
        """
        Return the formatted string representation of the money.

        Returns
        -------
        str

        """
        return f"{self._value:,} {self.currency}".replace(",", "_")


cdef class AccountBalance:
    """
    Represents an account balance in a particular currency.
    """

    def __init__(
        self,
        Currency currency not None,
        Money total not None,
        Money locked not None,
        Money free not None,
    ):
        """
        Initialize a new instance of the ``AccountBalance`` class.

        Parameters
        ----------
        total : Money
            The total account balance.
        locked : Money
            The account balance locked (assigned to pending orders).
        free : Money
            The account balance free for trading.

        Raises
        ------
        ValueError
            If any money.currency does not equal currency.
        ValueError
            If total - locked != free.

        """
        Condition.equal(currency, total.currency, "currency", "total.currency")
        Condition.equal(currency, locked.currency, "currency", "locked.currency")
        Condition.equal(currency, free.currency, "currency", "free.currency")
        Condition.true(total - locked == free.as_decimal(), "total - locked != free")

        self.currency = currency
        self.total = total
        self.locked = locked
        self.free = free

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"total={self.total.to_str()}, "
            f"locked={self.locked.to_str()}, "
            f"free={self.free.to_str()})"
        )

    @staticmethod
    cdef AccountBalance from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef Currency currency = Currency.from_str_c(values["currency"])
        return AccountBalance(
            currency=currency,
            total=Money(values["total"], currency),
            locked=Money(values["locked"], currency),
            free=Money(values["free"], currency),
        )

    @staticmethod
    def from_dict(dict values):
        """
        Return an account balance from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        AccountBalance

        """
        return AccountBalance.from_dict_c(values)

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "currency": self.currency.code,
            "total": str(self.total.as_decimal()),
            "locked": str(self.locked.as_decimal()),
            "free": str(self.free.as_decimal()),
        }
