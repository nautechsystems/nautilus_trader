# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

cimport numpy as np

from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick_scheme.base cimport TickScheme


cdef class TieredTickScheme(TickScheme):
    cdef list tiers
    cdef int max_ticks_per_tier
    cdef int price_precision
    cdef int tick_count

    cdef readonly np.ndarray ticks

    cpdef _build_ticks(self)

    cpdef int find_tick_index(self, double value)
    cpdef Price next_ask_price(self, double value, int n=*)
    cpdef Price next_bid_price(self, double value, int n=*)
