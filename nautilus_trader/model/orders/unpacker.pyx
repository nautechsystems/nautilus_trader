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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.stop_limit cimport StopLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder


cdef class OrderUnpacker:
    """
    Provides a means of unpacking orders from value dictionaries.
    """

    @staticmethod
    cdef Order unpack_c(dict values):
        Condition.not_none(values, "values")

        return OrderUnpacker.from_init_c(OrderInitialized.from_dict_c(values))

    @staticmethod
    cdef Order from_init_c(OrderInitialized init):
        if init.type == OrderType.MARKET:
            return MarketOrder.create(init=init)
        elif init.type == OrderType.LIMIT:
            return LimitOrder.create(init=init)
        elif init.type == OrderType.STOP_MARKET:
            return StopMarketOrder.create(init=init)
        elif init.type == OrderType.STOP_LIMIT:
            return StopLimitOrder.create(init=init)
        else:
            # Design-time error
            raise RuntimeError("invalid order type")

    @staticmethod
    def unpack(dict values) -> Order:
        """
        Return an order unpacked from the given values.

        Parameters
        ----------
        values : dict[str, object]

        Returns
        -------
        Order

        """
        return OrderUnpacker.unpack_c(values)
