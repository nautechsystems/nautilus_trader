# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.tick_type cimport TickType
from nautilus_trader.model.identifiers cimport Symbol, InstrumentId


cdef class Quantity:
    cdef readonly long value

    @staticmethod
    cdef Quantity zero()
    cdef bint equals(self, Quantity other)
    cdef str to_string_formatted(self)


cdef class Decimal:
    cdef readonly object value
    cdef readonly int precision

    cpdef float as_float(self)
    cpdef str to_string(self, bint format_commas=*)
    @staticmethod
    cdef Decimal zero()
    @staticmethod
    cdef Decimal from_string_to_decimal(str value)
    @staticmethod
    cdef int precision_from_string(str value)
    cdef bint equals(self, Decimal other)
    cpdef bint eq(self, Decimal other)
    cpdef bint ne(self, Decimal other)
    cpdef bint lt(self, Decimal other)
    cpdef bint le(self, Decimal other)
    cpdef bint gt(self, Decimal other)
    cpdef bint ge(self, Decimal other)
    cpdef Decimal add(self, Decimal other)
    cpdef Decimal subtract(self, Decimal other)


cdef class Price(Decimal):
    @staticmethod
    cdef Price from_string(str value)


cdef class Money(Decimal):
    @staticmethod
    cdef Money zero()
    @staticmethod
    cdef Money from_string(str value)


cdef class Tick:
    cdef readonly TickType type
    cdef readonly Symbol symbol
    cdef readonly Price bid
    cdef readonly Price ask
    cdef readonly int bid_size
    cdef readonly int ask_size
    cdef readonly datetime timestamp

    @staticmethod
    cdef Tick from_string_with_symbol(Symbol symbol, str values)
    @staticmethod
    cdef Tick from_string(str value)


cdef class BarSpecification:
    cdef readonly int step
    cdef readonly BarStructure structure
    cdef readonly PriceType price_type

    cdef bint equals(self, BarSpecification other)
    cdef str structure_string(self)
    cdef str quote_type_string(self)
    @staticmethod
    cdef BarSpecification from_string(str value)


cdef class BarType:
    cdef readonly Symbol symbol
    cdef readonly BarSpecification specification

    cdef bint equals(self, BarType other)
    cdef str structure_string(self)
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
    cdef readonly Currency base_currency
    cdef readonly SecurityType security_type
    cdef readonly int tick_precision
    cdef readonly Decimal tick_size
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
