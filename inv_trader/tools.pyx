#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="tools.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

import inspect
import pandas as pd

from cpython.datetime cimport datetime
from numpy import ndarray
from typing import Callable, List
from pandas.core.frame import DataFrame

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.objects import Price
from inv_trader.model.objects cimport Bar, DataBar


cdef str POINT = 'point'
cdef str PRICE = 'price'
cdef str MID = 'mid'
cdef str OPEN = 'open'
cdef str HIGH = 'high'
cdef str LOW = 'low'
cdef str CLOSE = 'close'
cdef str VOLUME = 'volume'
cdef str TIMESTAMP = 'timestamp'


cdef class BarBuilder:
    """
    Provides a means of building lists of bars from a given Pandas DataFrame of
    the correct specification.
    """
    cdef object _data
    cdef int _decimal_precision
    cdef int _volume_multiple

    def __init__(self,
                 data: DataFrame,
                 int decimal_precision=5,
                 int volume_multiple=1):
        """
        Initializes a new instance of the BarBuilder class.

        :param data: The DataFrame containing the market data.
        :param decimal_precision: The decimal precision for bar prices.
        :param volume_multiple: The volume multiple for the builder (> 0).
        """
        Precondition.type(data, DataFrame, 'data')
        Precondition.not_negative(decimal_precision, 'decimal_precision')
        Precondition.positive(volume_multiple, 'volume_multiple')

        self._data = data
        self._decimal_precision = decimal_precision
        self._volume_multiple = volume_multiple

    cpdef list build_data_bars(self):
        """
        Build a list of DataBars from the held Pandas DataFrame.
        
        :return: The list of bars.
        """
        return list(map(self._build_data_bar,
                        self._data.values,
                        pd.to_datetime(self._data.index)))

    cpdef list build_bars(self):
        """
        Build a list of Bars from the held Pandas DataFrame.

        :return: The list of bars.
        """
        return list(map(self._build_bar,
                        self._data.values,
                        pd.to_datetime(self._data.index)))

    cpdef DataBar _build_data_bar(
            self,
            double[:] values: ndarray,
            datetime timestamp):
        """
        Build a DataBar from the given index and values. The function expects the
        values to be an ndarray with 5 elements [open, high, low, close, volume].

        :param values: The values for the bar.
        :param timestamp: The timestamp for the bar.
        :return: The built DataBar.
        """
        return DataBar(values[0],
                       values[1],
                       values[2],
                       values[3],
                       values[4] * self._volume_multiple,
                       timestamp)

    cpdef Bar _build_bar(
            self,
            double[:] values: ndarray,
            datetime timestamp):
        """
        Build a Bar from the given index and values. The function expects the
        values to be an ndarray with 5 elements [open, high, low, close, volume].

        :param values: The values for the bar.
        :param timestamp: The timestamp for the bar.
        :return: The built Bar.
        """
        return Bar(Price.create(values[0], self._decimal_precision),
                   Price.create(values[1], self._decimal_precision),
                   Price.create(values[2], self._decimal_precision),
                   Price.create(values[3], self._decimal_precision),
                   int(values[4] * self._volume_multiple),
                   timestamp)


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

    def __init__(self,
                 indicator,
                 input_method: Callable=None,
                 list outputs: List[str]=None):
        """
        Initializes a new instance of the IndicatorUpdater class.

        :param indicator: The indicator for updating.
        :param input_method: The indicators input method.
        :param outputs: The list of the indicators output properties.
        """
        Precondition.type_or_none(input_method, Callable, 'input_method')

        self._indicator = indicator
        if input_method is None:
            self._input_method = indicator.update
        else:
            self._input_method = input_method

        self._input_params = []

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

        for param in inspect.signature(self._input_method).parameters:
            self._input_params.append(param_map[param])

        if outputs is None or len(outputs) == 0:
            self._outputs = ['value']
        else:
            self._outputs = outputs

    cpdef void update_bar(self, Bar bar):
        """
        Update the indicator with the given Bar object.

        :param bar: The update bar.
        """
        self._input_method(*[bar.__getattribute__(param) for param in self._input_params])

    cpdef void update_data_bar(self, DataBar bar):
        """
        Update the indicator with the given Bar object.

        :param bar: The update bar.
        """
        self._input_method(*[bar.__getattribute__(param) for param in self._input_params])

    cpdef object build_features(self, list bars):
        """
        Create a dictionary of output features from the given bars data.
        
        :return: The list of indicator output feature.
        """
        features = {}
        for output in self._outputs:
            features[output] = []

        for bar in bars:
            self.update_bar(bar)

            for value in self._get_values():
                features[value[0]].append(value[1])

        return features

    cpdef object build_features_data_bars(self, list bars):
        """
        Create a dictionary of output features from the given bars data.
        
        :return: The list of indicator output feature.
        """
        features = {}
        for output in self._outputs:
            features[output] = []

        for bar in bars:
            self.update_data_bar(bar)

            for value in self._get_values():
                features[value[0]].append(value[1])

        return features

    cdef list _get_values(self):
        """
        Create a list of the current indicator outputs.
        
        :return: The list of indicator outputs.
        """
        return [(output, self._indicator.__getattribute__(output)) for output in self._outputs]
