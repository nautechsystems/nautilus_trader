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

"""
The `Decimal` type is used as a robust wrapper around the built-in decimal.Decimal.

The type is intended to be used as both a first class value type, as well as the
base class to fundamental domain model value types. One difference from the built-in
decimal is that the specification of precision is more straight forward than providing
a context. Also this type is able to be used as an operand for mathematical ops
with `float` objects.
"""

import decimal

from cpython.object cimport PyObject_RichCompareBool
from cpython.object cimport Py_EQ
from cpython.object cimport Py_GE
from cpython.object cimport Py_GT
from cpython.object cimport Py_LE
from cpython.object cimport Py_LT
from cpython.object cimport Py_NE

from nautilus_trader.core.correctness cimport Condition


cdef int _MATH_ADD = 0
cdef int _MATH_SUB = 1
cdef int _MATH_MUL = 2
cdef int _MATH_DIV = 3
cdef int _MATH_TRUEDIV = 4
cdef int _MATH_FLOORDIV = 5
cdef int _MATH_MOD = 6


cdef class Decimal:
    """
    Represents a decimal number with a specified precision.
    """

    def __init__(self, value=0, precision=None):
        """
        Initialize a new instance of the `Decimal` class.

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
                self._value = decimal.Decimal(value)
        else:
            Condition.not_negative_int(precision, "precision")
            if not isinstance(value, float):
                value = float(value)
            self._value = decimal.Decimal(f'{value:.{precision}f}')

    def __eq__(self, other) -> bool:
        return Decimal._compare(self, other, Py_EQ)

    def __ne__(self, other) -> bool:
        return Decimal._compare(self, other, Py_NE)

    def __lt__(self, other) -> bool:
        return Decimal._compare(self, other, Py_LT)

    def __le__(self, other) -> bool:
        return Decimal._compare(self, other, Py_LE)

    def __gt__(self, other) -> bool:
        return Decimal._compare(self, other, Py_GT)

    def __ge__(self, other) -> bool:
        return Decimal._compare(self, other, Py_GE)

    def __add__(self, other) -> Decimal or float:
        if isinstance(self, float) or isinstance(other, float):
            return Decimal._eval_double(self, other, _MATH_ADD)
        else:
            return Decimal(Decimal._extract_value(self) + Decimal._extract_value(other))

    def __sub__(self, other) -> Decimal or float:
        if isinstance(self, float) or isinstance(other, float):
            return Decimal._eval_double(self, other, _MATH_SUB)
        else:
            return Decimal(Decimal._extract_value(self) - Decimal._extract_value(other))

    def __mul__(self, other) -> Decimal or float:
        if isinstance(self, float) or isinstance(other, float):
            return Decimal._eval_double(self, other, _MATH_MUL)
        else:
            return Decimal(Decimal._extract_value(self) * Decimal._extract_value(other))

    def __div__(self, other) -> Decimal or float:
        if isinstance(self, float) or isinstance(other, float):
            return Decimal._eval_double(self, other, _MATH_DIV)
        else:
            return Decimal(Decimal._extract_value(self) / Decimal._extract_value(other))

    def __truediv__(self, other) -> Decimal or float:
        if isinstance(self, float) or isinstance(other, float):
            return Decimal._eval_double(self, other, _MATH_TRUEDIV)
        else:
            return Decimal(Decimal._extract_value(self) / Decimal._extract_value(other))

    def __floordiv__(self, other) -> Decimal or float:
        if isinstance(self, float) or isinstance(other, float):
            return Decimal._eval_double(self, other, _MATH_FLOORDIV)
        else:
            return Decimal(Decimal._extract_value(self) // Decimal._extract_value(other))

    def __mod__(self, other) -> Decimal:
        if isinstance(self, float) or isinstance(other, float):
            return Decimal._eval_double(self, other, _MATH_MOD)
        else:
            return Decimal(Decimal._extract_value(self) % Decimal._extract_value(other))

    def __neg__(self):
        return Decimal(self._value.__neg__())

    def __pos__(self):
        return Decimal(self._value.__pos__())

    def __abs__(self):
        return Decimal(abs(self._value))

    def __round__(self, ndigits=None):
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
    cdef inline object _extract_value(object obj):
        if isinstance(obj, Decimal):
            return obj._value
        return obj

    @staticmethod
    cdef inline bint _compare(a, b, int op) except *:
        if isinstance(a, float) or isinstance(b, float):
            return NotImplemented
        if isinstance(a, Decimal):
            a = <Decimal>a._value
        if isinstance(b, Decimal):
            b = <Decimal>b._value

        return PyObject_RichCompareBool(a, b, op)

    @staticmethod
    cdef inline double _eval_double(double a, double b, int op) except *:
        if op == _MATH_ADD:
            return a + b
        elif op == _MATH_SUB:
            return a - b
        elif op == _MATH_MUL:
            return a * b
        elif op == _MATH_DIV:
            return a / b
        elif op == _MATH_TRUEDIV:
            return a / b
        elif op == _MATH_FLOORDIV:
            return a // b
        elif op == _MATH_MOD:
            return a % b
        else:
            return NotImplemented

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
