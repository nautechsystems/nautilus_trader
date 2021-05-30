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
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price


cdef class BidAskMinMax(Indicator):
    cdef readonly InstrumentId instrument_id
    """The instrument_id for inbound ticks.\n\n:returns: `InstrumentId`"""
    cdef readonly timedelta lookback
    """The look back duration in time.\n\n:returns: `timedelta`"""
    cdef readonly WindowedMinMaxPrices bids
    """The windowed min max prices.\n\n:returns: `WindowedMinMaxPrices`"""
    cdef readonly WindowedMinMaxPrices asks
    """The windowed min max prices.\n\n:returns: `WindowedMinMaxPrices`"""


cdef class WindowedMinMaxPrices:
    cdef object _min_prices
    cdef object _max_prices

    cdef readonly timedelta lookback
    """The look back duration in time.\n\n:returns: `timedelta`"""
    cdef readonly Price min_price
    """The minimum price in the window.\n\n:returns: `Price`"""
    cdef readonly Price max_price
    """The maximum price in the window.\n\n:returns: `Price`"""

    cpdef void add_price(self, datetime ts, Price price) except *
    cpdef void reset(self) except *

    cdef void _expire_stale_prices_by_cutoff(self, ts_prices, datetime cutoff) except *
    cdef void _add_min_price(self, datetime ts, Price price) except *
    cdef void _add_max_price(self, datetime ts, Price price) except *
