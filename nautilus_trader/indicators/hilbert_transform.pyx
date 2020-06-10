# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import cython
from collections import deque

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef class HilbertTransform(Indicator):
    """
    An indicator which calculates a Hilbert Transform across a rolling window.
    The Hilbert Transform itself, is an all-pass filter used in digital signal
    processing. By using present and prior price differences, and some feedback,
    price values are split into their complex number components of real (in-phase)
    and imaginary (quadrature) parts.
    """

    def __init__(self, int period=7, bint check_inputs=False):
        """
        Initializes a new instance of the HilbertTransform class.

        :param period: The rolling window period for the indicator (> 0).
        :param check: The flag indicating whether the input values should be checked.
        """
        Condition.positive_int(period, 'period')

        super().__init__(params=[period], check_inputs=check_inputs)
        self.period = period
        self._i_mult = 0.635
        self._q_mult = 0.338
        self._inputs = deque(maxlen=self.period)
        self._detrended_prices = deque(maxlen=self.period)
        self._in_phase = deque([0.0] * self.period, maxlen=self.period)
        self._quadrature = deque([0.0] * self.period, maxlen=self.period)
        self.value_in_phase = 0.0  # The last in-phase value (real part of complex number) held
        self.value_quad = 0.0      # The last quadrature value (imaginary part of complex number) held

    @cython.binding(True)
    cpdef void update(self, double price):
        """
        Update the indicator with the given point value (mid price).

        :param price: The price value (> 0).
        """
        if self.check_inputs:
            Condition.positive(price, 'price')

        self._inputs.append(price)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs()
            if len(self._inputs) >= self.period:
                self._set_initialized()
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

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._inputs.clear()
        self._detrended_prices.clear()
        self._in_phase = deque([0.0] * self.period, maxlen=self.period)
        self._quadrature = deque([0.0] * self.period, maxlen=self.period)
