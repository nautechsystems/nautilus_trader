# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from collections import deque

import numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data.bar cimport Bar


cdef class HilbertPeriod(Indicator):
    """
    An indicator which calculates the instantaneous period of phase-change for a
    market across a rolling window. One basic definition of a cycle is that the
    phase has a constant rate of change, i.e. A 10 bar cycle changes phase at
    the rate of 36 degrees per bar so that 360 degrees of phase is completed
    (one full cycle) every 10 bars.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    """

    def __init__(self, int period=7):
        Condition.positive_int(period, "period")
        super().__init__(params=[period])

        self.period = period
        self._i_mult = 0.635
        self._q_mult = 0.338
        self._amplitude_floor = 0.001
        self._inputs = deque(maxlen=self.period)
        self._detrended_prices = deque(maxlen=self.period)
        self._in_phase =deque([0.0] * self.period, maxlen=self.period)
        self._quadrature = deque([0.0] * self.period, maxlen=self.period)
        self._phase = deque([0.0] * 2, maxlen=2)
        self._delta_phase = []
        self.value = 0  # The last instantaneous period value

    cpdef void handle_bar(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.high.as_double(), bar.low.as_double())

    cpdef void update_raw(self, double high, double low) except *:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.

        """
        self._inputs.append((high + low) / 2)

        # Initialization logic (leave this here)
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._inputs) >= self.period:
                self._set_initialized(True)
            else:
                return

        # Update de-trended prices
        self._detrended_prices.append(self._inputs[-1] - self._inputs[0])

        # If insufficient de-trended prices to index feedback then return
        if len(self._detrended_prices) < 5:
            return

        self._calc_hilbert_transform()

        # Compute current phase
        cdef double in_phase_value = self._in_phase[-1] + self._in_phase[-2]
        cdef double quadrature_value = self._quadrature[-1] + self._quadrature[-2]
        if abs(in_phase_value) > 0.0:
            self._phase.append(np.arctan(abs(quadrature_value / in_phase_value)))

        # Resolve the arc tangent ambiguity
        if self._in_phase[-1] < 0.0 < self._quadrature[-1]:
            self._phase.append(180 - self._phase[-1])
        if self._in_phase[-1] < 0.0 and self._quadrature[-1] < 0.0:
            self._phase.append(180 + self._phase[-1])
        if self._in_phase[-1] > 0.0 > self._quadrature[-1]:
            self._phase.append(360 - self._phase[-1])

        # Compute a differential phase, resolve wraparound, and limit delta-phase errors
        self._delta_phase.append(self._phase[-1] - self._phase[-2])
        if self._phase[-2] < 90. and self._phase[-1] > 270.0:
            self._delta_phase[-1] = 360 + self._phase[-2] - self._phase[-1]
        if self._delta_phase[-1] < 1:
            self._delta_phase[-1] = 1
        if self._delta_phase[-1] > 60:
            self._delta_phase[-1] = 60

        # Sum delta-phase to reach 360 degrees (sum loop count is the instantaneous period)
        cdef int inst_period = 0
        cdef int cumulative_delta_phase = 0
        cdef int i
        for i in range(min(len(self._delta_phase) - 1, 50)):
            cumulative_delta_phase += self._delta_phase[-(1 + i)]
            if cumulative_delta_phase > 360.0:
                inst_period = i
                break

        self.value = max(inst_period, self.period)

    cpdef void _calc_hilbert_transform(self) except *:
        # Calculate the Hilbert Transform and update in-phase and quadrature values
        # Calculate feedback
        cdef double feedback1 = self._detrended_prices[-1]  # V1 (last)
        cdef double feedback2 = self._detrended_prices[-3]  # V2 (2 elements back from the last)
        cdef double feedback4 = self._detrended_prices[-5]  # V4 (4 elements back from the last)

        cdef double in_phase3 = self._in_phase[-4]             # (3 elements back from the last)
        cdef double quadrature2 = self._quadrature[-3]         # (2 elements back from the last)

        # Calculate in-phase
        self._in_phase.append(
            1.25 * (feedback4 - (self._i_mult * feedback2) + (self._i_mult * in_phase3)))

        # Calculate quadrature
        self._quadrature.append(
            feedback2 - (self._q_mult * feedback1) + (self._q_mult * quadrature2))

    cpdef void _reset(self) except *:
        self._inputs.clear()
        self._detrended_prices.clear()
        self._in_phase = deque([0.0] * self.period, maxlen=self.period)
        self._quadrature = deque([0.0] * self.period, maxlen=self.period)
        self._phase = deque([0.0] * 2, maxlen=2)
        self._delta_phase.clear()
        self.value = 0.0
