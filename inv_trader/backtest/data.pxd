#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime, timedelta

from inv_trader.common.data cimport DataClient
from inv_trader.model.objects cimport Symbol, BarType, Instrument


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for the BacktestEngine.
    """
    cdef readonly dict tick_data
    cdef readonly dict bar_data_bid
    cdef readonly dict bar_data_ask
    cdef readonly list minute_data_index
    cdef readonly int iteration
    cdef readonly dict data_providers

    cpdef void set_initial_iteration(self, datetime to_time, timedelta time_step)
    cpdef void iterate(self, datetime time)
    cpdef void subscribe_bars(self, BarType bar_type, handler)
    cpdef void unsubscribe_bars(self, BarType bar_type, handler)
    cpdef void subscribe_ticks(self, Symbol symbol, handler)
    cpdef void unsubscribe_ticks(self, Symbol symbol, handler)


cdef class DataProvider:
    """
    Provides data for the BacktestDataClient.
    """
    cdef readonly Instrument instrument
    cdef readonly dict iterations
    cdef readonly dict _bar_data_bid
    cdef readonly dict _bar_data_ask
    cdef readonly dict _bars

    cpdef void register_bar_type(self, BarType bar_type)
    cpdef void deregister_bar_type(self, BarType bar_type)
    cpdef void set_initial_iterations(self, datetime from_time, datetime to_time, timedelta time_step)
    cpdef dict iterate_bars(self, datetime time)
