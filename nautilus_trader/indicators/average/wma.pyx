# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import cython
import numpy as np
from collections import deque

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.core.correctness cimport Condition


cdef class WeightedMovingAverage(MovingAverage):
    """
    An indicator which calculates a weighted moving average across a rolling window.
    """

    def __init__(self,
                 int period,
                 weights=None):
        """
        Initializes a new instance of the SimpleMovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        :param weights: The weights for the moving average calculation
        (if not None then = period).
        """
        Condition.positive_int(period, 'period')
        if weights is not None:
            Condition.equal(len(weights), period, 'len(weights)', 'period')

        super().__init__(period, params=[period, weights])
        self._inputs = deque(maxlen=self.period)
        self.weights = weights
        self.value = 0.0

    @cython.binding(True)
    cpdef void update(self, double point):
        """
        Update the indicator with the given point value.

        :param point: The input point value for the update.
        """
        self._update(point)
        self._inputs.append(point)

        if self.initialized or self.weights is None:
            self.value = np.average(self._inputs, weights=self.weights, axis=0)
        else:
            self.value = np.average(self._inputs, weights=self.weights[-len(self._inputs):], axis=0)

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
        self._inputs.clear()
