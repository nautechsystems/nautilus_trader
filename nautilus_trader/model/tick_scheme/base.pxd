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

from nautilus_trader.model.objects cimport Price


cdef dict TICK_SCHEMES

cdef class TickScheme:
    cdef readonly str name
    """The name of the scheme.\n\n:returns: `str`"""
    cdef readonly Price min_price
    """The minimum valid price for the scheme.\n\n:returns: `Price`"""
    cdef readonly Price max_price
    """The maximum valid price for the scheme.\n\n:returns: `Price`"""

    cpdef Price next_ask_price(self, double value, int n=*)
    cpdef Price next_bid_price(self, double value, int n=*)


cpdef double round_down(double value, double base)
cpdef double round_up(double value, double base)

cpdef void register_tick_scheme(TickScheme tick_scheme)
cpdef TickScheme get_tick_scheme(str name)
