# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.average.moving_average import MovingAverage, MovingAverageType
from nautilus_trader.indicators.average.ama import AdaptiveMovingAverage
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.indicators.average.sma import SimpleMovingAverage
from nautilus_trader.indicators.average.wma import WeightedMovingAverage
from nautilus_trader.indicators.average.hma import HullMovingAverage
from nautilus_trader.core.correctness cimport Condition


cdef class MovingAverageFactory:
    """
    Provides a factory to construct different moving average indicators.
    """

    @staticmethod
    def create(int period,
               object ma_type: MovingAverageType,
               **kwargs) -> MovingAverage:
        """
        Create a moving average indicator corresponding to the given ma_type.

        :param period: The period of the moving average (> 0).
        :param ma_type: The moving average type.
        :return: The moving average indicator.
        """
        Condition.positive(period, 'period')

        if ma_type == MovingAverageType.SIMPLE:
            return SimpleMovingAverage(period)

        elif ma_type == MovingAverageType.EXPONENTIAL:
            return ExponentialMovingAverage(period)

        elif ma_type == MovingAverageType.WEIGHTED:
            return WeightedMovingAverage(period, **kwargs)

        elif ma_type == MovingAverageType.HULL:
            return HullMovingAverage(period)

        elif ma_type == MovingAverageType.ADAPTIVE:
            return AdaptiveMovingAverage(period, **kwargs)
