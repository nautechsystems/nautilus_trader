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
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.orderbook.data cimport OrderBookDelta


cpdef enum SubscriptionStatus:
    UNSUBSCRIBED = 0
    PENDING_STARTUP = 1
    RUNNING = 2


class InstrumentSearch(Data):
    def __init__(
        self,
        instruments,
        ts_event_ns,
        ts_recv_ns,
    ):
        super().__init__(ts_event_ns, ts_recv_ns)
        self.instruments = instruments


cdef class BPSOrderBookDelta(OrderBookDelta):
    pass
