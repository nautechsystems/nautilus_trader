# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.efficiency_ratio cimport EfficiencyRatio
from nautilus_trader.model.bar cimport Bar


cdef class AdaptiveMovingAverage(MovingAverage):
    """
    An indicator which calculates an adaptive moving average (AMA) across a
    rolling window. Developed by Perry Kaufman, the AMA is a moving average
    designed to account for market noise and volatility. The AMA will closely
    follow prices when the price swings are relatively small and the noise is
    low. The AMA will increase lag when the price swings increase.
    """

    def __init__(self,
                 int period_er,
                 int period_alpha_fast,
                 int period_alpha_slow):
        """
        Initialize a new instance of the AdaptiveMovingAverage class.

        :param period_er: The period for the internal Efficiency Ratio (> 0).
        :param period_alpha_fast: The period for the fast smoothing constant (> 0).
        :param period_alpha_slow: The period for the slow smoothing constant (> 0 < alpha_fast).
        """
        Condition.positive_int(period_er, "period_er")
        Condition.positive_int(period_alpha_fast, "period_alpha_fast")
        Condition.positive_int(period_alpha_slow, "period_alpha_slow")
        Condition.true(period_alpha_slow > period_alpha_fast, "period_alpha_slow > period_alpha_fast")
        super().__init__(period_er, params=[period_er,
                                            period_alpha_fast,
                                            period_alpha_slow])

        self._period_er = period_er
        self._period_alpha_fast = period_alpha_fast
        self._period_alpha_slow = period_alpha_slow
        self._alpha_fast = 2.0 / (float(self._period_alpha_fast) + 1.0)
        self._alpha_slow = 2.0 / (float(self._period_alpha_slow) + 1.0)
        self._alpha_diff = self._alpha_fast - self._alpha_slow
        self._efficiency_ratio = EfficiencyRatio(self._period_er)
        self._prior_value = 0.0
        self.value = 0.0

    cpdef void update(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        :param bar: The update bar.
        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value) except *:
        """
        Update the indicator with the given raw value.

        :param value: The update value.
        """
        # Check if this is the initial input (then initialize variables)
        if not self.has_inputs:
            self.value = value

        self._update()
        self._efficiency_ratio.update_raw(value)
        self._prior_value = self.value

        # Calculate smoothing constant (sc)
        sc = np.power(self._efficiency_ratio.value * self._alpha_diff + self._alpha_slow, 2)

        # Calculate AMA
        self.value = self._prior_value + sc * (value - self._prior_value)

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
        self._efficiency_ratio.reset()
        self._prior_value = 0.0
