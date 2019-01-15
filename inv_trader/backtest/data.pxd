#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime

from inv_trader.common.data cimport DataClient
from inv_trader.model.objects cimport Symbol, BarType, Instrument


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for the BacktestEngine.
    """
    cdef readonly dict tick_data
    cdef readonly dict bar_data_bid
    cdef readonly dict bar_data_ask
    cdef readonly int iteration
    cdef dict data_providers

    cpdef void subscribe_bars(self, BarType bar_type, handler)
    cpdef void unsubscribe_bars(self, BarType bar_type, handler)
    cpdef void subscribe_ticks(self, Symbol symbol, handler)
    cpdef void unsubscribe_ticks(self, Symbol symbol, handler)
    cpdef void iterate(self, datetime time)


cdef class DataProvider:
    """
    Provides data for the BacktestDataClient.
    """
    cdef readonly Instrument instrument
    cdef readonly dict iterations
    cdef dict _bar_data_bid
    cdef dict _bar_data_ask
    cdef dict _bars

    cpdef void register_bar_type(self, BarType bar_type)
    cpdef void deregister_bar_type(self, BarType bar_type)
    cpdef dict iterate_bars(self, datetime time)
