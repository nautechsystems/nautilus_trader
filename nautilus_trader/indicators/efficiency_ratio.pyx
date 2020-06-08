# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import cython

from collections import deque

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef class EfficiencyRatio(Indicator):
    """
    An indicator which calculates the efficiency ratio across a rolling window.
    The Kaufman Efficiency measures the ratio of the relative market speed in
    relation to the volatility, this could be thought of as a proxy for noise.
    """

    def __init__(self, int period, bint check_inputs=False):
        """
        Initializes a new instance of the EfficiencyRatio class.

        :param period: The rolling window period for the indicator (>= 2).
        :param check: The flag indicating whether the input values should be checked.
        """
        Condition.true(period >= 2, 'period >= 2')

        super().__init__(params=[period], check_inputs=check_inputs)
        self.period = period
        self._inputs = deque(maxlen=self.period)
        self._deltas = deque(maxlen=self.period)
        self.value = 0.0

    @cython.binding(True)
    cpdef void update(self, double price):
        """
        Update the indicator with the given price value.
        
        :param price: The price (> 0).
        """
        if self.check_inputs:
            Condition.positive(price, 'price')

        self._inputs.append(price)

        # Initialization logic
        if not self.initialized:
            self.has_inputs = True
            if len(self._inputs) < 2:
                return  # Not enough data
            elif len(self._inputs) >= self.period:
                self.initialized = True

        # Add data to queues
        self._deltas.append(abs(self._inputs[-1] - self._inputs[-2]))

        # Calculate efficiency ratio
        cdef double net_diff = abs(self._inputs[0] - self._inputs[-1])
        cdef double sum_deltas = sum(self._deltas)

        if sum_deltas > 0.0:
            self.value = net_diff / sum_deltas
        else:
            self.value = 0.0

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._inputs.clear()
        self._deltas.clear()
        self.value = 0.0
