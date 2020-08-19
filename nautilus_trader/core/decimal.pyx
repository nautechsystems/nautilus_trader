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
from libc.math cimport isnan, isfinite, fabs, round

from nautilus_trader.core.correctness cimport Condition


cdef dict _EPSILON_MAP = {
    1: 1e-1,
    2: 1e-2,
    3: 1e-3,
    4: 1e-4,
    5: 1e-5,
    6: 1e-6,
    7: 1e-7,
    8: 1e-8,
    9: 1e-9,
    10: 1e-10,
}

cdef Decimal64 _ZERO_DECIMAL = Decimal64()

cdef class Decimal64:
    """
    Represents a decimal64 floating point value.

    The data type allows up to 16 digits of significand with up to 9 decimal
    places of precision. The input double values are rounded to nearest with the
    specified precision.
    """

    def __init__(self, double value=0.0, int precision=0):
        """
        Initialize a new instance of the Decimal64 class.

        Parameters
        ----------
        value : double
            The IEEE-754 double value for the decimal
            (must have no more than 16 significands).
        precision : int
            The precision for the decimal [0, 9].

        Raises
        ------
        ValueError
            If value is 'nan'.
            If value is 'inf.
            If value is '-inf'.
            If precision is not in range [0, 9].

        """
        Condition.true(not isnan(value), "value is not nan")
        Condition.true(isfinite(value), "value is finite")
        Condition.in_range_int(precision, 0, 9, "precision")

        cdef int power = 10 ** precision             # For zero precision then zero power rule 10^0 = 1
        self._value = round(value * power) / power   # Rounding to nearest (bankers rounding)
        self._epsilon = _EPSILON_MAP[precision + 1]  # Choose epsilon for one digit more than precision
        self.precision = precision

    cdef bint _eq_eps_delta(self, double value1, double value2):
        # The values are considered equal if their absolute difference is less
        # than epsilon
        return fabs(value1 - value2) < self._epsilon

    cdef bint _ne_eps_delta(self, double value1, double value2):
        # The values are considered NOT equal if their absolute difference is
        # greater than OR equal to epsilon
        return fabs(value1 - value2) >= self._epsilon

    @staticmethod
    cdef Decimal64 zero():
        """
        Return a zero valued decimal.

        Returns
        -------
        Decimal64
            The value and precision will be zero.
        """
        return _ZERO_DECIMAL

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

        return Decimal64(float(value), precision=Decimal64.precision_from_string(value))

    @staticmethod
    cdef int precision_from_string(str value):
        """
        Return the decimal precision inferred from the number of digits after
        the '.' decimal place.

        Note: If no decimal place then precision will be zero.

        Parameters
        ----------
        value : str
            The string value to parse.

        Returns
        -------
        int

        """
        Condition.valid_string(value, "value")

        return len(value.partition('.')[2])  # If does not contain "." then partition will be ""

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
        return self._value

    cpdef object as_decimal(self):
        """
        Return the internal value as a built-in decimal.

        Returns
        -------
        decimal.Decimal

        """
        return decimal.Decimal(f"{self._value:.{self.precision}f}")

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
            if self.precision == 0:
                return f"{int(self._value):,}"
            else:
                return f"{self._value:,.{self.precision}f}"
        else:
            if self.precision == 0:
                return f"{int(self._value)}"
            else:
                return f"{self._value:.{self.precision}f}"

    cpdef bint eq(self, Decimal64 other):
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
        return self._eq_eps_delta(self._value, other._value)

    cpdef bint ne(self, Decimal64 other):
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
        return self._ne_eps_delta(self._value, other._value)

    cpdef bint lt(self, Decimal64 other):
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
        return self._ne_eps_delta(self._value, other._value) and self._value < other._value

    cpdef bint le(self, Decimal64 other):
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
        return self._eq_eps_delta(self._value, other._value) or self._value < other._value

    cpdef bint gt(self, Decimal64 other):
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
        return self._ne_eps_delta(self._value, other._value) and self._value > other._value

    cpdef bint ge(self, Decimal64 other):
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
        return self._eq_eps_delta(self._value, other._value) or self._value > other._value

    cpdef Decimal64 add_as_decimal(self, Decimal64 other, bint keep_precision=False):
        """
        Return a new decimal by adding the given decimal to this decimal.

        Parameters
        ----------
        other : Decimal64
            The other decimal to add.
        keep_precision : bool
            If the original precision should be maintained.

        Returns
        -------
        Decimal64

        """
        if keep_precision:
            # noinspection PyProtectedMember
            # direct access to protected member ok here
            return Decimal64(self._value + other._value, self.precision)
        else:
            # noinspection PyProtectedMember
            # direct access to protected member ok here
            return Decimal64(self._value + other._value, max(self.precision, other.precision))

    cpdef Decimal64 sub_as_decimal(self, Decimal64 other, bint keep_precision=False):
        """
        Return a new decimal by subtracting the given decimal from this decimal.

        Parameters
        ----------
        other : Decimal64
            The other decimal to subtract.
        keep_precision : bool
            If the original precision should be maintained.

        Returns
        -------
        Decimal64

        """
        if keep_precision:
            # noinspection PyProtectedMember
            # direct access to protected member ok here
            return Decimal64(self._value - other._value, self.precision)
        else:
            # noinspection PyProtectedMember
            # direct access to protected member ok here
            return Decimal64(self._value - other._value, max(self.precision, other.precision))

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
        try:
            return self._eq_eps_delta(self.as_double(), <double?>other)
        except TypeError:
            # User passed a Decimal64 rather than double
            return self._eq_eps_delta(self.as_double(), other.as_double())

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
        try:
            self._ne_eps_delta(self.as_double(), <double?>other)
        except TypeError:
            # User passed a Decimal64 rather than double
            return self._ne_eps_delta(self.as_double(), other.as_double())

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
        try:
            return self._ne_eps_delta(self.as_double(), other) and self.as_double() < <double?>other
        except TypeError:
            # User passed a Decimal64 rather than double
            return self._ne_eps_delta(self.as_double(), other.as_double()) and self.as_double() < other.as_double()

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
        try:
            return self._eq_eps_delta(self.as_double(), other) or self.as_double() < <double?>other
        except TypeError:
            # User passed a Decimal64 rather than double
            return self._eq_eps_delta(self.as_double(), other.as_double()) or self.as_double() < other.as_double()

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
        try:
            return self._ne_eps_delta(self.as_double(), other) and self.as_double() > <double?>other
        except TypeError:
            # User passed a Decimal64 rather than double
            return self._ne_eps_delta(self.as_double(), other.as_double()) and self.as_double() > other.as_double()

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
        try:
            return self._eq_eps_delta(self.as_double(), other) or self.as_double() > <double?>other
        except TypeError:
            # User passed a Decimal64 rather than double
            return self._eq_eps_delta(self.as_double(), other.as_double()) or self.as_double() > other.as_double()

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
        try:
            return self.as_double() + <double?>other
        except TypeError:
            # User passed a Decimal64 rather than double
            return self.as_double() + other.as_double()

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
        try:
            return self.as_double() - <double?>other
        except TypeError:
            # User passed a Decimal64 rather than double
            return self.as_double() - other.as_double()

    def __truediv__(self, other) -> float:
        """
        Return the result of dividing this object by the given object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        float

        """
        try:
            return self.as_double() / <double?>other
        except TypeError:
            # User passed a Decimal64 rather than double
            return self.as_double() / other.as_double()

    def __mul__(self, other) -> float:
        """
        Return the result of multiplying this object by the given object.

        Parameters
        ----------
        other : object
            The other object.

        Returns
        -------
        float

        """
        try:
            return self.as_double() * <double?>other
        except TypeError:
            # User passed a Decimal64 rather than double
            return self.as_double() * other.as_double()

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
        return (f"<{self.__class__.__name__}({self.to_string()}, "
                f"precision={self.precision}) object at {id(self)}>")
