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

"""Defines fundamental value objects for the trading domain."""

import decimal

import cython

from cpython.object cimport Py_EQ
from cpython.object cimport Py_GE
from cpython.object cimport Py_GT
from cpython.object cimport Py_LE
from cpython.object cimport Py_LT
from cpython.object cimport PyObject_RichCompareBool
from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport price_as_f64
from nautilus_trader.core.rust.model cimport price_free
from nautilus_trader.core.rust.model cimport price_new
from nautilus_trader.core.rust.model cimport quantity_as_f64
from nautilus_trader.core.rust.model cimport quantity_free
from nautilus_trader.core.rust.model cimport quantity_new
from nautilus_trader.core.text cimport precision_from_str
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class BaseDecimal:
    """
    The abstract base class for all domain value objects.

    Represents a decimal number with a specified precision and is intended to be
    used as the base class for fundamental domain model value types. The
    specification of precision is more explicit and straight forward than
    providing a decimal.Context. The `BaseDecimal` type and its subclasses are
    also able to be used as operands for mathematical operations with `float`
    objects. Return values are floats if one of the operands is a float, else
    a `decimal.Decimal`.

    Parameters
    ----------
    value : integer, float, string or Decimal
        The value of the decimal.
    precision : uint8
        The precision for the decimal. Use a precision of 0 for whole numbers
        (no fractional units).

    Raises
    ------
    OverflowError
        If `precision` is negative (< 0).

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.

    References
    ----------
    https://docs.python.org/3.9/library/decimal.html
    """

    def __init__(self, value, uint8_t precision):
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


