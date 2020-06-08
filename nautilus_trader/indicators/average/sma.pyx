# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import cython
from collections import deque

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport fast_mean
from nautilus_trader.indicators.average.moving_average cimport MovingAverage


cdef class SimpleMovingAverage(MovingAverage):
    """
    An indicator which calculates a simple moving average across a rolling window.
    """

    def __init__(self, int period):
        """
        Initializes a new instance of the SimpleMovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        """
        Condition.positive_int(period, 'period')

        super().__init__(period, params=[period])
        self._inputs = deque(maxlen=period)
        self.value = 0.0

    @cython.binding(True)
    cpdef update(self, double point):
        """
        Update the indicator with the given point value.

        :param point: The input point value for the update.
        """
        self._update(point)
        self._inputs.append(point)

        self.value = fast_mean(list(self._inputs))

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
        self._inputs.clear()
