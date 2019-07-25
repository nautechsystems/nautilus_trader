# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.common.data cimport DataClient
from nautilus_trader.model.c_enums.resolution cimport Resolution
from nautilus_trader.model.objects cimport Symbol, Tick, BarType, Bar, Instrument


cdef class BidAskBarPair:
    """
    Represents a bid ask bar pair for the same market and timestamp.
    """
    cdef readonly Bar bid
    cdef readonly Bar ask


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for backtesting.
    """
    cdef readonly dict data_ticks
    cdef readonly dict data_bars_bid
    cdef readonly dict data_bars_ask
    cdef readonly dict data_providers
    cdef readonly set data_symbols
    cdef readonly datetime execution_data_index_min
    cdef readonly datetime execution_data_index_max
    cdef readonly Resolution execution_resolution
    cdef readonly timedelta max_time_step

    cdef void _setup_execution_data(self)
    cdef bint _check_ticks_exist(self)
    cdef bint _check_bar_resolution_exists(self, Resolution resolution)
    cdef void _set_execution_data_index(self, Symbol symbol, datetime first, datetime last)
    cdef void _build_bars(self, BarType bar_type)
    cpdef void set_initial_iteration_indexes(self, datetime to_time)
    cpdef list iterate_ticks(self, datetime to_time)
    cpdef dict iterate_bars(self, datetime to_time)
    cpdef dict get_next_execution_bars(self, datetime time)
    cpdef void process_tick(self, Tick tick)
    cpdef void process_bars(self, dict bars)
    cpdef void reset(self)


cdef class DataProvider:
    """
    Provides data for a particular instrument for the BacktestDataClient.
    """
    cdef readonly Instrument instrument
    cdef readonly object _dataframe_ticks
    cdef readonly dict _dataframes_bars_bid
    cdef readonly dict _dataframes_bars_ask
    cdef readonly BarType bar_type_sec_bid
    cdef readonly BarType bar_type_sec_ask
    cdef readonly BarType bar_type_min_bid
    cdef readonly BarType bar_type_min_ask
    cdef readonly BarType bar_type_hour_bid
    cdef readonly BarType bar_type_hour_ask
    cdef readonly BarType bar_type_execution_bid
    cdef readonly BarType bar_type_execution_ask
    cdef readonly list ticks
    cdef readonly dict bars
    cdef readonly dict iterations
    cdef readonly int tick_index

    cpdef void register_ticks(self)
    cpdef void deregister_ticks(self)
    cpdef void register_bars(self, BarType bar_type)
    cpdef void deregister_bars(self, BarType bar_type)
    cpdef void set_execution_bar_res(self, Resolution resolution)
    cpdef void set_initial_iteration_indexes(self, datetime to_time)
    cpdef void set_tick_iteration_index(self, datetime to_time)
    cpdef void set_bar_iteration_index(self, BarType bar_type, datetime to_time)
    cpdef bint is_next_exec_bars_at_time(self, datetime time)
    cpdef Bar get_next_exec_bid_bar(self)
    cpdef Bar get_next_exec_ask_bar(self)

    cpdef list iterate_ticks(self, datetime to_time)
    cpdef dict iterate_bars(self, datetime to_time)
    cpdef void reset(self)
