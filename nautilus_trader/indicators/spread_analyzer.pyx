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

from collections import deque

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport fast_mean_iterated
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.tick cimport QuoteTick


cdef class SpreadAnalyzer(Indicator):
    """
    Provides various spread analysis metrics.
    """

    def __init__(self, Symbol symbol not None, int capacity):
        """
        Initialize a new instance of the SpreadAnalyzer class.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the tick updates.
        capacity : int
            The max length for the internal <QuoteTick> deque (determines averages).

        Raises
        ------
        ValueError
            If capacity is not positive (> 0).

        """
        Condition.positive_int(capacity, "capacity")
        super().__init__(params=[symbol, capacity])

        self.symbol = symbol
        self.capacity = capacity
        self.current_spread = 0
        self.average_spread = 0

        self._spreads = deque(maxlen=self.capacity)

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """
        Update the analyzer with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick for the update.

        Raises
        ------
        ValueError
            If tick.symbol does not equal the analyzers symbol.

        """
        Condition.equal(self.symbol, tick.symbol, "symbol", "tick.symbol")

        # Check initialization
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._spreads) == self.capacity:
                self._set_initialized(True)

        cdef double spread = tick.ask.as_double() - tick.bid.as_double()

        self.current_spread = spread
        self._spreads.append(spread)

        # Update average spread
        self.average_spread = fast_mean_iterated(
            values=list(self._spreads),
            next_value=spread,
            current_value=self.average_spread,
            expected_length=self.capacity,
            drop_left=False)

    cpdef void reset(self):
        """
        Reset the analyzer.

        All stateful values are reset to their initial value.

        """
        self._reset_base()
        self._spreads.clear()
        self.current_spread = 0
        self.average_spread = 0
