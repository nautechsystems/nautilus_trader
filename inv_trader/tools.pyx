#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="tools.pyx" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import cython
import inspect

from typing import Callable
from pandas.core.frame import DataFrame

from inv_trader.model.objects import DataBar


POINT = 'point'
PRICE = 'price'
MID = 'mid'
OPEN = 'open'
HIGH = 'high'
LOW = 'low'
CLOSE = 'close'
VOLUME = 'volume'
TIMESTAMP = 'timestamp'

cdef class BarBuilder:
    """
    Provides a means of building a bar from a given Pandas Series row of the
    correct specification.
    """
    cdef object _data
    cdef int _volume_multiple

    def __init__(self, data: DataFrame, volume_multiple: int):
        """
        Initializes a new instance of the BarBuilder class.

        :param volume_multiple: The volume multiple for the builder.
        """
        self._data = data
        self._volume_multiple = volume_multiple

    @cython.boundscheck(False)
    @cython.wraparound(False)
    @cython.binding(True)
    cdef object deconstruct_row(self, object row):
        return DataBar(row[1][0],
                       row[1][1],
                       row[1][2],
                       row[1][3],
                       row[1][4] * self._volume_multiple,
                       row[0])

    cpdef object build_bars_apply(self):
        """
        Build a bar from the held Pandas DataFrame.
        
        :return: The bars.
        """
        return self._data.apply(self.deconstruct_row, axis=1)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    @cython.binding(True)
    cpdef object build_bars_iter(self):
        """
        Build a bar from the held Pandas DataFrame.
        
        :return: The bars.
        """
        bars = []
        for row in self._data.iterrows():
            bars.append(self.deconstruct_row(row))

        return bars


cdef class IndicatorUpdater:
    """
    Provides an adapter for updating an indicator with a bar. When instantiated
    with a live indicator update method, the updater will inspect the method and
    construct the required parameter list for updates.
    """
    cdef object _update_method
    cdef object _update_params

    def __init__(self, update_method: Callable):
        """
        Initializes a new instance of the IndicatorUpdater class.

        :param update_method: The indicators update method.
        """
        self._update_method = update_method
        self._update_params = []

        param_map = {
            POINT: CLOSE,
            PRICE: CLOSE,
            MID: CLOSE,
            OPEN: OPEN,
            HIGH: HIGH,
            LOW: LOW,
            CLOSE: CLOSE,
            TIMESTAMP: TIMESTAMP
        }

        for param in inspect.signature(update_method).parameters:
            self._update_params.append(param_map[param])

    cpdef update(self, object bar):
        """
        Passes the needed values from the given bar to the indicator update
        method as a list of arguments.

        :param bar: The update bar.
        """
        args = [bar.__getattribute__(param) for param in self._update_params]
        self._update_method(*args)
