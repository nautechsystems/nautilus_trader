#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="tools.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime

from inv_trader.model.objects cimport Bar, DataBar


cdef class BarBuilder:
    """
    Provides a means of building lists of bars from a given Pandas DataFrame of
    the correct specification.
    """
    cdef object _data
    cdef int _decimal_precision
    cdef int _volume_multiple

    cpdef list build_databars_all(self)
    cpdef list build_databars_from(self, int index=*)
    cpdef list build_databars_range(self, int start=*, int end=*)
    cpdef list build_bars_all(self)
    cpdef list build_bars_from(self, int index=*)
    cpdef list build_bars_range(self, int start=*, int end=*)
    cpdef DataBar _build_databar(self, double[:] values, datetime timestamp)
    cpdef Bar _build_bar(self, double[:] values, datetime timestamp)


cdef class IndicatorUpdater:
    """
    Provides an adapter for updating an indicator with a bar. When instantiated
    with a live indicator update method, the updater will inspect the method and
    construct the required parameter list for updates.
    """
    cdef object _indicator
    cdef object _input_method
    cdef list _input_params
    cdef list _outputs

    cpdef void update_bar(self, Bar bar)
    cpdef void update_databar(self, DataBar bar)
    cpdef dict build_features(self, list bars)
    cpdef dict build_features_databars(self, list bars)
    cdef list _get_values(self)
