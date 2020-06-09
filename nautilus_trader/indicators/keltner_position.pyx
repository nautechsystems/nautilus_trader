# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import cython

from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.indicators.keltner_channel cimport KeltnerChannel
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef class KeltnerPosition(Indicator):
    """
    An indicator which calculates the relative position of the given price
    within a defined Keltner channel. This provides a measure of the relative
    'extension' of a market from the mean, as a multiple of volatility.
    """

    def __init__(self,
                 int period,
                 double k_multiplier,
                 ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
                 ma_type_atr not None: MovingAverageType=MovingAverageType.SIMPLE,
                 bint use_previous=True,
                 double atr_floor=0.0,
                 bint check_inputs=False):
        """
        Initializes a new instance of the KeltnerChannel class.

        :param period: The rolling window period for the indicator (> 0).
        :param k_multiplier: The multiplier for the ATR (> 0).
        :param ma_type: The moving average type for the middle band (cannot be None).
        :param ma_type_atr: The moving average type for the internal ATR (cannot be None).
        :param use_previous: The boolean flag indicating whether previous price values should be used.
        :param atr_floor: The ATR floor (minimum) output value for the indicator (>= 0).
        :param check_inputs: The flag indicating whether the input values should be checked.
        """
        Condition.positive_int(period, 'period')
        Condition.positive(k_multiplier, 'k_multiplier')
        Condition.not_negative(atr_floor, 'atr_floor')

        super().__init__(params=[period,
                                 k_multiplier,
                                 ma_type.name,
                                 ma_type_atr.name,
                                 use_previous,
                                 atr_floor],
                         check_inputs=check_inputs)
        self._period = period
        self._kc = KeltnerChannel(
            period,
            k_multiplier,
            ma_type,
            ma_type_atr,
            use_previous,
            atr_floor)
        self.value = 0.0

    @property
    def period(self) -> int:
        """
        :return: The period of the indicator.
        """
        return self._period

    @property
    def k_multiplier(self) -> float:
        """
        :return: The k-multiplier which calculates the upper and lower bands.
        """
        return self._kc.k_multiplier

    @cython.binding(True)
    cpdef void update(
            self,
            double high,
            double low,
            double close):
        """
        Update the indicator with the given values.

        :param high: The high price (> 0).
        :param low: The low price (> 0).
        :param close: The close price (> 0).
        """
        if self.check_inputs:
            Condition.positive(high, 'high')
            Condition.positive(low, 'low')
            Condition.positive(close, 'close')
            Condition.true(high >= low, 'high >= low')
            Condition.true(high >= close, 'high >= close')
            Condition.true(low <= close, 'low <= close')

        self._kc.update(high, low, close)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs()
            if self._kc.initialized:
                self._set_initialized()

        cdef double k_width = (self._kc.value_upper_band - self._kc.value_lower_band) / 2

        if k_width > 0.0:
            self.value = (close - self._kc.value_middle_band) / k_width
        else:
            self.value = 0.0

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._kc.reset()
        self.value = 0.0
