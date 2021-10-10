# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
    cdef np.ndarray ticks
    cdef np.ndarray boundaries
    cdef readonly np.ndarray bases
    cdef np.ndarray precisions

    cpdef int get_boundaries_idx(self, double value)
    cpdef Price next_ask_tick(self, double price)
    cpdef Price next_bid_tick(self, double price)
