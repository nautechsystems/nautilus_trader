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
"""
Defines Binance Spot/Margin specific enums.

References
----------
https://binance-docs.github.io/apidocs/spot/en/#public-api-definitions

"""
from enum import Enum
from enum import unique

from nautilus_trader.adapters.binance.common.enums import BinanceEnumParser
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import order_type_to_str
from nautilus_trader.model.orders import Order


@unique
class BinanceSpotPermissions(Enum):
    """
    Represents Binance Spot/Margin trading permissions.
    """

    SPOT = "SPOT"
    MARGIN = "MARGIN"
    LEVERAGED = "LEVERAGED"
    TRD_GRP_002 = "TRD_GRP_002"
    TRD_GRP_003 = "TRD_GRP_003"
    TRD_GRP_004 = "TRD_GRP_004"
    TRD_GRP_005 = "TRD_GRP_005"
    TRD_GRP_006 = "TRD_GRP_006"
    TRD_GRP_007 = "TRD_GRP_007"
    TRD_GRP_008 = "TRD_GRP_008"
    TRD_GRP_009 = "TRD_GRP_009"
    TRD_GRP_010 = "TRD_GRP_010"
    TRD_GRP_011 = "TRD_GRP_011"
    TRD_GRP_012 = "TRD_GRP_012"
    TRD_GRP_013 = "TRD_GRP_013"
    TRD_GRP_014 = "TRD_GRP_014"
    TRD_GRP_015 = "TRD_GRP_015"
    TRD_GRP_016 = "TRD_GRP_016"
    TRD_GRP_017 = "TRD_GRP_017"
    TRD_GRP_018 = "TRD_GRP_018"
    TRD_GRP_019 = "TRD_GRP_019"
    TRD_GRP_020 = "TRD_GRP_020"
    TRD_GRP_021 = "TRD_GRP_021"
    TRD_GRP_022 = "TRD_GRP_022"
    TRD_GRP_023 = "TRD_GRP_023"
    TRD_GRP_024 = "TRD_GRP_024"
    TRD_GRP_025 = "TRD_GRP_025"
    TRD_GRP_026 = "TRD_GRP_026"
    TRD_GRP_027 = "TRD_GRP_027"
    TRD_GRP_028 = "TRD_GRP_028"
    TRD_GRP_029 = "TRD_GRP_029"
    TRD_GRP_030 = "TRD_GRP_030"
    TRD_GRP_031 = "TRD_GRP_031"
    TRD_GRP_032 = "TRD_GRP_032"


@unique
class BinanceSpotSymbolStatus(Enum):
    """
    Represents a Binance Spot/Margin symbol status.
    """

    PRE_TRADING = "PRE_TRADING"
    TRADING = "TRADING"
    POST_TRADING = "POST_TRADING"
    END_OF_DAY = "END_OF_DAY"
    HALT = "HALT"
    AUCTION_MATCH = "AUCTION_MATCH"
    BREAK = "BREAK"


@unique
class BinanceSpotEventType(Enum):
    """
    Represents a Binance Spot/Margin event type.
    """

    outboundAccountPosition = "outboundAccountPosition"
    balanceUpdate = "balanceUpdate"
    executionReport = "executionReport"
    listStatus = "listStatus"


class BinanceSpotEnumParser(BinanceEnumParser):
    """
    Provides parsing methods for enums used by the 'Binance Spot/Margin' exchange.
    """

    def __init__(self) -> None:
        super().__init__()

        # Spot specific order type conversion
        self.spot_ext_to_int_order_type = {
            BinanceOrderType.LIMIT: OrderType.LIMIT,
            BinanceOrderType.MARKET: OrderType.MARKET,
            BinanceOrderType.STOP: OrderType.STOP_MARKET,
            BinanceOrderType.STOP_LOSS: OrderType.STOP_MARKET,
            BinanceOrderType.STOP_LOSS_LIMIT: OrderType.STOP_LIMIT,
            BinanceOrderType.TAKE_PROFIT: OrderType.LIMIT,
            BinanceOrderType.TAKE_PROFIT_LIMIT: OrderType.STOP_LIMIT,
            BinanceOrderType.LIMIT_MAKER: OrderType.LIMIT,
        }

        self.spot_valid_time_in_force = {
            TimeInForce.GTC,
            TimeInForce.GTD,  # Will be transformed to GTC with warning
            TimeInForce.FOK,
            TimeInForce.IOC,
        }

        self.spot_valid_order_types = {
            OrderType.MARKET,
            OrderType.LIMIT,
            OrderType.LIMIT_IF_TOUCHED,
            OrderType.STOP_LIMIT,
        }

    def parse_binance_order_type(self, order_type: BinanceOrderType) -> OrderType:
        try:
            return self.spot_ext_to_int_order_type[order_type]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Binance Spot/Margin order type, was {order_type}",  # pragma: no cover
            )

    def parse_internal_order_type(self, order: Order) -> BinanceOrderType:
        if order.order_type == OrderType.MARKET:
            return BinanceOrderType.MARKET
        elif order.order_type == OrderType.LIMIT:
            if order.is_post_only:
                return BinanceOrderType.LIMIT_MAKER
            else:
                return BinanceOrderType.LIMIT
        elif order.order_type == OrderType.STOP_LIMIT:
            return BinanceOrderType.STOP_LOSS_LIMIT
        elif order.order_type == OrderType.LIMIT_IF_TOUCHED:
            return BinanceOrderType.TAKE_PROFIT_LIMIT
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid or unsupported `OrderType`, was {order_type_to_str(order.order_type)}",  # pragma: no cover
            )
