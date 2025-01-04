# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition

from nautilus_trader.indicators.average.dema import DoubleExponentialMovingAverage
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.indicators.average.hma import HullMovingAverage
from nautilus_trader.indicators.average.moving_average import MovingAverage
from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.indicators.average.rma import WilderMovingAverage
from nautilus_trader.indicators.average.sma import SimpleMovingAverage
from nautilus_trader.indicators.average.vidya import VariableIndexDynamicAverage
from nautilus_trader.indicators.average.wma import WeightedMovingAverage


cdef class MovingAverageFactory:
    """
    Provides a factory to construct different moving average indicators.
    """

    @staticmethod
    def create(
        int period,
        ma_type: MovingAverageType,
        **kwargs,
    ) -> MovingAverage:
        """
        Create a moving average indicator corresponding to the given ma_type.

        Parameters
        ----------
        period : int
            The period of the moving average (> 0).
        ma_type : MovingAverageType
            The moving average type.

        Returns
        -------
        MovingAverage

        Raises
        ------
        ValueError
            If `period` is not positive (> 0).

        """
        Condition.positive_int(period, "period")

        if ma_type == MovingAverageType.SIMPLE:
            return SimpleMovingAverage(period)

        elif ma_type == MovingAverageType.EXPONENTIAL:
            return ExponentialMovingAverage(period)

        elif ma_type == MovingAverageType.WEIGHTED:
            return WeightedMovingAverage(period, **kwargs)

        elif ma_type == MovingAverageType.HULL:
            return HullMovingAverage(period)

        elif ma_type == MovingAverageType.WILDER:
            return WilderMovingAverage(period)

        elif ma_type == MovingAverageType.DOUBLE_EXPONENTIAL:
            return DoubleExponentialMovingAverage(period)

        elif ma_type == MovingAverageType.VARIABLE_INDEX_DYNAMIC:
            return VariableIndexDynamicAverage(period, **kwargs)
