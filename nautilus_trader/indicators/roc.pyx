# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import cython
from math import log
from collections import deque

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef class RateOfChange(Indicator):
    """
    An indicator which calculates the rate of change of price over a defined period.
    The return output can be simple or log.
    """

    def __init__(self,
                 int period,
                 bint use_log=False,
                 bint check_inputs=False):
        """
        Initializes a new instance of the RateOfChange class.

        :param period: The period for the indicator (> 1).
        :param use_log: Use log returns for value calculation.
        :param check_inputs: The flag indicating whether the input values should be checked.
        """
        Condition.true(period > 1, 'period > 1')

        super().__init__(params=[period], check_inputs=check_inputs)
        self.period = period
        self._use_log = use_log
        self._prices = deque(maxlen=self.period)
        self.value = 0.0

    @cython.binding(True)
    cpdef void update(self, double price):
        """
        Update the indicator with the given price value.

        :param price: The price value.
        """
        if self.check_inputs:
            Condition.positive(price, 'price')

        self._prices.append(price)

        if not self.initialized:
            self._set_has_inputs()
            if len(self._prices) >= self.period:
                self._set_initialized()

        if self._use_log:
            self.value = log(price / self._prices[0])
        else:
            self.value = (price - self._prices[0]) / self._prices[0]

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._prices.clear()
        self.value = 0.0
