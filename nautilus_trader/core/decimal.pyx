# -------------------------------------------------------------------------------------------------
# <copyright file="decimal.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import decimal

from libc.math cimport round
from cpython.unicode cimport PyUnicode_Contains

from nautilus_trader.core.correctness cimport Condition


cdef Decimal _ZERO_DECIMAL = Decimal()

cdef class Decimal:
    """
    Represents a decimal floating point value type with fixed precision.
    """

    def __init__(self, double value=0.0, int precision=1):
        """
        Initializes a new instance of the Decimal class.

        :param value: The value of the decimal.
        :param precision: The precision of the decimal (> 0).
        :raises ValueError: If the precision is not positive (> 0).
        """
        Condition.positive_int(precision, 'precision')

        self._power = 10 ** precision
        self._value = round(value * self._power) / self._power  # Rounding to nearest
        self.precision = precision

    @staticmethod
    cdef Decimal zero():
        """
        Return a zero valued decimal.
        
        :return Decimal.
        """
        return _ZERO_DECIMAL

    @staticmethod
    cdef Decimal from_string_to_decimal(str value):
        """
        Return a decimal from the given string. Precision will be inferred from the
        number of digits after the decimal place.

        :param value: The string value to parse.
        :return: Decimal.
        """
        Condition.valid_string(value, 'value')

        return Decimal(float(value), precision=Decimal.precision_from_string(value))

    @staticmethod
    cdef int precision_from_string(str value) except -1:
        """
        Return the decimal precision inferred from the number of digits after the decimal place.

        :param value: The string value to parse.
        :return: int.
        """
        Condition.valid_string(value, 'value')

        try:
            if PyUnicode_Contains(value, '.'):
                return len(value.partition('.')[2])
            else:
                return 1
        except IndexError:
            return -1

    cpdef double as_double(self):
        """
        Return the internal value as a floating point number.

        :return double.
        """
        return self._value

    cpdef object as_decimal(self):
        """
        Return the internal value as a built-in decimal.

        :return decimal.Decimal.
        """
        return decimal.Decimal(f'{self._value:.{self.precision}f}')

    cpdef bint equals(self, Decimal other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value == other._value

    cpdef str to_string(self, bint format_commas=False):
        """
        Return the formatted string representation of this object.
        
        :param format_commas: If the string should be formatted with commas separating thousands.
        :return: str.
        """
        if format_commas:
            return f'{self._value:,.{self.precision}f}'
        else:
            return f'{self._value:.{self.precision}f}'

    cpdef bint eq(self, Decimal other):
        """
        Return a value indicating whether this decimal is equal to (==) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value == other._value

    cpdef bint ne(self, Decimal other):
        """
        Return a value indicating whether this decimal is not equal to (!=) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value != other._value

    cpdef bint lt(self, Decimal other):
        """
        Return a value indicating whether this decimal is less than (<) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value < other._value

    cpdef bint le(self, Decimal other):
        """
        Return a value indicating whether this decimal is less than or equal to (<=) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value <= other._value

    cpdef bint gt(self, Decimal other):
        """
        Return a value indicating whether this decimal is greater than (>) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value > other._value

    cpdef bint ge(self, Decimal other):
        """
        Return a value indicating whether this decimal is greater than or equal to (>=) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        # noinspection PyProtectedMember
        # direct access to protected member ok here
        return self._value >= other._value

    cpdef Decimal add_decimal(self, Decimal other, bint keep_precision=False):
        """
        Return a new decimal by adding the given decimal to this decimal.

        :param other: The other decimal to add.
        :param keep_precision: If the original precision should be maintained.
        :return Decimal.
        """
        if keep_precision:
            # noinspection PyProtectedMember
            # direct access to protected member ok here
            return Decimal(self._value + other._value, self.precision)
        else:
            # noinspection PyProtectedMember
            # direct access to protected member ok here
            return Decimal(self._value + other._value, max(self.precision, other.precision))

    cpdef Decimal subtract_decimal(self, Decimal other, bint keep_precision=False):
        """
        Return a new decimal by subtracting the given decimal from this decimal.

        :param other: The other decimal to subtract.
        :param keep_precision: If the original precision should be maintained.
        :return Decimal.
        """
        if keep_precision:
            # noinspection PyProtectedMember
            # direct access to protected member ok here
            return Decimal(self._value - other._value, self.precision)
        else:
            # noinspection PyProtectedMember
            # direct access to protected member ok here
            return Decimal(self._value - other._value, max(self.precision, other.precision))

    def __eq__(self, other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.as_double() == <double?>other
        except TypeError:
            return self.as_double() == other.as_double()

    def __ne__(self, other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.as_double() != <double?>other
        except TypeError:
            return self.as_double() != other.as_double()

    def __lt__(self, other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.as_double() < <double?>other
        except TypeError:
            return self.as_double() < other.as_double()

    def __le__(self, other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.as_double() <= <double?>other
        except TypeError:
            return self.as_double() <= other.as_double()

    def __gt__(self, other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.as_double() > <double?>other
        except TypeError:
            return self.as_double() > other.as_double()

    def __ge__(self, other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.as_double() >= <double?>other
        except TypeError:
            return self.as_double() >= other.as_double()

    def __add__(self, other) -> double:
        """
        Return the result of adding the given object to this object.

        :param other: The other object.
        :return double.
        """
        try:
            return self.as_double() + <double?>other
        except TypeError:
            return self.as_double() + other.as_double()

    def __sub__(self, other) -> double:
        """
        Return the result of subtracting the given object from this object.

        :param other: The other object.
        :return double.
        """
        try:
            return self.as_double() - <double?>other
        except TypeError:
            return self.as_double() - other.as_double()

    def __truediv__(self, other) -> double:
        """
        Return the result of dividing this object by the given object.

        :param other: The other object.
        :return double.
        """
        try:
            return self.as_double() / <double?>other
        except TypeError:
            return self.as_double() / other.as_double()

    def __mul__(self, other) -> double:
        """
        Return the result of multiplying this object by the given object.

        :param other: The other object.
        :return double.
        """
        try:
            return self.as_double() * <double?>other
        except TypeError:
            return self.as_double() * other.as_double()

    def __hash__(self) -> int:
        """"
         Return the hash code of this object.

        :return int.
        """
        return hash(self._value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}, precision={self.precision}) object at {id(self)}>"
