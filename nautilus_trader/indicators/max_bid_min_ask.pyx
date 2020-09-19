# -------------------------------------------------------------------------------------------------
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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport is_datetime_utc
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick cimport QuoteTick


cdef class MaxBidMinAsk(Indicator):

    def __init__(self, Symbol symbol not None, timedelta lookback not None):
        """
        Initialize a new instance of the MaxBidMinAsk class

        Parameters
        ----------
        symbol : Symbol
            The symbol for inbound ticks.
        lookback : timedelta
            The look back duration in time.

        """
        self.symbol = symbol
        self.lookback = lookback

        self.max_bid = None
        self.min_ask = None

        self._bid_prices = deque()
        self._ask_prices = deque()

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        self._handle_bid_and_ask(tick.bid, tick.ask, tick.timestamp)

    cpdef void reset(self):
        self._reset_base()
        self.max_bid = None
        self.min_ask = None
        self._bid_prices.clear()
        self._ask_prices.clear()

    cdef inline void _handle_bid_and_ask(self, Price bid, Price ask, datetime timestamp):
        Condition.true(is_datetime_utc(timestamp), "timestamp is tz aware UTC")

        cdef datetime cutoff = timestamp - self.lookback

        # Bids
        self._prune_by_datetime_cutoff(self._bid_prices, cutoff)
        self._append_bid(bid, timestamp)

        # Asks
        self._prune_by_datetime_cutoff(self._ask_prices, cutoff)
        self._append_ask(ask, timestamp)

        # Pull out the min/max
        self.max_bid = max([p[1] for p in self._bid_prices])
        self.min_ask = min([p[1] for p in self._ask_prices])

        self._set_has_inputs(True)
        self._set_initialized(True)

    cdef inline void _prune_by_datetime_cutoff(self, object ts_prices, datetime cutoff):
        """Drop items that are older than the cutoff"""
        while ts_prices and ts_prices[0][0] < cutoff:
            ts_prices.popleft()

    cdef inline void _append_bid(self, Price bid, datetime timestamp):
        """Handle bids"""
        # Pop front elements that are less than or equal (since we want the max bid)
        while self._bid_prices and self._bid_prices[-1][1] <= bid:
            self._bid_prices.pop()

        # Pop back elements that are less than or equal to the new bid
        while self._bid_prices and self._bid_prices[0][1] <= bid:
            self._bid_prices.popleft()

        self._bid_prices.append((timestamp, bid))

    cdef inline void _append_ask(self, Price ask, datetime timestamp):
        """Handle asks"""
        # Pop front elements that are less than or equal (since we want the max ask)
        while self._ask_prices and self._ask_prices[-1][1] <= ask:
            self._ask_prices.pop()

        # Pop back elements that are less than or equal to the new ask
        while self._ask_prices and self._ask_prices[0][1] <= ask:
            self._ask_prices.popleft()

        self._ask_prices.append((timestamp, ask))
