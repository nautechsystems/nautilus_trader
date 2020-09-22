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
from nautilus_trader.indicators.utils.windowed_min_max_prices cimport WindowedMinMaxPrices
from nautilus_trader.model.identifiers cimport Symbol
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
