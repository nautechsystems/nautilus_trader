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
Define common enumerations for the dYdX adapter.
"""

from enum import Enum
from enum import unique

from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce


@unique
class DYDXChannel(Enum):
    """
    Represents a dYdX websocket channel.
    """

    TRADES = "v4_trades"
    ORDERBOOK = "v4_orderbook"
    CANDLES = "v4_candles"
    MARKETS = "v4_markets"
    SUBACCOUNTS = "v4_subaccounts"
    BLOCK_HEIGHT = "v4_block_height"


@unique
class DYDXLiquidity(Enum):
    """
    Represents a dYdX liquidity type.
    """

    TAKER = "TAKER"
    MAKER = "MAKER"


@unique
class DYDXFillType(Enum):
    """
    Represents a dYdX fill type.
    """

    LIMIT = "LIMIT"
    LIQUIDATED = "LIQUIDATED"
    LIQUIDATION = "LIQUIDATION"
    DELEVERAGED = "DELEVERAGED"
    OFFSETTING = "OFFSETTING"


@unique
class DYDXMarketType(Enum):
    """
    Represents a dYdX market type.
    """

    PERPETUAL = "PERPETUAL"
    SPOT = "SPOT"


@unique
class DYDXPerpetualPositionStatus(Enum):
    """
    Represents a dYdX position status.
    """

    OPEN = "OPEN"
    CLOSED = "CLOSED"
    LIQUIDATED = "LIQUIDATED"


@unique
class DYDXOrderStatus(Enum):
    """
    Represents a dYdX order status.
    """

    OPEN = "OPEN"
    FILLED = "FILLED"
    CANCELED = "CANCELED"
    BEST_EFFORT_CANCELED = "BEST_EFFORT_CANCELED"
    UNTRIGGERED = "UNTRIGGERED"
    BEST_EFFORT_OPENED = "BEST_EFFORT_OPENED"


@unique
class DYDXTimeInForce(Enum):
    """
    Represents a dYdX time in force setting.
    """

    # GTT represents Good Til Time, where an order will first match with existing orders on the book
    # and any remaining size will be added to the book as a maker order, which will expire at a
    # given expiry time.
    GTT = "GTT"  # Good Til Time

    # FOK represents Fill or KILl where it's enforced that an order will either be filled
    # completely and immediately by maker orders on the book or canceled if the entire amount can't
    # be filled.
    FOK = "FOK"  # Fill or Kill

    # IOC represents Immediate or Cancel, where it's enforced that an order only be matched with
    # maker orders on the book. If the order has remaining size after matching with existing orders
    # on the book, the remaining size is not placed on the book.
    IOC = "IOC"  # Immediate or Cancel


@unique
class DYDXPositionSide(Enum):
    """
    Represents a dYdX position side.
    """

    LONG = "LONG"
    SHORT = "SHORT"


@unique
class DYDXEndpointType(Enum):
    """
    Represents a dYdX endpoint perpetual market status.
    """

    NONE = "NONE"
    ASSET = "ASSET"
    MARKET = "MARKET"
    ACCOUNT = "ACCOUNT"
    TRADE = "TRADE"
    POSITION = "POSITION"


@unique
class DYDXPerpetualMarketStatus(Enum):
    """
    Represents a dYdX endpoint perpetual market status.
    """

    ACTIVE = "ACTIVE"
    PAUSED = "PAUSED"
    CANCEL_ONLY = "CANCEL_ONLY"
    POST_ONLY = "POST_ONLY"
    INITIALIZING = "INITIALIZING"
    FINAL_SETTLEMENT = "FINAL_SETTLEMENT"


@unique
class DYDXPerpetualMarketType(Enum):
    """
    Represents a dYdX endpoint perpetual market type.
    """

    CROSS = "CROSS"
    ISOLATED = "ISOLATED"


@unique
class DYDXOrderSide(Enum):
    """
    Represents a dYdX order side type.
    """

    BUY = "BUY"
    SELL = "SELL"


@unique
class DYDXOrderType(Enum):
    """
    Represents a dYdX order type.
    """

    LIMIT = "LIMIT"
    MARKET = "MARKET"
    STOP_LIMIT = "STOP_LIMIT"
    STOP_MARKET = "STOP_MARKET"
    TRAILING_STOP = "TRAILING_STOP"
    TAKE_PROFIT = "TAKE_PROFIT"
    TAKE_PROFIT_MARKET = "TAKE_PROFIT_MARKET"
    LIQUIDATED = "LIQUIDATED"
    DELEVERAGED = "DELEVERAGED"


@unique
class DYDXCandlesResolution(Enum):
    """
    Represent the kline resolution for dYdX.
    """

    ONE_MINUTE = "1MIN"
    FIVE_MINUTES = "5MINS"
    FIFTEEN_MINUTES = "15MINS"
    THIRTY_MINUTES = "30MINS"
    ONE_HOUR = "1HOUR"
    FOUR_HOURS = "4HOURS"
    ONE_DAY = "1DAY"


@unique
class DYDXTransferType(Enum):
    """
    Represent the different transfer types.
    """

    TRANSFER_IN = "TRANSFER_IN"
    TRANSFER_OUT = "TRANSFER_OUT"
    DEPOSIT = "DEPOSIT"
    WITHDRAWAL = "WITHDRAWAL"


class DYDXEnumParser:
    """
    Convert dYdX enums to Nautilus enums.
    """

    def __init__(self) -> None:
        """
        Create a new instance of the dYdX enum parser.
        """
        self.dydx_to_nautilus_order_type = {
            DYDXOrderType.LIMIT: OrderType.LIMIT,
            DYDXOrderType.MARKET: OrderType.MARKET,
            DYDXOrderType.STOP_LIMIT: OrderType.STOP_LIMIT,
            DYDXOrderType.STOP_MARKET: OrderType.STOP_MARKET,
        }

        self.nautilus_to_dydx_order_type = {
            value: key for key, value in self.dydx_to_nautilus_order_type.items()
        }

        self.dydx_to_nautilus_order_side = {
            DYDXOrderSide.BUY: OrderSide.BUY,
            DYDXOrderSide.SELL: OrderSide.SELL,
        }

        self.nautilus_to_dydx_order_side = {
            value: key for key, value in self.dydx_to_nautilus_order_side.items()
        }

        self.dydx_to_nautilus_order_status = {
            DYDXOrderStatus.OPEN: OrderStatus.ACCEPTED,
            DYDXOrderStatus.FILLED: OrderStatus.FILLED,
            DYDXOrderStatus.CANCELED: OrderStatus.CANCELED,
            DYDXOrderStatus.BEST_EFFORT_CANCELED: OrderStatus.PENDING_CANCEL,
            DYDXOrderStatus.BEST_EFFORT_OPENED: OrderStatus.ACCEPTED,
            DYDXOrderStatus.UNTRIGGERED: OrderStatus.ACCEPTED,
        }

        self.dydx_to_nautilus_time_in_force = {
            DYDXTimeInForce.GTT: TimeInForce.GTD,
            DYDXTimeInForce.FOK: TimeInForce.FOK,
            DYDXTimeInForce.IOC: TimeInForce.IOC,
        }

        self.dydx_to_nautilus_liquidity_side = {
            DYDXLiquidity.MAKER: LiquiditySide.MAKER,
            DYDXLiquidity.TAKER: LiquiditySide.TAKER,
        }

        self.dydx_to_nautilus_position_side = {
            DYDXPositionSide.LONG: PositionSide.LONG,
            DYDXPositionSide.SHORT: PositionSide.SHORT,
        }

        self.bar_type_to_dydx_candle_resolution_map = {
            (BarAggregation.MINUTE, 1): DYDXCandlesResolution.ONE_MINUTE,
            (BarAggregation.MINUTE, 5): DYDXCandlesResolution.FIVE_MINUTES,
            (BarAggregation.MINUTE, 15): DYDXCandlesResolution.FIFTEEN_MINUTES,
            (BarAggregation.MINUTE, 30): DYDXCandlesResolution.THIRTY_MINUTES,
            (BarAggregation.MINUTE, 60): DYDXCandlesResolution.ONE_HOUR,
            (BarAggregation.MINUTE, 240): DYDXCandlesResolution.FOUR_HOURS,
            (BarAggregation.HOUR, 1): DYDXCandlesResolution.ONE_HOUR,
            (BarAggregation.HOUR, 4): DYDXCandlesResolution.FOUR_HOURS,
            (BarAggregation.HOUR, 24): DYDXCandlesResolution.ONE_DAY,
            (BarAggregation.DAY, 1): DYDXCandlesResolution.ONE_DAY,
        }

    def parse_dydx_order_type(self, order_type: DYDXOrderType) -> OrderType:
        """
        Convert a DYDX order type to a Nautilus order type.
        """
        return self.dydx_to_nautilus_order_type[order_type]

    def parse_nautilus_order_type(self, order_type: OrderType) -> DYDXOrderType:
        """
        Convert a Nautilus order type to a DYDX order type.
        """
        return self.nautilus_to_dydx_order_type[order_type]

    def parse_dydx_order_side(self, order_side: DYDXOrderSide | None) -> OrderSide:
        """
        Convert a DYDX order side to a Nautilus order side.
        """
        if order_side is None:
            return OrderSide.NO_ORDER_SIDE

        return self.dydx_to_nautilus_order_side[order_side]

    def parse_nautilus_order_side(self, order_side: OrderSide) -> DYDXOrderSide | None:
        """
        Convert a Nautilus order side to a DYDX order side.
        """
        if order_side == OrderSide.NO_ORDER_SIDE:
            return None

        return self.nautilus_to_dydx_order_side[order_side]

    def parse_dydx_order_status(self, order_status: DYDXOrderStatus) -> OrderStatus:
        """
        Convert a DYDX order status to a Nautilus order status.
        """
        return self.dydx_to_nautilus_order_status[order_status]

    def parse_dydx_time_in_force(self, time_in_force: DYDXTimeInForce) -> TimeInForce:
        """
        Convert a DYDX time in force to a Nautilus time in force.
        """
        return self.dydx_to_nautilus_time_in_force[time_in_force]

    def parse_dydx_liquidity_side(self, liquidity_side: DYDXLiquidity) -> LiquiditySide:
        """
        Convert a DYDX liquidity side to a Nautilus liquidity side.
        """
        return self.dydx_to_nautilus_liquidity_side[liquidity_side]

    def parse_dydx_position_side(self, position_side: DYDXPositionSide) -> PositionSide:
        """
        Convert a DYDX position side to a Nautilus position side.
        """
        return self.dydx_to_nautilus_position_side[position_side]

    def parse_dydx_kline(self, bar_type: BarType) -> DYDXCandlesResolution:
        """
        Convert a Nautilus bar type to a dYdX candles resolution.
        """
        aggregation: BarAggregation = bar_type.spec.aggregation
        return self.bar_type_to_dydx_candle_resolution_map[(aggregation, bar_type.spec.step)]
