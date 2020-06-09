# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import cython
import numpy as np

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.average.wma import WeightedMovingAverage
from nautilus_trader.core.correctness cimport Condition


cdef class HullMovingAverage(MovingAverage):
    """
    An indicator which calculates a Hull Moving Average (HMA) across a rolling
    window. The HMA, developed by Alan Hull, is an extremely fast and smooth
    moving average.
    """

    def __init__(self, int period):
        """
        Initializes a new instance of the HullMovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        """
        Condition.positive_int(period, 'period')

        super().__init__(period, params=[period])
        self._period_halved = int(self.period / 2)
        self._period_sqrt = int(np.sqrt(self.period))

        self._w1 = self._get_weights(self._period_halved)
        self._w2 = self._get_weights(self.period)
        self._w3 = self._get_weights(self._period_sqrt)

        self._ma1 = WeightedMovingAverage(self._period_halved, weights=self._w1)
        self._ma2 = WeightedMovingAverage(self.period, weights=self._w2)
        self._ma3 = WeightedMovingAverage(self._period_sqrt, weights=self._w3)

        self.value = 0.0

    cdef list _get_weights(self, int size):
        w = np.arange(1, size + 1)
        return list(w / sum(w))

    @cython.binding(True)
    cpdef void update(self, double point):
        """
        Update the indicator with the given point value.

        :param point: The input point value for the update.
        """
        self._update(point)

        self._ma1.update(point)
        self._ma2.update(point)
        self._ma3.update(self._ma1.value * 2.0 - self._ma2.value)

        self.value = self._ma3.value

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
        self._ma1.reset()
        self._ma2.reset()
        self._ma3.reset()
