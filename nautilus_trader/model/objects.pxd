# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.resolution cimport Resolution
from nautilus_trader.model.c_enums.quote_type cimport QuoteType
from nautilus_trader.model.identifiers cimport Symbol, InstrumentId


cdef class Quantity:
    cdef readonly long value

    cdef bint equals(self, Quantity other)


cdef class Price:
    cdef readonly object value
    cdef readonly int precision

    cdef bint equals(self, Price other)
    cpdef Price add(self, Price price)
    cpdef Price subtract(self, Price price)
    cpdef float as_float(self)


cdef class Money:
    cdef readonly object value

    cdef bint equals(self, Money other)
    cpdef float as_float(self)


cdef class Tick:
    cdef readonly Symbol symbol
    cdef readonly Price bid
    cdef readonly Price ask
    cdef readonly datetime timestamp

    @staticmethod
    cdef Tick from_string_with_symbol(Symbol symbol, str values)
    @staticmethod
    cdef Tick from_string(str value)


cdef class BarSpecification:
    cdef readonly int period
    cdef readonly Resolution resolution
    cdef readonly QuoteType quote_type

    cpdef timedelta timedelta(self)
    cdef bint equals(self, BarSpecification other)
    cdef str resolution_string(self)
    cdef str quote_type_string(self)
    @staticmethod
    cdef BarSpecification from_string(str value)


cdef class BarType:
    cdef readonly Symbol symbol
    cdef readonly BarSpecification specification

    cdef bint equals(self, BarType other)
    cdef str resolution_string(self)
    cdef str quote_type_string(self)

    @staticmethod
    cdef BarType from_string(str value)


cdef class Bar:
    cdef readonly Price open
    cdef readonly Price high
    cdef readonly Price low
    cdef readonly Price close
    cdef readonly long volume
    cdef readonly datetime timestamp
    cdef readonly bint checked

    @staticmethod
    cdef Bar from_string(str value)


cdef class DataBar:
    cdef readonly float open
    cdef readonly float high
    cdef readonly float low
    cdef readonly float close
    cdef readonly float volume
    cdef readonly datetime timestamp


cdef class Instrument:
    cdef readonly InstrumentId id
    cdef readonly Symbol symbol
    cdef readonly str broker_symbol
    cdef readonly Currency quote_currency
    cdef readonly SecurityType security_type
    cdef readonly int tick_precision
    cdef readonly object tick_size
    cdef readonly Quantity round_lot_size
    cdef readonly int min_stop_distance_entry
    cdef readonly int min_stop_distance
    cdef readonly int min_limit_distance_entry
    cdef readonly int min_limit_distance
    cdef readonly Quantity min_trade_size
    cdef readonly Quantity max_trade_size
    cdef readonly object rollover_interest_buy
    cdef readonly object rollover_interest_sell
    cdef readonly datetime timestamp
