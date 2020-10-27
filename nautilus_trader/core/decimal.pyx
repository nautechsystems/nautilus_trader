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

from decimal import Decimal as PyDecimal

from nautilus_trader.core.correctness cimport Condition


cdef class Decimal:
    """
    Represents a decimal number with a specified precision.

    The type interoperates with the built-in `decimal.Decimal` correctly.

    """

    def __init__(self, value=0, precision=None):
        """
        Initialize a new instance of the Decimal class.

        Parameters
        ----------
        value : integer, float, string, decimal.Decimal or Decimal
            The value of the decimal. If value is a float, then a precision must
            be specified.
        precision : int, optional
            The precision for the decimal. If a precision is specified then the
            value will be rounded to the precision. Else the precision will be
            inferred from the given value.

        Raises
        ------
        TypeError
            If value is a float and precision is not specified.
        ValueError
            If precision is negative (< 0).

        """
        Condition.not_none(value, "value")

        if precision is None:  # Infer precision
            if isinstance(value, float):
                raise TypeError("precision cannot be inferred from a float, "
                                "please specify a precision when passing a float")
            elif isinstance(value, Decimal):
                self._value = value._value
            else:
                self._value = PyDecimal(value)
        else:
            Condition.not_negative_int(precision, "precision")
            if not isinstance(value, float):
                value = float(value)
            self._value = PyDecimal(f'{value:.{precision}f}')

    def __eq__(self, other) -> bool:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return NotImplemented
        return a == b

    def __ne__(self, other) -> bool:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return NotImplemented
        return a != b

    def __lt__(self, other) -> bool:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return NotImplemented
        return a < b

    def __le__(self, other) -> bool:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return NotImplemented
        return a <= b

    def __gt__(self, other) -> bool:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return NotImplemented
        return a > b

    def __ge__(self, other) -> bool:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return NotImplemented
        return a >= b

    def __add__(self, other) -> Decimal or float:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return a + b
        else:
            return Decimal(a + b)

    def __sub__(self, other) -> Decimal or float:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return a - b
        else:
            return Decimal(a - b)

    def __mul__(self, other) -> Decimal or float:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return a * b
        else:
            return Decimal(a * b)

    def __div__(self, other) -> Decimal or float:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return a / b
        else:
            return Decimal(a / b)

    def __truediv__(self, other) -> Decimal or float:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return a / b
        else:
            return Decimal(a / b)

    def __floordiv__(self, other) -> Decimal or float:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return a // b
        else:
            return Decimal(a // b)

    def __mod__(self, other) -> Decimal:
        a, b = Decimal._convert_values(self, other)
        if isinstance(b, float):
            return NotImplemented
        else:
            return Decimal(a % b)

    def __neg__(self):
        """
        Return a copy with the sign switched.

        Rounds, if it has reason.

        Returns
        -------
        Decimal

        """
        return Decimal(self._value.__neg__())

    def __pos__(self):
        """
        Return a copy, unless it is a NaN.

        Rounds the number (if more than precision digits)

        Returns
        -------
        Decimal

        """
        return Decimal(self._value.__pos__())

    def __abs__(self):
        """
        Returns
        -------
        Decimal
            The absolute value of the decimal.

        """
        return Decimal(abs(self._value))

    def __round__(self, ndigits=None):
        """
        Round the decimal.

        Rounds half toward even.

        Parameters
        ----------
        ndigits : int
            The number of digits to round to.

        Returns
        -------
        Decimal
            A rounded copy of this object.

        """
        return Decimal(round(self._value, ndigits))

    def __float__(self):
        return float(self._value)

    def __int__(self):
        return int(self._value)

    def __hash__(self) -> int:
        return hash(self._value)

    def __str__(self) -> str:
        return str(self._value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self}')"

    @staticmethod
    cdef inline tuple _convert_values(object a, object b):
        if isinstance(a, float) or isinstance(b, float):
            return float(a), float(b)
        if isinstance(a, Decimal):
            a = a._value
        if isinstance(b, Decimal):
            b = b._value
        return a, b

    @property
    def precision(self):
        """
        The precision of the decimal.

        Returns
        -------
        int

        """
        return abs(self._value.as_tuple().exponent)

    cpdef object as_decimal(self):
        """
        The value of the decimal as a built-in `decimal.Decimal`.

        Returns
        -------
        decimal.Decimal

        """
        return self._value

    cpdef double as_double(self) except *:
        """
        The value of the decimal as a `double`.

        Returns
        -------
        double

        """
        return float(self._value)
