# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import cython

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.core.correctness cimport Condition


cdef class ExponentialMovingAverage(MovingAverage):
    """
    An indicator which calculates an exponential moving average across a
    rolling window.
    """

    def __init__(self, int period):
        """
        Initializes a new instance of the ExponentialMovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        """
        Condition.positive_int(period, 'period')

        super().__init__(period, params=[period])
        self.alpha = 2.0 / (period + 1.0)
        self.value = 0.0

    @cython.binding(True)
    cpdef update(self, double point):
        """
        Update the indicator with the given point value.

        :param point: The input point value for the update.
        """
        # Check if this is the initial input
        if not self.has_inputs:
            self._update(point)
            self.value = point
            return

        self._update(point)
        self.value = self.alpha * point + ((1.0 - self.alpha) * self.value)

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
