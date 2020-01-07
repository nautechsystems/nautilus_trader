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
    cdef readonly dict instruments
    cdef readonly dict ticks
    cdef readonly dict bars_bid
    cdef readonly dict bars_ask

    cpdef void add_instrument(self, Instrument instrument)
    cpdef void add_ticks(self, Symbol symbol, data)
    cpdef void add_bars(self, Symbol symbol, BarStructure structure, PriceType price_type, data)
    cpdef void check_integrity(self)


cdef class BacktestDataClient(DataClient):
    cdef readonly list ticks
    cdef readonly list execution_resolutions
    cdef readonly datetime min_timestamp
    cdef readonly datetime max_timestamp

    cpdef void process_tick(self, Tick tick)
    cpdef void reset(self)
