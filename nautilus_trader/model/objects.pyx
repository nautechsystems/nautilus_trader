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
        Condition.not_negative_int(precision, "precision")

        return Decimal(format(value, f'.{precision}f'))

    cpdef double as_double(self):
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
        return Quantity(self + other)

    cdef inline Quantity sub(self, Quantity other):
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
        Condition.not_negative_int(precision, "precision")

        return Quantity(format(value, f'.{precision}f'))

    cpdef double as_double(self):
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
        return f"{self.as_double():,.{self.precision}f}"


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

    @staticmethod
    def from_float(value: float, precision: int) -> Price:
        """
        Return a price from the given float and precision.

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
        Condition.not_negative_int(precision, "precision")

        return Price(format(value, f'.{precision}f'))

    cpdef double as_double(self):
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
        if not isinstance(value, float):
            value = float(value)
        super().__init__(f"{value:.{currency.precision}f}")

        self.currency = currency

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
        return Money(self + other, self.currency)

    cdef inline Money sub(self, Money other):
        return Money(self - other, self.currency)

    @staticmethod
    cdef Money zero(Currency currency):
        return Money(0, currency)

    cpdef double as_double(self):
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
