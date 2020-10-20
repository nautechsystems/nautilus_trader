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
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.currency cimport Currency


cdef class Quantity(Decimal):
    """
    Represents a quantity with a non-negative value.

    Capable of storing either a whole number (no decimal places) of “shares”
    (securities denominated in whole units) or a decimal value containing
    decimal places for non-share quantity asset classes (securities denominated
    in fractional units).

    Attributes
    ----------
    precision : int
        The decimal precision of this object.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/index.html#Qty

    """

    def __init__(self, value=0):
        """
        Initialize a new instance of the Quantity class.

        Parameters
        ----------
        value : integer, string, decimal.Decimal or Decimal.
            The value of the quantity.

        Raises
        ------
        TypeError
            If value is a float (use the from_float method).
        ValueError
            If value is negative (< 0).

        """
        super().__init__(value)

        # Post Condition
        Condition.true(self >= 0, f"quantity positive, was {self.to_string()}")

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
        return f"<{self.__class__.__name__}('{self}') object at {id(self)}>"

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

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return str(self._value)

    cpdef str to_string_formatted(self):
        """
        Return the formatted string representation of this object.

        Returns
        -------
        str

        """
        if self.precision > 0:
            return str(self)

        if self < 1000 or self % 1000 != 0:
            return f"{self.as_double():,.0f}"

        if self < 1000000:
            return f"{round(self / 1000)}K"

        cdef str millions = f"{self.as_double() / 1000000:.3f}".rstrip("0").rstrip(".")
        return f"{millions}M"


cdef class Price(Decimal):
    """
    Represents a price in a financial market.

    The number of decimal places may vary. For certain asset classes prices may
    be negative values. For example, prices for options strategies can be
    negative under certain market conditions.

    Attributes
    ----------
    precision : int
        The decimal precision of the price.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/index.html#Qty

    """

    def __init__(self, value=0):
        """
        Initialize a new instance of the Price class.

        Parameters
        ----------
        value : integer, string, decimal.Decimal or Decimal.
            The value of the price.

        Raises
        ------
        TypeError
            If value is a float (use the from_float method).

        """
        super().__init__(value)

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
        return f"<{self.__class__.__name__}('{self}') object at {id(self)}>"

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

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return str(self._value)


cdef class Money(Decimal):
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
        value : integer, float, string, decimal.Decimal or Decimal.
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
        return self._value == other._value and self.currency == other.currency

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
        return self._value < other._value and self.currency == other.currency

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
        return self._value <= other._value and self.currency == other.currency

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
        return self._value > other._value and self.currency == other.currency

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
        return self._value >= other._value and self.currency == other.currency

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
        return f"<{self.__class__.__name__}('{self._value}', {self.currency}) object at {id(self)}>"

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return str(self._value)

    cpdef str to_string_formatted(self):
        """
        Return the formatted string representation of this object.

        Returns
        -------
        str

        """
        # TODO implement __format__ on Decimal
        return f"{self.as_double():,.{self.currency.precision}f} {self.currency}"
