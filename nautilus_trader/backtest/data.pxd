# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.data cimport DataClient
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.objects cimport Tick, Instrument
from nautilus_trader.model.identifiers cimport Symbol


cdef class BacktestDataContainer:
    cdef readonly set symbols
    cdef readonly dict instruments
    cdef readonly dict ticks
    cdef readonly dict bars_bid
    cdef readonly dict bars_ask

    cpdef void add_instrument(self, Instrument instrument) except *
    cpdef void add_ticks(self, Symbol symbol, data) except *
    cpdef void add_bars(self, Symbol symbol, BarStructure structure, PriceType price_type, data) except *
    cpdef void check_integrity(self) except *
    cpdef long total_data_size(self)


cdef class BacktestDataClient(DataClient):
    cdef BacktestDataContainer _data
    cdef object _tick_data
    cdef unsigned short[:] _symbols
    cdef double[:, :] _price_volume
    cdef datetime[:] _timestamps
    cdef dict _symbol_index
    cdef dict _price_precisions
    cdef dict _size_precisions
    cdef int _index
    cdef int _index_last

    cdef readonly list execution_resolutions
    cdef readonly datetime min_timestamp
    cdef readonly datetime max_timestamp
    cdef readonly bint has_data

    cpdef void setup(self, datetime start, datetime stop) except *
    cdef Tick generate_tick(self)

    cpdef void process_tick(self, Tick tick) except *
    cpdef void reset(self) except *
