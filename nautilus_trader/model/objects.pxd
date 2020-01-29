# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol, InstrumentId


cdef class Quantity(Decimal):
    @staticmethod
    cdef Quantity zero()
    @staticmethod
    cdef Quantity from_string(str value)
    cpdef Quantity add(self, Decimal other)
    cpdef Quantity subtract(self, Decimal other)


cdef class Price(Decimal):
    @staticmethod
    cdef Price from_string(str value)
    cpdef Price add(self, Decimal other)
    cpdef Price subtract(self, Decimal other)


cdef class Money(Decimal):
    @staticmethod
    cdef Money zero()
    @staticmethod
    cdef Money from_string(str value)
    cpdef Money add(self, Decimal other)
    cpdef Money subtract(self, Decimal other)


cdef class Tick:
    cdef readonly Symbol symbol
    cdef readonly Price bid
    cdef readonly Price ask
    cdef readonly double bid_size
    cdef readonly double ask_size
    cdef readonly datetime timestamp

    @staticmethod
    cdef Tick from_string_with_symbol(Symbol symbol, str values)
    @staticmethod
    cdef Tick from_string(str value)
    cpdef str to_string(self)


cdef class BarSpecification:
    cdef readonly int step
    cdef readonly BarStructure structure
    cdef readonly PriceType price_type

    @staticmethod
    cdef BarSpecification from_string(str value)
    cdef str structure_string(self)
    cdef str price_type_string(self)
    cpdef bint equals(self, BarSpecification other)
    cpdef str to_string(self)


cdef class BarType:
    cdef readonly Symbol symbol
    cdef readonly BarSpecification specification

    @staticmethod
    cdef BarType from_string(str value)
    cdef str structure_string(self)
    cdef str price_type_string(self)
    cpdef bint equals(self, BarType other)
    cpdef str to_string(self)


cdef class Bar:
    cdef readonly Price open
    cdef readonly Price high
    cdef readonly Price low
    cdef readonly Price close
    cdef readonly double volume
    cdef readonly datetime timestamp
    cdef readonly bint checked

    @staticmethod
    cdef Bar from_string(str value)
    cpdef str to_string(self)


cdef class Instrument:
    cdef readonly InstrumentId id
    cdef readonly Symbol symbol
    cdef readonly str broker_symbol
    cdef readonly Currency quote_currency
    cdef readonly SecurityType security_type
    cdef readonly int tick_precision
    cdef readonly Decimal tick_size
    cdef readonly double round_lot_size
    cdef readonly int min_stop_distance_entry
    cdef readonly int min_stop_distance
    cdef readonly int min_limit_distance_entry
    cdef readonly int min_limit_distance
    cdef readonly double min_trade_size
    cdef readonly double max_trade_size
    cdef readonly Decimal rollover_interest_buy
    cdef readonly Decimal rollover_interest_sell
    cdef readonly datetime timestamp


cdef class ForexInstrument(Instrument):
    cdef readonly Currency base_currency
