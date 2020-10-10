# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

"""Define common basic value objects in the trading domain."""

from cpython.object cimport Py_GE
from cpython.object cimport Py_GT
from cpython.object cimport Py_LE
from cpython.object cimport Py_LT

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport precision_from_string
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.quicktions cimport Fraction


cdef class Decimal(Fraction):
    """
    Represents a decimal with a fixed precision.

    Attributes
    ----------
    precision : int
        The precision of the decimal.

    """

    def __init__(self, value=0):
        """
        Initialize a new instance of the Decimal class.

        Parameters
        ----------
        value : integer, string, decimal.Decimal or Fraction.
            The value of the quantity.

        Raises
        ------
        TypeError
            If value is a float.

        """
        Condition.not_none(value, "value")

        if isinstance(value, float):
            raise TypeError("decimal precision cannot be inferred from a float, please use from_float()")
        super().__init__(value)

        self.precision = precision_from_string(str(value))

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the
        objects location in memory.

        Returns
        -------
        str

        """
        return f"<{self.__class__.__name__}({self}) object at {id(self)}>"

    cdef inline Decimal add(self, Fraction other):
        """
        Add the other decimal to this decimal.

        Parameters
        ----------
        other : Decimal
            The decimal to add.

        Returns
        -------
        Decimal

        """
        return Decimal(self + other)

    cdef inline Decimal sub(self, Fraction other):
        """
        Subtract the other decimal from this decimal.

        Parameters
        ----------
        other : Decimal
            The decimal to subtract.

        Returns
        -------
        Decimal

        """
        return Decimal(self - other)

    @staticmethod
    def from_float(value: float, precision: int):
        """
        Return a decimal from the given value and precision.

        Parameters
        ----------
        value : double
            The value of the decimal.
        precision : int, optional.
            The precision for the decimal.

        Raises
        ------
        ValueError
            If precision is negative (< 0).

        """
        Condition.type(precision, int, "precision")

        return Decimal.from_float_c(value, precision)

    @staticmethod
    cdef inline Decimal from_float_c(double value, int precision):
        """
        Return a decimal from the given value and precision.

        Returns
        -------
        Decimal

        Raises
        ------
        ValueError
            If precision is negative (< 0)

        """
        Condition.not_negative_int(precision, "precision")

        return Decimal(format(value, f'.{precision}f'))

    cpdef double as_double(self) except *:
        """
        Return the value of the decimal as a double.

        Returns
        -------
        double

        """
        return self._numerator / self._denominator

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return format(self.as_double(), f'.{self.precision}f')


cdef class Quantity(Fraction):
    """
    Represents a quantity with a non-negative value.

    Attributes
    ----------
    precision : int
        The decimal precision of this object.

    """

    def __init__(self, value=0):
        """
        Initialize a new instance of the Quantity class.

        Parameters
        ----------
        value : integer, string, decimal.Decimal or Fraction.
            The value of the quantity.

        Raises
        ------
        TypeError
            If value is a float.
        ValueError
            If value is negative (< 0).

        """
        Condition.not_none(value, "value")

        if isinstance(value, float):
            raise TypeError("decimal precision cannot be inferred from a float, please use from_float()")
        super().__init__(value)

        self.precision = precision_from_string(str(value))

        # Post Condition
        Condition.true(self >= 0, f"quantity not positive, was {self.to_string()}")

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the
        objects location in memory.

        Returns
        -------
        str

        """
        return f"<{self.__class__.__name__}({self}) object at {id(self)}>"

    cdef inline Quantity add(self, Quantity other):
        """
        Add the other quantity to this quantity.

        Parameters
        ----------
        other : Quantity
            The quantity to add.

        Returns
        -------
        Quantity

        """
        return Quantity(self + other)

    cdef inline Quantity sub(self, Quantity other):
        """
        Subtract the other quantity from this quantity.

        Parameters
        ----------
        other : Fraction
            The quantity to subtract.

        Returns
        -------
        Quantity

        """
        return Quantity(self - other)

    @staticmethod
    def from_float(value: float, precision: int):
        """
        Return a quantity from the given value and precision.

        Parameters
        ----------
        value : double
            The value of the quantity.
        precision : int, optional.
            The decimal precision for the quantity.

        Raises
        ------
        ValueError
            If value is negative (< 0).
        ValueError
            If precision is negative (< 0).

        """
        Condition.type(precision, int, "precision")

        return Quantity.from_float_c(value, int(precision))

    @staticmethod
    cdef inline Quantity from_float_c(double value, int precision):
        """
        Return a quantity from the given value and precision.

        Parameters
        ----------
        value : double
            The value of the quantity.
        precision : int, optional.
            The decimal precision for the quantity.

        Raises
        ------
        ValueError
            If value is negative (< 0).
        ValueError
            If precision is negative (< 0).

        """
        Condition.not_negative_int(precision, "precision")

        return Quantity(format(value, f'.{precision}f'))

    cpdef double as_double(self) except *:
        """
        Return the value of the quantity as a double.

        Returns
        -------
        double

        """
        return self._numerator / self._denominator

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return format(self.as_double(), f'.{self.precision}f')

    cpdef str to_string_formatted(self):
        """
        Return the formatted string representation of this object.

        Returns
        -------
        str

        """
        cdef double self_as_double = self.as_double()

        if self.precision > 0:
            return f"{self_as_double:.{self.precision}f}"

        if self < 1000 or self % 1000 != 0:
            return f"{self_as_double:,.0f}"

        if self < 1000000:
            return f"{round(self_as_double / 1000)}K"

        cdef str millions = f"{self_as_double / 1000000:.3f}".rstrip("0").rstrip(".")
        return f"{millions}M"


