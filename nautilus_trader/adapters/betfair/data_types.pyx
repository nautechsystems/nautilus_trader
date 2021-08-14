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

from nautilus_trader.serialization.base import register_serializable_object


cpdef enum SubscriptionStatus:
    UNSUBSCRIBED = 0
    PENDING_STARTUP = 1
    RUNNING = 2


class InstrumentSearch(Data):
    def __init__(
        self,
        instruments,
        ts_event,
        ts_init,
    ):
        super().__init__(ts_event, ts_init)
        self.instruments = instruments


cdef class BSPOrderBookDelta(OrderBookDelta):
    @staticmethod
    def from_dict(dict values) -> BSPOrderBookDelta:
        return BSPOrderBookDelta.from_dict_c(values)

    @staticmethod
    def to_dict(BSPOrderBookDelta obj):
        return BSPOrderBookDelta.to_dict_c(obj)


register_serializable_object(BSPOrderBookDelta, BSPOrderBookDelta.to_dict, BSPOrderBookDelta.from_dict)
