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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class HilbertTransform(Indicator):
    """
    An indicator which calculates a Hilbert Transform across a rolling window.
    The Hilbert Transform itself, is an all-pass filter used in digital signal
    processing. By using present and prior price differences, and some feedback,
    price values are split into their complex number components of real (in-phase)
    and imaginary (quadrature) parts.

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
        self._inputs = deque(maxlen=self.period)
        self._detrended_prices = deque(maxlen=self.period)
        self._in_phase = deque([0] * self.period, maxlen=self.period)
        self._quadrature = deque([0] * self.period, maxlen=self.period)
        self.value_in_phase = 0
        self.value_quad = 0

    cpdef void handle_bar(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double price) except *:
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        price : double
            The price.

        """
        self._inputs.append(price)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._inputs) >= self.period:
                self._set_initialized(True)
            else:
                return

        # Update de-trended prices
        self._detrended_prices.append(price - self._inputs[0])

        # If insufficient de-trended prices to index feedback then return
        if len(self._detrended_prices) < 5:
            return

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

        self.value_in_phase = self._in_phase[-1]
        self.value_quad = self._quadrature[-1]

    cpdef void _reset(self) except *:
        self._inputs.clear()
        self._detrended_prices.clear()
        self._in_phase = deque([0] * self.period, maxlen=self.period)
        self._quadrature = deque([0] * self.period, maxlen=self.period)
