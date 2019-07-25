#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="currency.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.quote_type cimport QuoteType


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
