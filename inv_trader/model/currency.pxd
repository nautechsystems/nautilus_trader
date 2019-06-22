#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="currency.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


from inv_trader.c_enums.currency cimport Currency
from inv_trader.c_enums.quote_type cimport QuoteType


cdef class ExchangeRateCalculator:
    """
    Provides exchange rates between currencies.
    """

    cpdef float get_rate(
            self,
            Currency quote_currency,
            Currency base_currency,
            QuoteType quote_type,
            dict bid_rates,
            dict ask_rates)
