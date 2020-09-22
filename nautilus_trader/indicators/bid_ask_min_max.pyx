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

from cpython.datetime cimport timedelta

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.tick cimport QuoteTick


cdef class BidAskMinMax(Indicator):
    """
    Given a historic lookback window of bid/ask prices, keep a running
    computation of the min/max values of the bid/ask prices within the window.
    """

    def __init__(self, Symbol symbol not None, timedelta lookback not None):
        """
        Initialize a new instance of the BidAskMinMax class.

        Parameters
        ----------
        symbol : Symbol
            The symbol for inbound ticks.
        lookback : timedelta
            The look back duration in time.
        """
        self.symbol = symbol
        self.lookback = lookback
        # Set up the bid/ask windows
        self.bids = WindowedMinMaxPrices(lookback)
        self.asks = WindowedMinMaxPrices(lookback)

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """
        Given a QuoteTick, run its bid/ask through the indicator

        Parameters
        ----------
        tick : QuoteTick
            Incoming quote tick to process
        """
        self.bids.add_price(tick.timestamp, tick.bid)
        self.asks.add_price(tick.timestamp, tick.ask)

        # Mark as having input and initialized
        self._set_has_inputs(True)
        self._set_initialized(True)

    cpdef void reset(self):
        """Reset the instance to like-new."""
        self._reset_base()
        # Reset the windows
        self.bids.reset()
        self.asks.reset()
