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


cdef class MaxBidMinAsk(Indicator):

    cdef readonly Symbol symbol
    cdef readonly timedelta lookback

    cdef readonly Price max_bid
    cdef readonly Price min_ask

    cdef object _bid_prices
    cdef object _ask_prices

    cpdef void handle_quote_tick(self, QuoteTick tick) except *

    cpdef void reset(self)

    cdef inline void _handle_bid_and_ask(self, Price bid, Price ask, datetime timestamp)
    cdef inline void _prune_by_datetime_cutoff(self, object ts_prices, datetime cutoff)
    cdef inline void _append_bid(self, Price bid, datetime timestamp)
    cdef inline void _append_ask(self, Price ask, datetime timestamp)
