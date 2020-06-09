# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from enum import Enum, unique

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


@unique
class MovingAverageType(Enum):
    """
    Represents the type of moving average.
    """
    SIMPLE = 0
    EXPONENTIAL = 1
    WEIGHTED = 2
    HULL = 3
    ADAPTIVE = 4


cdef class MovingAverage(Indicator):
    """
    The base class for all moving average type indicators.
    """

    def __init__(self,
                 int period,
                 list params=None):
        """
        Initializes a new instance of the abstract MovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        :param params: The initialization parameters for the indicator.
        """
        Condition.positive_int(period, 'period')

        super().__init__(params)
        self.period = period
        self.count = 0
        self.value = 0.0

    def update(self, double point):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    cdef void _update(self, double point):
        """
        Update the moving average indicator with the given point value.
        
        :param point: The input point value.
        """
        self.count += 1

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs()
            if self.count >= self.period:
                self._set_initialized()

    cdef void _reset_ma(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self.count = 0
        self.value = 0.0
