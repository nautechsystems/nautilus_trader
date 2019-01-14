#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from inv_trader.common.data cimport DataClient
from inv_trader.model.objects cimport Instrument, BarType


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for the BacktestEngine.
    """
    cdef int _iteration
    cdef dict data_providers

    cdef void iterate(self)
    cdef void _iterate_bar_type(self, BarType bar_type)


cdef class DataProvider:
    """
    Provides data for the BacktestDataClient.
    """
    cdef readonly Instrument instrument
    cdef readonly dict bid_data
    cdef readonly dict ask_data
    cdef dict _bar_builders

    cpdef void register_bar_type(self, BarType bar_type)
