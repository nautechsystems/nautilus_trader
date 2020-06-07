# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from libc.math cimport round, pow


cpdef double fast_round(double value, int precision):
    """
    Return the given value rounded to the nearest precision digits.
    
    :param value: The value to round.
    :param precision: The precision to round to.
    :return: double.
    """
    cdef int power = 10 ** precision
    return round(value * power) / power


cpdef double fast_round2(double value, int precision):
    """
    Return the given value rounded to the nearest precision digits.
    
    :param value: The value to round.
    :param precision: The precision to round to.
    :return: double.
    """
    cdef double power = pow(10, precision)
    return round(value * power) / power
