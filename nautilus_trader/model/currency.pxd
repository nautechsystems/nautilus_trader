# -------------------------------------------------------------------------------------------------
# <copyright file="currency.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.quote_type cimport QuoteType


cdef class ExchangeRateCalculator:
    cpdef float get_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            QuoteType quote_type,
            dict bid_rates,
            dict ask_rates) except *