@cython.auto_pickle(True)
cdef class Quantity:
    """
    Represents a quantity with a non-negative value.

    Capable of storing either a whole number (no decimal places) of “shares”
    (securities denominated in whole units) or a decimal value containing
    decimal places for non-share quantity asset classes (securities denominated
    in fractional units).

    Parameters
    ----------
    value : integer, float, string, Decimal
        The value of the quantity.
    precision : uint8
        The precision for the quantity. Use a precision of 0 for whole numbers
        (no fractional units).

    Raises
    ------
    ValueError
        If `value` is negative (< 0).
    OverflowError
        If `precision` is negative (< 0).

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/index.html#Qty
    """

    def __init__(self, double value, uint8_t precision):
        Condition.true(value >= 0.0, f"quantity negative, was {value}")

        self._qty = quantity_new(value, precision)

    def __eq__(self, other) -> bool:
        return Quantity._compare(self, other, Py_EQ)

    def __lt__(self, other) -> bool:
        return Quantity._compare(self, other, Py_LT)

    def __le__(self, other) -> bool:
        return Quantity._compare(self, other, Py_LE)

    def __gt__(self, other) -> bool:
        return Quantity._compare(self, other, Py_GT)

    def __ge__(self, other) -> bool:
        return Quantity._compare(self, other, Py_GE)

    def __add__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) + float(b)
        return Quantity._extract_decimal(a) + Quantity._extract_decimal(b)

    def __radd__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) + float(b)
        return Quantity._extract_decimal(a) + Quantity._extract_decimal(b)

    def __sub__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) - float(b)
        return Quantity._extract_decimal(a) - Quantity._extract_decimal(b)

    def __rsub__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) - float(b)
        return Quantity._extract_decimal(a) - Quantity._extract_decimal(b)

    def __mul__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) * float(b)
        return Quantity._extract_decimal(a) * Quantity._extract_decimal(b)

    def __rmul__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) * float(b)
        return Quantity._extract_decimal(a) * Quantity._extract_decimal(b)

    def __truediv__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) / float(b)
        return Quantity._extract_decimal(a) / Quantity._extract_decimal(b)

    def __rtruediv__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) / float(b)
        return Quantity._extract_decimal(a) / Quantity._extract_decimal(b)

    def __floordiv__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) // float(b)
        return Quantity._extract_decimal(a) // Quantity._extract_decimal(b)

    def __rfloordiv__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) // float(b)
        return Quantity._extract_decimal(a) // Quantity._extract_decimal(b)

    def __mod__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) % float(b)
        return Quantity._extract_decimal(a) % Quantity._extract_decimal(b)

    def __rmod__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) % float(b)
        return Quantity._extract_decimal(a) * Quantity._extract_decimal(b)

    def __neg__(self) -> decimal.Decimal:
        return self.as_decimal().__neg__()

    def __pos__(self) -> decimal.Decimal:
        return self.as_decimal().__pos__()

    def __abs__(self) -> decimal.Decimal:
        return abs(self.as_decimal())

    def __round__(self, ndigits=None) -> decimal.Decimal:
        return round(self.as_decimal(), ndigits)

    def __float__(self) -> float:
        return quantity_as_f64(&self._qty)

    def __int__(self) -> int:
        return int(quantity_as_f64(&self._qty))

    def __hash__(self) -> int:
        return hash(self._qty.value)

    def __str__(self) -> str:
        return f"{quantity_as_f64(&self._qty):.{self._qty.precision}f}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self}')"

    def __del__(self) -> None:
        quantity_free(self._qty)  # `self._qty` moved to rust (then dropped)

    cdef int64_t raw_int64(self):
        return self._qty.value

    @staticmethod
    cdef object _extract_decimal(object obj):
        if hasattr(obj, "as_decimal"):
            return obj.as_decimal()
        return obj

    @staticmethod
    cdef bint _compare(a, b, int op) except *:
        if isinstance(a, Quantity):
            a = <Quantity>a.as_decimal()
        elif isinstance(a, Price):
            a = <Price>a.as_decimal()

        if isinstance(b, Quantity):
            b = <Quantity>b.as_decimal()
        elif isinstance(b, Price):
            b = <Price>b.as_decimal()

        return PyObject_RichCompareBool(a, b, op)

    @property
    def precision(self) -> int:
        return self._qty.precision

    @staticmethod
    cdef Quantity zero_c(uint8_t precision):
        return Quantity(0, precision)

    @staticmethod
    cdef Quantity from_str_c(str value):
        return Quantity(float(value), precision=precision_from_str(value))

    @staticmethod
    cdef Quantity from_int_c(int value):
        return Quantity(value, precision=0)

    @staticmethod
    def zero(uint8_t precision=0) -> Quantity:
        """
        Return a quantity with a value of zero.

        precision : uint8, default 0
            The precision for the quantity.

        Returns
        -------
        Quantity

        Raises
        ------
        OverflowError
            If `precision` is negative (< 0).

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

        Warnings
        --------
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
        return f"{quantity_as_f64(&self._qty):,.{self._qty.precision}f}".replace(",", "_")

    cpdef void add_assign(self, Quantity other) except *:
        assert other._qty.precision <= self._qty.precision
        self._qty.value += other.raw_int64()

    cpdef void sub_assign(self, Quantity other) except *:
        assert other._qty.precision <= self._qty.precision
        self._qty.value -= other.raw_int64()

    cpdef object as_decimal(self):
        """
        Return the value as a built-in `Decimal`.

        Returns
        -------
        Decimal

        """
        return decimal.Decimal(f"{quantity_as_f64(&self._qty):.{self._qty.precision}f}")

    cpdef double as_double(self) except *:
        """
        Return the value as a `double`.

        Returns
        -------
        double

        """
        return quantity_as_f64(&self._qty)


@cython.auto_pickle(True)
cdef class Price:
    """
    Represents a price in a financial market.

    The number of decimal places may vary. For certain asset classes, prices may
    have negative values. For example, prices for options instruments can be
    negative under certain conditions.

    Parameters
    ----------
    value : integer, float, string or Decimal
        The value of the price.
    precision : uint8
        The precision for the price. Use a precision of 0 for whole numbers
        (no fractional units).

    Raises
    ------
    OverflowError
        If `precision` is negative (< 0).

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/index.html#Price
    """

    def __init__(self, double value, uint8_t precision):
        self._price = price_new(value, precision)

    def __eq__(self, other) -> bool:
        return Price._compare(self, other, Py_EQ)

    def __lt__(self, other) -> bool:
        return Price._compare(self, other, Py_LT)

    def __le__(self, other) -> bool:
        return Price._compare(self, other, Py_LE)

    def __gt__(self, other) -> bool:
        return Price._compare(self, other, Py_GT)

    def __ge__(self, other) -> bool:
        return Price._compare(self, other, Py_GE)

    def __add__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) + float(b)
        return Price._extract_decimal(a) + Price._extract_decimal(b)

    def __radd__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) + float(b)
        return Price._extract_decimal(a) + Price._extract_decimal(b)

    def __sub__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) - float(b)
        return Price._extract_decimal(a) - Price._extract_decimal(b)

    def __rsub__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) - float(b)
        return Price._extract_decimal(a) - Price._extract_decimal(b)

    def __mul__(a, b) -> decimal.Decimal or float:
        cdef Price self = a
        if isinstance(b, float):
            return float(a) * float(b)
        return Price._extract_decimal(a) * Price._extract_decimal(b)

    def __rmul__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) * float(b)
        return Price._extract_decimal(a) * Price._extract_decimal(b)

    def __truediv__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) / float(b)
        return Price._extract_decimal(a) / Price._extract_decimal(b)

    def __rtruediv__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) / float(b)
        return Price._extract_decimal(a) / Price._extract_decimal(b)

    def __floordiv__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) // float(b)
        return Price._extract_decimal(a) // Price._extract_decimal(b)

    def __rfloordiv__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) // float(b)
        return Price._extract_decimal(a) // Price._extract_decimal(b)

    def __mod__(a, b) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) % float(b)
        return Price._extract_decimal(a) % Price._extract_decimal(b)

    def __rmod__(b, a) -> decimal.Decimal or float:
        if isinstance(b, float):
            return float(a) % float(b)
        return Price._extract_decimal(a) * Price._extract_decimal(b)

    def __neg__(self) -> decimal.Decimal:
        return self.as_decimal().__neg__()

    def __pos__(self) -> decimal.Decimal:
        return self.as_decimal().__pos__()

    def __abs__(self) -> decimal.Decimal:
        return abs(self.as_decimal())

    def __round__(self, ndigits=None) -> decimal.Decimal:
        return round(self.as_decimal(), ndigits)

    def __float__(self) -> float:
        return price_as_f64(&self._price)

    def __int__(self) -> int:
        return int(price_as_f64(&self._price))

    def __hash__(self) -> int:
        return hash(self._price.value)

    def __str__(self) -> str:
        return f"{price_as_f64(&self._price):.{self._price.precision}f}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self}')"

    def __del__(self) -> None:
        price_free(self._price)  # `self._price` moved to rust (then dropped)

    cdef int64_t raw_int64(self):
        return self._price.value

    @staticmethod
    cdef object _extract_decimal(object obj):
        if hasattr(obj, "as_decimal"):
            return obj.as_decimal()
        return obj

    @staticmethod
    cdef bint _compare(a, b, int op) except *:
        if isinstance(a, Quantity):
            a = <Quantity>a.as_decimal()
        elif isinstance(a, Price):
            a = <Price>a.as_decimal()

        if isinstance(b, Quantity):
            b = <Quantity>b.as_decimal()
        elif isinstance(b, Price):
            b = <Price>b.as_decimal()

        return PyObject_RichCompareBool(a, b, op)

    @property
    def precision(self) -> int:
        return self._price.precision

    @staticmethod
    cdef Price from_str_c(str value):
        return Price(float(value), precision=precision_from_str(value))

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

    cpdef void add_assign(self, Price other) except *:
        assert other._price.precision <= self._price.precision
        self._price.value += other.raw_int64()

    cpdef void sub_assign(self, Price other) except *:
        assert other._price.precision <= self._price.precision
        self._price.value -= other.raw_int64()

    cpdef object as_decimal(self):
        """
        Return the value as a built-in `Decimal`.

        Returns
        -------
        Decimal

        """
        return decimal.Decimal(f"{price_as_f64(&self._price):.{self._price.precision}f}")

    cpdef double as_double(self) except *:
        """
        Return the value as a `double`.

        Returns
        -------
        double

        """
        return price_as_f64(&self._price)


cdef class Money(BaseDecimal):
    """
    Represents an amount of money including currency type.

    Parameters
    ----------
    value : integer, float, string or Decimal
        The amount of money in the currency denomination.
    currency : Currency
        The currency of the money.
    """

    def __init__(self, value, Currency currency not None):
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
            If `value` is malformed.

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
    Represents an account balance denominated in a particular currency.

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
        If money currencies are not equal.
    ValueError
        If any money is negative (< 0).
    ValueError
        If `total` - `locked` != `free`.
    """

    def __init__(
        self,
        Money total not None,
        Money locked not None,
        Money free not None,
    ):
        Condition.equal(total.currency, locked.currency, "total.currency", "locked.currency")
        Condition.equal(total.currency, free.currency, "total.currency", "free.currency")
        Condition.not_negative(total.as_decimal(), "total")
        Condition.not_negative(locked.as_decimal(), "locked")
        Condition.not_negative(free.as_decimal(), "free")
        Condition.true(total.as_decimal() - locked.as_decimal() == free.as_decimal(), "total - locked != free")

        self.total = total
        self.locked = locked
        self.free = free
        self.currency = total.currency

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
            total=Money(values["total"], currency),
            locked=Money(values["locked"], currency),
            free=Money(values["free"], currency),
        )

    @staticmethod
    def from_dict(dict values) -> AccountBalance:
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
            "total": str(self.total.as_decimal()),
            "locked": str(self.locked.as_decimal()),
            "free": str(self.free.as_decimal()),
            "currency": self.currency.code,
        }