cdef class Price(Fraction):
    """
    Represents a price in a financial market.

    Attributes
    ----------
    precision : int
        The decimal precision of the price.

    """

    def __init__(self, value=0):
        """
        Initialize a new instance of the Price class.

        Parameters
        ----------
        value : integer, string, decimal.Decimal or Fraction.
            The value of the price (>= 0).

        Raises
        ------
        TypeError
            If value is a float.
        ValueError
            If value is negative (< 0).

        """
        Condition.not_none(value, "value")

        if isinstance(value, float):
            raise TypeError("decimal precision cannot be inferred from a float, please use from_float()")
        super().__init__(value)

        self.precision = precision_from_string(str(value))

        # Post Condition
        Condition.true(self >= 0, f"price not positive, was {self}")

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the
        objects location in memory.

        Returns
        -------
        str

        """
        return f"<{self.__class__.__name__}({self}) object at {id(self)}>"

    cdef inline Price add(self, Fraction other):
        """
        Add the other price to this price.

        Parameters
        ----------
        other : Fraction
            The fractional value to add.

        Returns
        -------
        Price

        """
        return Price(self + other)

    cdef inline Price sub(self, Fraction other):
        """
        Subtract the other price from this price.

        Parameters
        ----------
        other : Fraction
            The fractional value to subtract.

        Returns
        -------
        Price

        """
        return Price(self - other)

    @staticmethod
    def from_float(value: float, precision: int) -> Price:
        """
        Return a price from the given value and precision.

        Parameters
        ----------
        value : double
            The value of the price.
        precision : int.
            The decimal precision of the price.

        Raises
        ------
        ValueError
            If value is negative (< 0).
        ValueError
            If precision is negative (< 0).

        """
        Condition.type(precision, int, "precision")

        return Price.from_float_c(value, precision)

    @staticmethod
    cdef inline Price from_float_c(double value, int precision):
        """
        Return a price from the given value and precision.

        Parameters
        ----------
        value : double
            The value of the price.
        precision : int.
            The decimal precision of the price.

        Raises
        ------
        ValueError
            If value is negative (< 0).
        ValueError
            If precision is negative (< 0).

        """
        Condition.not_negative_int(precision, "precision")

        return Price(format(value, f'.{precision}f'))

    cpdef double as_double(self) except *:
        """
        Return the value of the price as a double.

        Returns
        -------
        double

        """
        return self._numerator / self._denominator

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return format(self.as_double(), f'.{self.precision}f')


cdef class Money(Fraction):
    """
    Represents an amount of money including currency type.

    Attributes
    ----------
    currency : Currency
        The currency of the money.

    """

    def __init__(self, value, Currency currency not None):
        """
        Initialize a new instance of the Money class.

        Parameters
        ----------
        value : integer, float, string, decimal.Decimal or Fraction.
            The value of the money.
        currency : Currency
            The currency of the money.

        """
        Condition.not_none(value, "value")

        if not isinstance(value, float):
            value = float(value)
        super().__init__(f"{value:.{currency.precision}f}")

        self.currency = currency

    def __eq__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : Money
            The other object to equate.

        Returns
        -------
        bool

        """
        return self._eq(other) and self.currency == other.currency

    def __ne__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : Money
            The other object to equate.

        Returns
        -------
        bool

        """
        return not self == other

    def __lt__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        Parameters
        ----------
        other : Money
            The other object to equate.

        Returns
        -------
        bool

        """
        return self._richcmp(other, Py_LT) and self.currency == other.currency

    def __le__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        Parameters
        ----------
        other : Money
            The other object to equate.

        Returns
        -------
        bool

        """
        return self._richcmp(other, Py_LE) and self.currency == other.currency

    def __gt__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        Parameters
        ----------
        other : Money
            The other object to equate.

        Returns
        -------
        bool

        """
        return self._richcmp(other, Py_GT) and self.currency == other.currency

    def __ge__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        Parameters
        ----------
        other : Money
            The other object to equate.

        Returns
        -------
        bool

        """
        return self._richcmp(other, Py_GE) and self.currency == other.currency

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        Notes
        -----
        The hash is based on the ticks timestamp only.

        Returns
        -------
        int

        """
        return hash(self.to_string_formatted())

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return (f"<{self.__class__.__name__}({self}, currency={self.currency}) "
                f"object at {id(self)}>")

    cdef inline Money add(self, Money other):
        """
        Add the given money to this money.

        Parameters
        ----------
        other : Money
            The money to add.

        Returns
        -------
        Money

        Raises
        ------
        ValueError
            If currency is not equal to this currency

        """
        Condition.equal(self.currency, other.currency, "self.currency", "other.currency")
        return Money(self + other, self.currency)

    cdef inline Money sub(self, Money other):
        """
        Subtract the given money from this money.

        Parameters
        ----------
        other : Money
            The money to subtract.

        Returns
        -------
        Money

        Raises
        ------
        ValueError
            If currency is not equal to this currency

        """
        Condition.equal(self.currency, other.currency, "self.currency", "other.currency")
        return Money(self - other, self.currency)

    cpdef double as_double(self) except *:
        """
        Return the value of the money as a double.

        Returns
        -------
        double

        """
        return self._numerator / self._denominator

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return format(self.as_double(), f'.{self.currency.precision}f')

    cpdef str to_string_formatted(self):
        """
        Return the formatted string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.as_double():,.{self.currency.precision}f} {self.currency}"
