#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="tools.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

import inspect
import pandas as pd

from cpython.datetime cimport datetime
from typing import Callable, List
from pandas.core.frame import DataFrame

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.objects cimport Bar, DataBar
from inv_trader.model.price cimport price
from inv_indicators.base.indicator import Indicator

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

    def __init__(self,
                 data: DataFrame=None,
                 int decimal_precision=5,
                 int volume_multiple=1):
        """
        Initializes a new instance of the BarBuilder class.

        :param data: The DataFrame containing the market data.
        :param decimal_precision: The decimal precision for bar prices.
        :param volume_multiple: The volume multiple for the builder (> 0).
        """
        Precondition.type_or_none(data, DataFrame, 'data')
        Precondition.not_negative(decimal_precision, 'decimal_precision')
        Precondition.positive(volume_multiple, 'volume_multiple')

        self._data = data
        self._decimal_precision = decimal_precision
        self._volume_multiple = volume_multiple

    cpdef list build_databars_all(self):
        """
        Build a list of DataBars from all data.
        
        :return: The list of DataBars.
        """
        return list(map(self._build_databar,
                        self._data.values,
                        pd.to_datetime(self._data.index, utc=True)))

    cpdef list build_databars_from(self, int index=0):
        """
        Build a list of DataBars from the given index.
        
        :return: The list of DatBars.
        """
        Precondition.not_negative(index, 'index')

        return list(map(self._build_databar,
                        self._data.iloc[index:].values,
                        pd.to_datetime(self._data.iloc[index:].index, utc=True)))

    cpdef list build_databars_range(self, int start=0, int end=-1):
        """
        Build a list of DataBars within the given range.
        
        :return: The list of Bars.
        """
        Precondition.not_negative(start, 'start')

        return list(map(self._build_databar,
                        self._data.iloc[start:end].values,
                        pd.to_datetime(self._data.iloc[start:end].index, utc=True)))

    cpdef list build_bars_all(self):
        """
        Build a list of Bars from all data.

        :return: The list of Bars.
        """
        return list(map(self._build_bar,
                        self._data.values,
                        pd.to_datetime(self._data.index, utc=True)))

    cpdef list build_bars_from(self, int index=0):
        """
        Build a list of Bars from the given index (>= 0).

        :return: The list of Bars.
        """
        Precondition.not_negative(index, 'index')

        return list(map(self._build_bar,
                        self._data.iloc[index:].values,
                        pd.to_datetime(self._data.iloc[index:].index, utc=True)))

    cpdef list build_bars_range(self, int start=0, int end=-1):
        """
        Build a list of Bars within the given range.

        :return: The list of Bars.
        """
        Precondition.not_negative(start, 'start')

        return list(map(self._build_bar,
                        self._data.iloc[start:end].values,
                        pd.to_datetime(self._data.iloc[start:end].index, utc=True)))

    cpdef DataBar _build_databar(self, double[:] values, datetime timestamp):
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

    cpdef Bar _build_bar(self, double[:] values, datetime timestamp):
        """
        Build a Bar from the given index and values. The function expects the
        values to be an ndarray with 5 elements [open, high, low, close, volume].

        :param values: The values for the bar.
        :param timestamp: The timestamp for the bar.
        :return: The built Bar.
        """
        return Bar(price(values[0], self._decimal_precision),
                   price(values[1], self._decimal_precision),
                   price(values[2], self._decimal_precision),
                   price(values[3], self._decimal_precision),
                   int(values[4] * self._volume_multiple),
                   timestamp)


cdef class IndicatorUpdater:
    """
    Provides an adapter for updating an indicator with a bar. When instantiated
    with a live indicator update method, the updater will inspect the method and
    construct the required parameter list for updates.
    """

    def __init__(self,
                 indicator: Indicator,
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

        cdef dict param_map = {
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

    cpdef void update_databar(self, DataBar bar):
        """
        Update the indicator with the given Bar object.

        :param bar: The update bar.
        """
        self._input_method(*[bar.__getattribute__(param) for param in self._input_params])

    cpdef dict build_features(self, list bars):
        """
        Create a dictionary of output features from the given bars data.
        
        :return: The list of indicator output feature.
        """
        cdef dict features = {}
        for output in self._outputs:
            features[output] = []

        for bar in bars:
            self.update_bar(bar)

            for value in self._get_values():
                features[value[0]].append(value[1])

        return features

    cpdef dict build_features_databars(self, list bars):
        """
        Create a dictionary of output features from the given bars data.
        
        :return: The list of indicator output feature.
        """
        cdef dict features = {}
        for output in self._outputs:
            features[output] = []

        for bar in bars:
            self.update_databar(bar)

            for value in self._get_values():
                features[value[0]].append(value[1])

        return features

    cdef list _get_values(self):
        """
        Create a list of the current indicator outputs.
        
        :return: The list of indicator outputs.
        """
        return [(output, self._indicator.__getattribute__(output)) for output in self._outputs]
