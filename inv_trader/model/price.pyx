#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="price.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False


from decimal import Decimal, getcontext
from inv_trader.core.precondition cimport Precondition


cpdef object price(float value, int precision):
    """
    Creates and returns a new price from the given values.
    The price is rounded to the given decimal precision.

    :param value: The price value (> 0).
    :param precision: The decimal precision of the price (> 0).
    :return: A Decimal representing the price.
    :raises ValueError: If the price is not positive (> 0).
    :raises ValueError: If the precision is negative (< 0).
    """
    Precondition.positive(value, 'value')
    Precondition.positive(precision, 'precision')

    getcontext().prec = precision
    return Decimal(value)
