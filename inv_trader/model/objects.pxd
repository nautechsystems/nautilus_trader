#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.enums.currency_code cimport CurrencyCode
from inv_trader.enums.security_type cimport SecurityType
from inv_trader.enums.venue cimport Venue
from inv_trader.enums.resolution cimport Resolution
from inv_trader.enums.quote_type cimport QuoteType


cdef class Symbol:
    """
    Represents the symbol for a financial market tradeable instrument.
    """
    cdef readonly str code
    cdef readonly Venue venue
    cdef str venue_string(self)
    cdef bint equals(self, Symbol other)


cdef class Price:
    """
    Represents a financial market price
    """
    cdef readonly object value
    cdef readonly int precision
    cpdef float as_float(self)


cdef class Tick:
    """
    Represents a single tick in a financial market.
    """
    cdef readonly Symbol symbol
    cdef readonly Price bid
    cdef readonly Price ask
    cdef readonly datetime timestamp


cdef class BarType:
    """
    Represents a financial market symbol and bar specification.
    """
    cdef readonly Symbol symbol
    cdef readonly int period
    cdef readonly Resolution resolution
    cdef readonly QuoteType quote_type
    cdef bint equals(self, BarType other)
    cdef str resolution_string(self)
    cdef str quote_type_string(self)


cdef class Bar:
    """
    Represents a financial market trade bar.
    """
    cdef readonly Price open
    cdef readonly Price high
    cdef readonly Price low
    cdef readonly Price close
    cdef readonly long volume
    cdef readonly datetime timestamp
    cdef readonly bint checked


cdef class DataBar:
    """
    Represents a financial market trade bar.
    """
    cdef readonly float open
    cdef readonly float high
    cdef readonly float low
    cdef readonly float close
    cdef readonly float volume
    cdef readonly datetime timestamp


cdef class Instrument:
    """
    Represents a tradeable financial market instrument.
    """
    cdef readonly Symbol symbol
    cdef readonly str broker_symbol
    cdef readonly CurrencyCode quote_currency
    cdef readonly SecurityType security_type
    cdef readonly int tick_precision
    cdef readonly object tick_size
    cdef readonly object tick_value
    cdef readonly object target_direct_spread
    cdef readonly int round_lot_size
    cdef readonly int contract_size
    cdef readonly int min_stop_distance_entry
    cdef readonly int min_limit_distance_entry
    cdef readonly int min_stop_distance
    cdef readonly int min_limit_distance
    cdef readonly int min_trade_size
    cdef readonly int max_trade_size
    cdef readonly object margin_requirement
    cdef readonly object rollover_interest_buy
    cdef readonly object rollover_interest_sell
    cdef readonly datetime timestamp
