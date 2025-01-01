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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.limit_if_touched cimport LimitIfTouchedOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_if_touched cimport MarketIfTouchedOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.model.orders.stop_limit cimport StopLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_limit cimport TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market cimport TrailingStopMarketOrder


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
        if init.order_type == OrderType.MARKET:
            return MarketOrder.create_c(init=init)
        elif init.order_type == OrderType.LIMIT:
            return LimitOrder.create_c(init=init)
        elif init.order_type == OrderType.STOP_MARKET:
            return StopMarketOrder.create_c(init=init)
        elif init.order_type == OrderType.STOP_LIMIT:
            return StopLimitOrder.create_c(init=init)
        elif init.order_type == OrderType.MARKET_TO_LIMIT:
            return MarketToLimitOrder.create_c(init=init)
        elif init.order_type == OrderType.MARKET_IF_TOUCHED:
            return MarketIfTouchedOrder.create_c(init=init)
        elif init.order_type == OrderType.LIMIT_IF_TOUCHED:
            return LimitIfTouchedOrder.create_c(init=init)
        elif init.order_type == OrderType.TRAILING_STOP_MARKET:
            return TrailingStopMarketOrder.create_c(init=init)
        elif init.order_type == OrderType.TRAILING_STOP_LIMIT:
            return TrailingStopLimitOrder.create_c(init=init)
        else:
            raise RuntimeError("invalid `OrderType`")  # pragma: no cover (design-time error)

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

    @staticmethod
    def from_init(OrderInitialized init) -> Order:
        """
        Return an order initialized from the given event.

        Parameters
        ----------
        init : OrderInitialized
            The event to initialize with.

        Returns
        -------
        Order

        """
        return OrderUnpacker.from_init_c(init)
