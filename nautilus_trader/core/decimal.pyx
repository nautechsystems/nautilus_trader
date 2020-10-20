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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport precision_from_string
from decimal import Decimal as PyDecimal


cdef class Decimal:
    """
    Represents a financial decimal number with a fixed precision.

    Attributes
    ----------
    precision : int
        The precision of the decimal.

    """

    def __init__(self, value="0"):
        """
        Initialize a new instance of the Decimal class.

        Parameters
        ----------
        value : integer, string, decimal.Decimal or Decimal.
            The value of the quantity.

        Raises
        ------
        TypeError
            If value is a float.

        """
        Condition.not_none(value, "value")

        if isinstance(value, float):
            raise TypeError("decimal precision cannot be inferred from a float, please use from_float()")
        elif isinstance(value, Decimal):
            self._value = value.as_decimal()
        else:
            self._value = PyDecimal(value)

        self.precision = precision_from_string(str(self._value))

        if self.precision < 0:
            raise RuntimeError(f"Invalid decimal precision, was {self.precision}")

    @staticmethod
    cdef inline tuple _convert_values(object a, object b):
        if isinstance(a, Decimal):
            a = a._value
        if isinstance(b, Decimal):
            b = b._value
        if isinstance(a, float) or isinstance(b, float):
            return float(a), float(b)
        return a, b

    def __eq__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return NotImplemented
        return a == b

    def __ne__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return NotImplemented
        return a != b

    def __lt__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return NotImplemented
        return a < b

    def __le__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return NotImplemented
        return a <= b

    def __gt__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return NotImplemented
        return a > b

    def __ge__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return NotImplemented
        return a >= b

    def __add__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return a + b
        else:
            return Decimal(a + b)

    def __sub__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return a - b
        else:
            return Decimal(a - b)

    def __mul__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return a * b
        else:
            return Decimal(a * b)

    def __div__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return a / b
        else:
            return Decimal(a / b)

    def __truediv__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return a / b
        else:
            return Decimal(a / b)

    def __floordiv__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return a // b
        else:
            return Decimal(a // b)

    def __mod__(self, other):
        a, b = Decimal._convert_values(self, other)
        if isinstance(a, float):
            return NotImplemented
        else:
            return Decimal(a % b)

    def __float__(self):
        """Return the objects value as a float."""
        return float(self._value)

    def __abs__(self):
        """abs(a)"""
        return Decimal(abs(self._value))

    def __round__(self, ndigits=None):
        """round(self, ndigits)
        Rounds half toward even.
        """
        return Decimal(round(self._value, ndigits))

    def __hash__(self) -> int:
        """
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
        return str(self._value)

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

        return Decimal.from_float_to_decimal(value, precision)

    @staticmethod
    cdef inline Decimal from_float_to_decimal(double value, int precision):
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

        return Decimal(f'{value:.{precision}f}')

    cpdef object as_decimal(self):
        """
        Return this object as a builtin decimal.Decimal.

        Returns
        -------
        decimal.Decimal

        """
        return self._value

    cpdef double as_double(self) except *:
        """
        Return the value of the decimal as a double.

        Returns
        -------
        double

        """
        return float(self._value)
