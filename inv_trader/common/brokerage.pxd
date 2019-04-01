#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="brokerage.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.model.objects cimport Symbol, Money, Quantity, Price


cdef class CommissionCalculator:
    """
    Provides a means of calculating commissions.
    """
    cdef dict rates
    cdef float default_rate_bp
    cdef Money minimum

    cpdef Money calculate(self, Symbol symbol, Quantity filled_quantity, Price filled_price, float exchange_rate)
    cpdef Money calculate_for_notional(self, Symbol symbol, Money notional_value)

    cdef float _get_commission_rate(self, Symbol symbol)
