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

import decimal
import numbers

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport precision_from_string


cdef dict _QUANTIZE_MAP = {
    0: decimal.Decimal('0'),
    1: decimal.Decimal('0.1'),
    2: decimal.Decimal('0.' + '0' * 1 + '1'),
    3: decimal.Decimal('0.' + '0' * 2 + '1'),
    4: decimal.Decimal('0.' + '0' * 3 + '1'),
    5: decimal.Decimal('0.' + '0' * 4 + '1'),
    6: decimal.Decimal('0.' + '0' * 5 + '1'),
    7: decimal.Decimal('0.' + '0' * 6 + '1'),
    8: decimal.Decimal('0.' + '0' * 7 + '1'),
    9: decimal.Decimal('0.' + '0' * 8 + '1'),
    10: decimal.Decimal('0.' + '0' * 9 + '1'),
    11: decimal.Decimal('0.' + '0' * 10 + '1'),
    12: decimal.Decimal('0.' + '0' * 11 + '1'),
    13: decimal.Decimal('0.' + '0' * 12 + '1'),
    14: decimal.Decimal('0.' + '0' * 13 + '1'),
    15: decimal.Decimal('0.' + '0' * 14 + '1'),
    16: decimal.Decimal('0.' + '0' * 15 + '1'),
}

cdef class Decimal64:
    """
    Represents a decimal64 floating point value with a fixed precision context.

    Rounding behaviour is as per the built-in decimal.Decimal type.
    """

    def __init__(self, value=0, int precision=0):
        """
        Initialize a new instance of the Decimal64 class.

        Parameters
        ----------
        value : integer, string, tuple, float, or decimal.Decimal.
            The value for the decimal.
        precision : int, optional
            The precision for the decimal.

        Raises
        ------
        ValueError
            If precision is negative (< 0).

        """
        Condition.not_negative(precision, "precision")

        # https://docs.python.org/3.9/library/decimal.html?highlight=quantize#decimal.Decimal.quantize
        self._value = decimal.Decimal(value).quantize(_QUANTIZE_MAP.get(precision))
        self.precision = precision

    @staticmethod
    cdef Decimal64 from_string_to_decimal(str value):
        """
        Return a decimal from the given string.

        Precision will be inferred from the number of digits after the decimal place.
        Note: If no decimal place then precision will be zero.

        Parameters
        ----------
        value : str
            The string value to parse.

        Returns
        -------
        Decimal64

        """
        Condition.valid_string(value, "value")

        return Decimal64(value, precision=precision_from_string(value))

    cpdef str to_string(self, bint format_commas=False):
        """
        Return the formatted string representation of this object.

        Parameters
        ----------
        format_commas : bool
            If the string should be formatted with commas separating thousands.

        Returns
        -------
        str

        """
        if format_commas:
            return f"{self._value:,}"
        else:
            return str(self._value)

    cpdef int as_int(self):
        """
        Return the internal value as an integer.

        Returns
        -------
        int

        """
        return int(self._value)

    cpdef double as_double(self):
        """
        Return the internal value as a real number.

        Returns
        -------
        double

        """
        return float(self._value)

    cpdef object as_decimal(self):
        """
        Return the internal value as a built-in decimal.

        Returns
        -------
        decimal.Decimal

        """
        return self._value

    cpdef bint is_zero(self) except *:
        """
        Return a value indicating whether the value of the decimal is equal to zero.

        Returns
        -------
        bool

        """
        return self._value == _QUANTIZE_MAP[0]

    cpdef bint eq(self, Decimal64 other) except *:
        """
        Return a value indicating whether this decimal is equal to (==) the given decimal.

        Parameters
        ----------
        other : Decimal64
            The other decimal for the equality check.

        Returns
        -------
        bool

        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value == other._value

    cpdef bint ne(self, Decimal64 other) except *:
        """
        Return a value indicating whether this decimal is not equal to (!=) the
        given decimal.

        Parameters
        ----------
        other : Decimal64
            The other decimal for the equality check.

        Returns
        -------
        bool

        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value != other._value

    cpdef bint lt(self, Decimal64 other) except *:
        """
        Return a value indicating whether this decimal is less than (<) the
        given decimal.

        Parameters
        ----------
        other : Decimal64
            The other decimal for the comparison.

        Returns
        -------
        bool

        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value < other._value

    cpdef bint le(self, Decimal64 other) except *:
        """
        Return a value indicating whether this decimal is less than or equal to
        (<=) the given decimal.

        Parameters
        ----------
        other : Decimal64
            The other decimal for the comparison.

        Returns
        -------
        bool

        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value <= other._value

    cpdef bint gt(self, Decimal64 other) except *:
        """
        Return a value indicating whether this decimal is greater than (>) the
        given decimal.

        Parameters
        ----------
        other : Decimal64
            The other decimal for the comparison.

        Returns
        -------
        bool

        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value > other._value

    cpdef bint ge(self, Decimal64 other) except *:
        """
        Return a value indicating whether this decimal is greater than or equal
        to (>=) the given decimal.

        Parameters
        ----------
        other : Decimal64
            The other decimal for the comparison.

        Returns
        -------
        bool

        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value >= other._value

    def __eq__(self, other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        bool

        """
        if isinstance(other, Decimal64):
            return self.eq(other)

        return self._value == other

    def __ne__(self, other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        bool

        """
        if isinstance(other, Decimal64):
            return self.ne(other)

        return self._value != other

    def __lt__(self, other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        bool

        """
        if isinstance(other, Decimal64):
            return self.lt(other)

        return self._value < other

    def __le__(self, other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given
        object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        bool

        """
        if isinstance(other, Decimal64):
            return self.le(other)

        return self._value <= other

    def __gt__(self, other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        bool

        """
        if isinstance(other, Decimal64):
            return self.gt(other)

        return self._value > other

    def __ge__(self, other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given
        object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        bool

        """
        if isinstance(other, Decimal64):
            return self.ge(other)

        return self._value >= other

    def __add__(self, other) -> float:
        """
        Return the result of adding the given object to this object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        float

        """
        if isinstance(other, numbers.Number):
            return self.as_double() + other
        if isinstance(other, Decimal64):
            return Decimal64(self.as_double() + other.as_double(), max(self.precision, other.precision))

        return self._value + other

    def __sub__(self, other) -> float:
        """
        Return the result of subtracting the given object from this object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        float

        """
        if isinstance(other, numbers.Number):
            return self.as_double() - other
        if isinstance(other, Decimal64):
            return Decimal64(self.as_double() - other.as_double(), max(self.precision, other.precision))

        return self._value - other

    def __truediv__(self, other) -> float:
        """
        Return the result of dividing this object by the given object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        decimal.Decimal

        """
        if isinstance(other, numbers.Number):
            return self.as_double() / other
        if isinstance(other, Decimal64):
            return Decimal64(self.as_double() / other.as_double(), max(self.precision, other.precision))

        return self._value / other

    def __mul__(self, other) -> float:
        """
        Return the result of multiplying this object by the given object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        decimal.Decimal

        """
        if isinstance(other, numbers.Number):
            return self.as_double() * other
        if isinstance(other, Decimal64):
            return Decimal64(self.as_double() * other.as_double(), max(self.precision, other.precision))

        return self._value * other

    def __hash__(self) -> int:
        """"
         Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self._value)

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
        return (f"<{self.__class__.__name__}({self._value}, "
                f"precision={self.precision}) object at {id(self)}>")
