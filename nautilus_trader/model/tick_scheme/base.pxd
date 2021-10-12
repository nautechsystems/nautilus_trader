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

from nautilus_trader.model.objects cimport Price


cdef class TickScheme:
    cdef readonly Price min_tick
    cdef readonly Price max_tick
    cpdef Price nearest_ask_tick(self, double price)
    cpdef Price next_ask_tick(self, double price, int n=*)
    cpdef Price nearest_bid_tick(self, double price)
    cpdef Price next_bid_tick(self, double price, int n=*)