cdef class MarginBalance:
    """
    Represents a margin balance optionally associated with a particular instrument.

    Parameters
    ----------
    initial : Money
        The initial (order) margin requirement for the instrument.
    maintenance : Money
        The maintenance (position) margin requirement for the instrument.
    instrument_id : InstrumentId, optional
        The instrument ID associated with the margin.

    Raises
    ------
    ValueError
        If `margin_init` currency does not equal `currency`.
    ValueError
        If `margin_maint` currency does not equal `currency`.
    ValueError
        If any margin is negative (< 0).
    """

    def __init__(
        self,
        Money initial not None,
        Money maintenance not None,
        InstrumentId instrument_id=None,
    ):
        Condition.equal(initial.currency, maintenance.currency, "initial.currency", "maintenance.currency")
        Condition.not_negative(initial.as_decimal(), "initial")
        Condition.not_negative(maintenance.as_decimal(), "maintenance")

        self.initial = initial
        self.maintenance = maintenance
        self.currency = initial.currency
        self.instrument_id = instrument_id

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"initial={self.initial.to_str()}, "
            f"maintenance={self.maintenance.to_str()}, "
            f"instrument_id={self.instrument_id.value if self.instrument_id is not None else None})"
        )

    @staticmethod
    cdef MarginBalance from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef Currency currency = Currency.from_str_c(values["currency"])
        cdef str instrument_id_str = values.get("instrument_id")
        return MarginBalance(
            initial=Money(values["initial"], currency),
            maintenance=Money(values["maintenance"], currency),
            instrument_id=InstrumentId.from_str_c(instrument_id_str) if instrument_id_str is not None else None,
        )

    @staticmethod
    def from_dict(dict values) -> MarginBalance:
        """
        Return a margin balance from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        MarginAccountBalance

        """
        return MarginBalance.from_dict_c(values)

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "initial": str(self.initial.as_decimal()),
            "maintenance": str(self.maintenance.as_decimal()),
            "currency": self.currency.code,
            "instrument_id": self.instrument_id.value if self.instrument_id is not None else None,
        }
