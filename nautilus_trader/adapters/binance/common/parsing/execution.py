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

from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType


class BinanceExecutionParser:
    """
    Provides common parsing methods for execution on the 'Binance' exchange.

    Warnings:
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self) -> None:
        # Construct dictionary hashmaps
        self.ext_status_to_int_status = {
            BinanceOrderStatus.NEW: OrderStatus.ACCEPTED,
            BinanceOrderStatus.CANCELED: OrderStatus.CANCELED,
            BinanceOrderStatus.PARTIALLY_FILLED: OrderStatus.PARTIALLY_FILLED,
            BinanceOrderStatus.FILLED: OrderStatus.FILLED,
            BinanceOrderStatus.NEW_ADL: OrderStatus.FILLED,
            BinanceOrderStatus.NEW_INSURANCE: OrderStatus.FILLED,
            BinanceOrderStatus.EXPIRED: OrderStatus.EXPIRED,
        }

        # NOTE: There was some asymmetry in the original `parse_order_type` functions for SPOT & FUTURES
        # need to check that the below is absolutely correct..
        self.ext_order_type_to_int_order_type = {
            BinanceOrderType.STOP: OrderType.STOP_LIMIT,
            BinanceOrderType.STOP_LOSS: OrderType.STOP_MARKET,
            BinanceOrderType.STOP_MARKET: OrderType.STOP_MARKET,
            BinanceOrderType.STOP_LOSS_LIMIT: OrderType.STOP_LIMIT,
            BinanceOrderType.TAKE_PROFIT: OrderType.LIMIT_IF_TOUCHED,
            BinanceOrderType.TAKE_PROFIT_LIMIT: OrderType.STOP_LIMIT,
            BinanceOrderType.TAKE_PROFIT_MARKET: OrderType.MARKET_IF_TOUCHED,
            BinanceOrderType.LIMIT: OrderType.LIMIT,
            BinanceOrderType.LIMIT_MAKER: OrderType.LIMIT,
        }

        # Build symmetrical reverse dictionary hashmaps
        self._build_int_to_ext_dicts()

    def _build_int_to_ext_dicts(self):
        self.int_status_to_ext_status = dict(
            map(
                reversed,
                self.ext_status_to_int_status.items(),
            ),
        )
        self.int_order_type_to_ext_order_type = dict(
            map(
                reversed,
                self.ext_order_type_to_int_order_type.items(),
            ),
        )

    def parse_binance_time_in_force(self, time_in_force: BinanceTimeInForce) -> TimeInForce:
        if time_in_force == BinanceTimeInForce.GTX:
            return TimeInForce.GTC
        else:
            return TimeInForce[time_in_force.value]

    def parse_binance_order_status(self, order_status: BinanceOrderStatus) -> OrderStatus:
        try:
            return self.ext_status_to_int_status[order_status]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized binance order status, was {order_status}",  # pragma: no cover
            )

    def parse_internal_order_status(self, order_status: OrderStatus) -> BinanceOrderStatus:
        try:
            return self.int_status_to_ext_status[order_status]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized internal order status, was {order_status}",  # pragma: no cover
            )

    def parse_binance_order_type(self, order_type: BinanceOrderType) -> OrderType:
        try:
            return self.ext_order_type_to_int_order_type[order_type]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized binance order type, was {order_type}",  # pragma: no cover
            )

    def parse_internal_order_type(self, order_type: OrderType) -> BinanceOrderType:
        try:
            return self.int_order_type_to_ext_order_type[order_type]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized internal order type, was {order_type}",  # pragma: no cover
            )

    def parse_binance_trigger_type(self, trigger_type: str) -> TriggerType:
        # Replace method in child class, if compatible
        raise RuntimeError(  # pragma: no cover (design-time error)
            "Cannot parse binance trigger type (not implemented).",  # pragma: no cover
        )
