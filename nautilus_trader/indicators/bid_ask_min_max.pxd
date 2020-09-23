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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick cimport QuoteTick


cdef class BidAskMinMax(Indicator):
    """
    Given a historic lookback window of bid/ask prices, keep a running
    computation of the min/max values of the bid/ask prices within the window.
    """

    cdef readonly Symbol symbol
    cdef readonly timedelta lookback

    cdef readonly WindowedMinMaxPrices bids
    cdef readonly WindowedMinMaxPrices asks

    cpdef void handle_quote_tick(self, QuoteTick tick) except *
    cpdef void reset(self)


cdef class WindowedMinMaxPrices:
    """
    Over the course of a defined lookback window, efficiently keep track
    of the min/max values currently in the window.
    """

    cdef readonly timedelta lookback

    cdef readonly Price min_price
    cdef readonly Price max_price

    cdef object _min_prices
    cdef object _max_prices

    cpdef void add_price(self, datetime ts, Price price)
    cpdef void reset(self)

    cdef inline void _expire_stale_prices_by_cutoff(self, object ts_prices, datetime cutoff)
    cdef inline void _add_min_price(self, datetime ts, Price price)
    cdef inline void _add_max_price(self, datetime ts, Price price)
