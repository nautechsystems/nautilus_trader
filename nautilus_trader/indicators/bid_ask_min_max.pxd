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


cdef class BidAskMinMax(Indicator):
    cdef readonly Symbol symbol
    cdef readonly timedelta lookback
    cdef readonly WindowedMinMaxPrices bids
    cdef readonly WindowedMinMaxPrices asks


cdef class WindowedMinMaxPrices:
    cdef object _min_prices
    cdef object _max_prices

    cdef readonly timedelta lookback
    cdef readonly Price min_price
    cdef readonly Price max_price

    cpdef void add_price(self, datetime ts, Price price) except *
    cpdef void reset(self) except *

    cdef inline void _expire_stale_prices_by_cutoff(self, object ts_prices, datetime cutoff) except *
    cdef inline void _add_min_price(self, datetime ts, Price price) except *
    cdef inline void _add_max_price(self, datetime ts, Price price) except *
