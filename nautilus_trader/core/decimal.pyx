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
    """Represents a decimal number with a specified precision.

    The type interoperates with the built-in decimal.Decimal correctly.

    Attributes
    ----------
    precision : int
        The precision of the decimal.

    """

    def __init__(self, value="0", precision=None):
        """Initialize a new instance of the Decimal class.

        Parameters
        ----------
        value : int, float, decimal.Decimal or Decimal
            The value of the decimal. If value is a float, then a precision must
            be specified.
        precision : int, optional
            The precision for the decimal. If a precision is specified then the
            decimals value will be rounded to the precision. Else the precision
            will be inferred from the given value.

        Raises
        ------
        TypeError
            If value is a float and precision is not specified.
        ValueError
            If precision is negative (< 0).

        """
        Condition.not_none(value, "value")

        if precision is not None:
            Condition.not_negative_int(precision, "precision")
            self._value = PyDecimal(f'{float(value):.{precision}f}')
            self.precision = precision
        else:  # Infer precision
            if isinstance(value, float):
                raise TypeError("precision cannot be inferred from a float, "
                                "please specify a precision when passing a float")
            elif isinstance(value, Decimal):
                self._value = value._value
            else:
                self._value = PyDecimal(value)

            self.precision = precision_from_string(str(self._value))

            # Post-condition
            Condition.not_negative_int(self.precision, "precision")

    @staticmethod
    cdef inline tuple _convert_values(object a, object b):
        if isinstance(a, float) or isinstance(b, float):
            return float(a), float(b)
        if isinstance(a, Decimal):
            a = a._value
        if isinstance(b, Decimal):
            b = b._value
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

    def __neg__(self):
        """Returns a copy with the sign switched.

        Rounds, if it has reason.
        """
        return Decimal(self._value.__neg__())

    def __pos__(self):
        """Returns a copy, unless it is a sNaN.

        Rounds the number (if more than precision digits)
        """
        return Decimal(self._value.__pos__())

    def __abs__(self):
        """abs(a)"""
        return Decimal(abs(self._value))

    def __round__(self, ndigits=None):
        """Return a rounded copy of this object.

        Rounds half toward even.

        Parameters
        ----------
        ndigits : int
            The number of digits to round to.

        """
        return Decimal(round(self._value, ndigits))

    def __float__(self):
        """Return the objects value as a float.

        Returns
        -------
        float

        """
        return float(self._value)

    def __int__(self):
        """Converts self to an int, truncating if necessary.

        Returns
        -------
        int

        """
        return int(self._value)

    def __hash__(self) -> int:
        """Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self._value)

    def __str__(self) -> str:
        """Return the string representation of this object.

        Returns
        -------
        str

        """
        return str(self._value)

    def __repr__(self) -> str:
        """Return the string representation of this object

        The string includes the objects location in memory.

        Returns
        -------
        str

        """
        return f"<{self.__class__.__name__}('{self}') object at {id(self)}>"

    cpdef object as_decimal(self):
        """Return this object as a built-in `decimal.Decimal`.

        Returns
        -------
        decimal.Decimal

        """
        return self._value

    cpdef double as_double(self) except *:
        """Return the value of this object as a `double`.

        Returns
        -------
        double

        """
        return float(self._value)
