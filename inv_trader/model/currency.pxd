#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="currency.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


from inv_trader.enums.currency_code cimport CurrencyCode
from inv_trader.enums.quote_type cimport QuoteType


cdef class CurrencyConverter:
    """
    Provides exchange rate calculations between currencies.
    """

    cpdef float exchange_rate(
            self,
            CurrencyCode from_currency,
            CurrencyCode to_currency,
            QuoteType quote_type,
            dict bid_rates,
            dict ask_rates)