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
from nautilus_trader.model.c_enums.currency cimport Currency, currency_to_string


cdef Quantity _QUANTITY_ZERO = Quantity()
cdef Quantity _QUANTITY_ONE = Quantity(1)

cdef class Quantity(Decimal):
    """
    Represents a quantity with a non-negative value.

    Attributes
    ----------
    precision : int
        The precision of the underlying decimal value.

    """

    def __init__(self, double value=0, int precision=0):
        """
        Initializes a new instance of the Quantity class.

        :param value: The value of the quantity (>= 0).
        :param precision: The decimal precision of the quantity (>= 0).
        :raises ValueError: If the value is negative (< 0).
        :raises ValueError: If the precision is negative (< 0).
        """
        Condition.not_negative(value, 'value')
        super().__init__(value, precision)

    @staticmethod
    cdef Quantity zero():
        """
        Return a quantity of zero.
        
        :return Money.
        """
        return _QUANTITY_ZERO

    @staticmethod
    cdef Quantity one():
        """
        Return a quantity with a value of 1.
        
        :return Quantity.
        """
        return _QUANTITY_ONE

    @staticmethod
    cdef Quantity from_string(str value):
        """
        Return a quantity from the given string. Precision will be inferred from the
        number of digits after the decimal place.

        :param value: The string value to parse.
        :return: Quantity.
        """
        Condition.valid_string(value, 'value')

        return Quantity(float(value), precision=Decimal.precision_from_string(value))

    cpdef Quantity add(self, Quantity other):
        """
        Return a new quantity by adding the given quantity to this quantity.

        :param other: The other quantity to add.
        :return Quantity.
        """
        return Quantity(self._value + other._value, max(self.precision, other.precision))

    cpdef Quantity sub(self, Quantity other):
        """
        Return a new quantity by subtracting the quantity from this quantity.

        :param other: The other quantity to subtract.
        :raises ValueError: If value of the other decimal is greater than this price.
        :return Quantity.
        """
        return Quantity(self._value - other._value, max(self.precision, other.precision))

    cpdef str to_string_formatted(self):
        """
        Return the formatted string representation of this object.
        """
        if self.precision > 0:
            return f'{self._value:.{self.precision}f}'

        if self._value < 1000 or self._value % 1000 != 0:
            return f'{self._value:.{self.precision}f}'

        if self._value < 1000000:
            return f'{self._value / 1000:.{0}f}K'

        cdef str millions = f'{self._value / 1000000:.{3}f}'.rstrip('0').rstrip('.')
        return f'{millions}M'


cdef class Price(Decimal):
    """
    Represents a price of a financial market instrument.
    """

    def __init__(self, double value, int precision):
        """
        Initializes a new instance of the Price class.

        :param value: The value of the price (>= 0).
        :param precision: The decimal precision of the price (>= 0).
        :raises ValueError: If the value is negative (< 0).
        :raises ValueError: If the precision is negative (< 0).
        """
        Condition.not_negative(value, 'value')
        super().__init__(value, precision)

    @staticmethod
    cdef Price from_string(str value):
        """
        Return a price from the given string. Precision will be inferred from the
        number of digits after the decimal place.

        :param value: The string value to parse.
        :return: Price.
        """
        Condition.valid_string(value, 'value')

        return Price(float(value), precision=Decimal.precision_from_string(value))

    cpdef Price add(self, Decimal other):
        """
        Return a new price by adding the given decimal to this price.

        :param other: The other decimal to add (precision must be <= this decimals precision).
        :raises ValueError: If the precision of the other decimal is not <= this precision.
        :return Price.
        """
        Condition.true(self.precision >= other.precision, 'self.precision >= price.precision')

        return Price(self._value + other._value, self.precision)

    cpdef Price sub(self, Decimal other):
        """
        Return a new price by subtracting the decimal price from this price.

        :param other: The other decimal to subtract (precision must be <= this decimals precision).
        :raises ValueError: If the precision of the other decimal is not <= this precision.
        :raises ValueError: If value of the other decimal is greater than this price.
        :return Price.
        """
        Condition.true(self.precision >= other.precision, 'self.precision >= price.precision')

        return Price(self._value - other._value, self.precision)


cdef class Money(Decimal):
    """
    Represents the 'concept' of money including currency type.
    """

    def __init__(self, double value, Currency currency):
        """
        Initializes a new instance of the Money class.
        Note: The value is rounded to 2 decimal places of precision.

        :param value: The value of the money.
        :param currency: The currency of the money.
        """
        Condition.not_equal(currency, Currency.UNDEFINED, 'currency', 'UNDEFINED')
        super().__init__(value, precision=2)

        self.currency = currency

    @staticmethod
    cdef Money from_string(str value, Currency currency):
        """
        Return money parsed from the given string value.
        
        :param value: The string value to parse.
        :param currency: The currency for the money.
        :return Money.
        """
        Condition.valid_string(value, 'value')

        return Money(float(value), currency)

    cpdef Money add(self, Money other):
        """
        Return new money by adding the given money to this money.

        :param other: The other money to add.
        :return Money.
        :raises ValueError: If the other currency is not equal to this money.
        """
        # TODO: Fix handling of multiple currencies
        # Condition.equal(self.currency, other.currency, 'self.currency', 'other.currency')

        return Money(self._value + other._value, self.currency)

    cpdef Money sub(self, Money other):
        """
        Return new money by subtracting the given money from this money.

        :param other: The other money to subtract.
        :return Money.
        :raises ValueError: If the other currency is not equal to this money.
        """
        # TODO: Fix handling of multiple currencies
        # Condition.equal(self.currency, other.currency, 'self.currency', 'other.currency')

        return Money(self._value - other._value, self.currency)

    cpdef bint equals(self, Decimal other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        :raises ValueError: If the other is not of type Money.
        """
        Condition.type(other, Money, 'other')

        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value == other._value and self.currency == other.currency

    cpdef str to_string_formatted(self):
        """
        Return the formatted string representation of this object.
        """
        return f'{self.to_string(format_commas=True)} {currency_to_string(self.currency)}'

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return (f"<{self.__class__.__name__}({self.to_string()}, currency={currency_to_string(self.currency)}) "
                f"object at {id(self)}>")
