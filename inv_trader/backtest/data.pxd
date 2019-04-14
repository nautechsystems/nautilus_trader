#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime, timedelta

from inv_trader.common.data cimport DataClient
from inv_trader.model.objects cimport Tick, BarType, Bar, Instrument


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for backtesting.
    """
    cdef readonly dict data_ticks
    cdef readonly dict data_bars_bid
    cdef readonly dict data_bars_ask
    cdef readonly list data_minute_index
    cdef readonly dict data_providers
    cdef readonly bint use_ticks

    cpdef void set_initial_iteration(self, datetime to_time, timedelta time_step)
    cpdef list iterate_ticks(self, datetime to_time)
    cpdef dict iterate_bars(self, datetime to_time)
    cpdef dict get_next_minute_bars(self, datetime time)
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
    cdef readonly BarType bar_type_min_bid
    cdef readonly BarType bar_type_min_ask
    cdef readonly list ticks
    cdef readonly dict bars
    cdef readonly dict iterations
    cdef readonly int tick_index
    cdef readonly bint has_ticks

    cpdef void register_ticks(self)
    cpdef void deregister_ticks(self)
    cpdef void build_minute_bars(self)
    cpdef void register_bars(self, BarType bar_type)
    cpdef void deregister_bars(self, BarType bar_type)
    cpdef void set_initial_iterations(self, datetime from_time, datetime to_time, timedelta time_step)
    cpdef bint is_next_minute_bars_at_time(self, datetime time)
    cpdef Bar get_next_minute_bid_bar(self, datetime time)
    cpdef Bar get_next_minute_ask_bar(self, datetime time)

    cpdef list iterate_ticks(self, datetime to_time)
    cpdef dict iterate_bars(self, datetime to_time)
    cpdef void reset(self)
