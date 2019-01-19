#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="money.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from inv_trader.core.decimal cimport Decimal
from inv_trader.core.precondition cimport Precondition


cpdef Decimal money_zero():
    """
    :return: A decimal representing money with value zero and precision 2.
    """
    return Decimal(0, 2)


cpdef Decimal money(float amount):
    """
    Creates and returns money from the given values.
    The money is rounded to two decimal digits.

    :param amount: The money amount.
    :return: A Decimal representing the money.
    :raises ValueError: If the amount is negative (< 0).
    """
    Precondition.not_negative(amount, 'amount')

    return Decimal(amount, 2)
