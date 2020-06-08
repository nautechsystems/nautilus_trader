# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import cython
from collections import deque

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef class OnBalanceVolume(Indicator):
    """
    An indicator which calculates the momentum of relative positive or negative volume.
    """

    def __init__(self, int period=0, bint check_inputs=False):
        """
        Initializes a new instance of the OnBalanceVolume class.

        :param period: The period for the indicator, zero indicates no window (>= 0).
        :param check_inputs: The flag indicating whether the input values should be checked.
        """
        Condition.not_negative(period, 'period')

        super().__init__(params=[period], check_inputs=check_inputs)
        self.period = period
        self._obv = deque(maxlen=None if self.period == 0 else self.period)
        self.value = 0.0

    @cython.binding(True)
    cpdef void update(
            self,
            double open_price,
            double close_price,
            double volume):
        """
        Update the indicator with the given values.

        :param open_price: The open price (> 0).
        :param close_price: The close price (> 0).
        :param volume: The volume (>= 0).
        """
        if self.check_inputs:
            Condition.positive(open_price, 'open_price')
            Condition.positive(close_price, 'close_price')
            Condition.not_negative(volume, 'volume')

        if close_price > open_price:
            self._obv.append(volume)
        elif close_price < open_price:
            self._obv.append(-volume)
        else:
            self._obv.append(0)

        self.value = sum(self._obv)

        # Initialization logic
        if not self.initialized:
            self.has_inputs = True
            if (self.period == 0 and len(self._obv) > 0) or len(self._obv) >= self.period:
                self.initialized = True

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._obv.clear()
