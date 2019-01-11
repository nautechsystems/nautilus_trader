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
from inv_trader.model.objects cimport BarType


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for the BacktestEngine.
    """
    cdef int _iteration
    cdef dict bar_builders

    cdef void iterate(self)
    cdef void _iterate_bar_type(self, BarType bar_type)
