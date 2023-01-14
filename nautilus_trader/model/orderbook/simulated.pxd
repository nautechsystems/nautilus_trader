# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.orderbook.book cimport L1OrderBook
from nautilus_trader.model.orderbook.book cimport L2OrderBook
from nautilus_trader.model.orderbook.book cimport L3OrderBook


cdef class SimulatedL1OrderBook(L1OrderBook):
    cdef void _update_bid(self, double price, double size) except *
    cdef void _update_ask(self, double price, double size) except *


cdef class SimulatedL2OrderBook(L2OrderBook):
    pass


cdef class SimulatedL3OrderBook(L3OrderBook):
    pass
