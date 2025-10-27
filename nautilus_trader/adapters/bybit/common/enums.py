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

from __future__ import annotations

from enum import Enum
from enum import unique
from typing import TYPE_CHECKING

from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import time_in_force_to_str


if TYPE_CHECKING:
    from nautilus_trader.model.data import BarType


def raise_error(error):
    raise error


@unique
class BybitUnifiedMarginStatus(Enum):
    CLASSIC_ACCOUNT = 1  # Classic account
    UNIFIED_TRADING_ACCOUNT_1_0 = 3  # Unified trading account 1.0
    UNIFIED_TRADING_ACCOUNT_1_0_PRO = 4  # Unified trading account 1.0 (pro version)
    UNIFIED_TRADING_ACCOUNT_2_0 = 5  # Unified trading account 2.0
    UNIFIED_TRADING_ACCOUNT_2_0_PRO = 6  # Unified trading account 2.0 (pro version)


@unique
class BybitMarginMode(Enum):
    ISOLATED_MARGIN = "ISOLATED_MARGIN"
    REGULAR_MARGIN = "REGULAR_MARGIN"
    PORTFOLIO_MARGIN = "PORTFOLIO_MARGIN"


@unique
class BybitPositionMode(Enum):
    """
    https://bybit-exchange.github.io/docs/v5/position/position-mode
    """

    MERGED_SINGLE = 0
    BOTH_SIDES = 3


@unique
class BybitPositionIdx(Enum):
    # One-way mode position
    ONE_WAY = 0
    # Buy side of hedge-mode position
    BUY_HEDGE = 1
    # Sell side of hedge-mode position
    SELL_HEDGE = 2


@unique
class BybitAccountType(Enum):
    UNIFIED = "UNIFIED"


@unique
class BybitProductType(Enum):
    SPOT = "spot"
    LINEAR = "linear"
    INVERSE = "inverse"
    OPTION = "option"

    @property
    def is_spot(self) -> bool:
        return self == BybitProductType.SPOT

    @property
    def is_linear(self) -> bool:
        return self == BybitProductType.LINEAR

    @property
    def is_inverse(self) -> bool:
        return self == BybitProductType.INVERSE

    @property
    def is_option(self) -> bool:
        return self == BybitProductType.OPTION


@unique
class BybitContractType(Enum):
    LINEAR_PERPETUAL = "LinearPerpetual"
    LINEAR_FUTURE = "LinearFutures"
    INVERSE_PERPETUAL = "InversePerpetual"
    INVERSE_FUTURE = "InverseFutures"


@unique
class BybitOptionType(Enum):
    CALL = "Call"
    PUT = "Put"


@unique
class BybitPositionSide(Enum):
    FLAT = ""
    BUY = "Buy"
    SELL = "Sell"

    def parse_to_position_side(self) -> PositionSide:
        if self == BybitPositionSide.FLAT:
            return PositionSide.FLAT
        elif self == BybitPositionSide.BUY:
            return PositionSide.LONG
        elif self == BybitPositionSide.SELL:
            return PositionSide.SHORT
        raise RuntimeError(f"invalid position side, was {self}")


@unique
class BybitWsOrderRequestMsgOP(Enum):
    CREATE = "order.create"
    AMEND = "order.amend"
    CANCEL = "order.cancel"
    CREATE_BATCH = "order.create-batch"
    AMEND_BATCH = "order.amend-batch"
    CANCEL_BATCH = "order.cancel-batch"


@unique
class BybitKlineInterval(Enum):
    MINUTE_1 = "1"
    MINUTE_3 = "3"
    MINUTE_5 = "5"
    MINUTE_15 = "15"
    MINUTE_30 = "30"
    HOUR_1 = "60"
    HOUR_2 = "120"
    HOUR_4 = "240"
    HOUR_6 = "360"
    HOUR_12 = "720"
    DAY_1 = "D"
    WEEK_1 = "W"
    MONTH_1 = "M"


@unique
class BybitOrderStatus(Enum):
    CREATED = "Created"
    NEW = "New"
    REJECTED = "Rejected"
    PARTIALLY_FILLED = "PartiallyFilled"
    PARTIALLY_FILLED_CANCELED = "PartiallyFilledCanceled"
    FILLED = "Filled"
    CANCELED = "Cancelled"
    UNTRIGGERED = "Untriggered"
    TRIGGERED = "Triggered"
    DEACTIVATED = "Deactivated"


@unique
class BybitOrderSide(Enum):
    UNKNOWN = ""  # It will be an empty string in some cases
    BUY = "Buy"
    SELL = "Sell"


@unique
class BybitOrderType(Enum):
    MARKET = "Market"
    LIMIT = "Limit"
    UNKNOWN = "UNKNOWN"  # Used when execution type is Funding


@unique
class BybitStopOrderType(Enum):
    """
    https://bybit-exchange.github.io/docs/v5/enum#stopordertype
    """

    NONE = ""  # Default
    UNKNOWN = "UNKNOWN"  # Classic account value
    TAKE_PROFIT = "TakeProfit"
    STOP_LOSS = "StopLoss"
    TRAILING_STOP = "TrailingStop"
    STOP = "Stop"
    PARTIAL_TAKE_PROFIT = "PartialTakeProfit"
    PARTIAL_STOP_LOSS = "PartialStopLoss"
    TPSL_ORDER = "tpslOrder"
    OCO_ORDER = "OcoOrder"  # Spot only
    MM_RATE_CLOSE = "MmRateClose"
    BIDIRECTIONAL_TPSL_ORDER = "BidirectionalTpslOrder"


@unique
class BybitTriggerType(Enum):
    NONE = ""  # Default
    LAST_PRICE = "LastPrice"
    INDEX_PRICE = "IndexPrice"
    MARK_PRICE = "MarkPrice"


@unique
class BybitTriggerDirection(Enum):
    NONE = 0
    RISES_TO = 1  # Triggered when market price rises to triggerPrice
    FALLS_TO = 2  # Triggered when market price falls to triggerPrice


@unique
class BybitTpSlMode(Enum):
    FULL = "Full"  # Entire position for TP/SL
    PARTIAL = "Partial"  # Partial position: must be used for Limit TP/SL


@unique
class BybitTimeInForce(Enum):
    GTC = "GTC"
    IOC = "IOC"
    FOK = "FOK"
    POST_ONLY = "PostOnly"


@unique
class BybitExecType(Enum):
    TRADE = "Trade"
    ADL_TRADE = "AdlTrade"  # Auto-Deleveraging
    FUNDING = "Funding"  # Funding fee
    BUST_TRADE = "BustTrade"  # Liquidation
    DELIVERY = "Delivery"  # Delivery
    SETTLE = "Settle"  # Settle Inverse futures settlement
    BLOCK_TRADE = "BlockTrade"
    MOVE_POSITION = "MovePosition"
    UNKNOWN = "UNKNOWN"  # Classic account value (cannot be used to query)


@unique
class BybitTransactionType(Enum):
    # Assets that transferred into Unified wallet
    TRANSFER_IN = "TRANSFER_IN"
    # Assets that transferred out of Unified wallet
    TRANSFER_OUT = "TRANSFER_OUT"
    TRADE = "TRADE"
    SETTLEMENT = "SETTLEMENT"
    DELIVERY = "DELIVERY"
    LIQUIDATION = "LIQUIDATION"
    AIRDROP = "AIRDRP"


@unique
class BybitEndpointType(Enum):
    NONE = "NONE"
    ASSET = "ASSET"
    MARKET = "MARKET"
    ACCOUNT = "ACCOUNT"
    TRADE = "TRADE"
    POSITION = "POSITION"
    USER = "USER"


def check_dict_keys(key, data):
    try:
        return data[key]
    except KeyError as e:
        raise RuntimeError(
            f"Unrecognized Bybit {key} not found in {data}",
        ) from e


class BybitEnumParser:
    def __init__(self) -> None:
        self.bybit_to_nautilus_order_side = {
            BybitOrderSide.BUY: OrderSide.BUY,
            BybitOrderSide.SELL: OrderSide.SELL,
        }
        self.nautilus_to_bybit_order_side = {
            b: a for a, b in self.bybit_to_nautilus_order_side.items()
        }
        self.bybit_to_nautilus_order_type = {
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.NONE,
                BybitOrderSide.BUY,
                BybitTriggerDirection.NONE,
            ): OrderType.MARKET,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.NONE,
                BybitOrderSide.SELL,
                BybitTriggerDirection.NONE,
            ): OrderType.MARKET,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.UNKNOWN,
                BybitOrderSide.BUY,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.MARKET,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.UNKNOWN,
                BybitOrderSide.BUY,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.MARKET,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.UNKNOWN,
                BybitOrderSide.SELL,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.MARKET,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.UNKNOWN,
                BybitOrderSide.SELL,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.MARKET,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.NONE,
                BybitOrderSide.BUY,
                BybitTriggerDirection.NONE,
            ): OrderType.LIMIT,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.NONE,
                BybitOrderSide.SELL,
                BybitTriggerDirection.NONE,
            ): OrderType.LIMIT,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.STOP,
                BybitOrderSide.BUY,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.MARKET_IF_TOUCHED,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.STOP,
                BybitOrderSide.SELL,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.MARKET_IF_TOUCHED,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.STOP,
                BybitOrderSide.BUY,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.STOP_MARKET,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.STOP,
                BybitOrderSide.SELL,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.STOP_MARKET,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.STOP,
                BybitOrderSide.BUY,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.LIMIT_IF_TOUCHED,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.STOP,
                BybitOrderSide.SELL,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.LIMIT_IF_TOUCHED,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.UNKNOWN,
                BybitOrderSide.BUY,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.LIMIT_IF_TOUCHED,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.UNKNOWN,
                BybitOrderSide.BUY,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.LIMIT_IF_TOUCHED,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.UNKNOWN,
                BybitOrderSide.SELL,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.LIMIT_IF_TOUCHED,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.UNKNOWN,
                BybitOrderSide.SELL,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.LIMIT_IF_TOUCHED,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.STOP,
                BybitOrderSide.BUY,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.STOP_LIMIT,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.STOP,
                BybitOrderSide.SELL,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.STOP_LIMIT,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.PARTIAL_STOP_LOSS,
                BybitOrderSide.BUY,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.STOP_LIMIT,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.PARTIAL_STOP_LOSS,
                BybitOrderSide.SELL,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.STOP_LIMIT,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.TRAILING_STOP,
                BybitOrderSide.BUY,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.TRAILING_STOP_MARKET,
            (
                BybitOrderType.MARKET,
                BybitStopOrderType.TRAILING_STOP,
                BybitOrderSide.SELL,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.TRAILING_STOP_MARKET,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.TRAILING_STOP,
                BybitOrderSide.BUY,
                BybitTriggerDirection.RISES_TO,
            ): OrderType.TRAILING_STOP_LIMIT,
            (
                BybitOrderType.LIMIT,
                BybitStopOrderType.TRAILING_STOP,
                BybitOrderSide.SELL,
                BybitTriggerDirection.FALLS_TO,
            ): OrderType.TRAILING_STOP_LIMIT,
        }

        # TODO check time in force mapping
        self.bybit_to_nautilus_time_in_force = {
            BybitTimeInForce.GTC: TimeInForce.GTC,
            BybitTimeInForce.IOC: TimeInForce.IOC,
            BybitTimeInForce.FOK: TimeInForce.FOK,
            BybitTimeInForce.POST_ONLY: TimeInForce.GTC,
        }
        self.nautilus_to_bybit_time_in_force = {
            TimeInForce.GTC: BybitTimeInForce.GTC,
            TimeInForce.IOC: BybitTimeInForce.IOC,
            TimeInForce.FOK: BybitTimeInForce.FOK,
        }


        self.bybit_to_nautilus_order_status = {
            (OrderType.MARKET, BybitOrderStatus.CREATED): OrderStatus.SUBMITTED,
            (OrderType.MARKET, BybitOrderStatus.NEW): OrderStatus.ACCEPTED,
            (OrderType.MARKET, BybitOrderStatus.REJECTED): OrderStatus.REJECTED,
            (OrderType.MARKET, BybitOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.MARKET, BybitOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.MARKET, BybitOrderStatus.PARTIALLY_FILLED_CANCELED): OrderStatus.CANCELED,
            (OrderType.MARKET, BybitOrderStatus.FILLED): OrderStatus.FILLED,

            (OrderType.LIMIT, BybitOrderStatus.CREATED): OrderStatus.SUBMITTED,
            (OrderType.LIMIT, BybitOrderStatus.NEW): OrderStatus.ACCEPTED,
            (OrderType.LIMIT, BybitOrderStatus.REJECTED): OrderStatus.REJECTED,
            (OrderType.LIMIT, BybitOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.LIMIT, BybitOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.LIMIT, BybitOrderStatus.PARTIALLY_FILLED_CANCELED): OrderStatus.CANCELED,
            (OrderType.LIMIT, BybitOrderStatus.FILLED): OrderStatus.FILLED,

            (OrderType.MARKET_IF_TOUCHED, BybitOrderStatus.CREATED): OrderStatus.SUBMITTED,
            (OrderType.MARKET_IF_TOUCHED, BybitOrderStatus.NEW): OrderStatus.ACCEPTED,
            (OrderType.MARKET_IF_TOUCHED, BybitOrderStatus.REJECTED): OrderStatus.REJECTED,
            (OrderType.MARKET_IF_TOUCHED, BybitOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.MARKET_IF_TOUCHED, BybitOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            (OrderType.MARKET_IF_TOUCHED, BybitOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            (OrderType.MARKET_IF_TOUCHED, BybitOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.MARKET_IF_TOUCHED, BybitOrderStatus.PARTIALLY_FILLED_CANCELED): OrderStatus.CANCELED,
            (OrderType.MARKET_IF_TOUCHED, BybitOrderStatus.FILLED): OrderStatus.FILLED,

            (OrderType.LIMIT_IF_TOUCHED, BybitOrderStatus.CREATED): OrderStatus.SUBMITTED,
            (OrderType.LIMIT_IF_TOUCHED, BybitOrderStatus.NEW): OrderStatus.ACCEPTED,
            (OrderType.LIMIT_IF_TOUCHED, BybitOrderStatus.REJECTED): OrderStatus.REJECTED,
            (OrderType.LIMIT_IF_TOUCHED, BybitOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.LIMIT_IF_TOUCHED, BybitOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            (OrderType.LIMIT_IF_TOUCHED, BybitOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            (OrderType.LIMIT_IF_TOUCHED, BybitOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.LIMIT_IF_TOUCHED, BybitOrderStatus.PARTIALLY_FILLED_CANCELED): OrderStatus.CANCELED,
            (OrderType.LIMIT_IF_TOUCHED, BybitOrderStatus.FILLED): OrderStatus.FILLED,

            (OrderType.STOP_MARKET, BybitOrderStatus.CREATED): OrderStatus.SUBMITTED,
            (OrderType.STOP_MARKET, BybitOrderStatus.NEW): OrderStatus.ACCEPTED,
            (OrderType.STOP_MARKET, BybitOrderStatus.REJECTED): OrderStatus.REJECTED,
            (OrderType.STOP_MARKET, BybitOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.STOP_MARKET, BybitOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            (OrderType.STOP_MARKET, BybitOrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
            (OrderType.STOP_MARKET, BybitOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            (OrderType.STOP_MARKET, BybitOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.STOP_MARKET, BybitOrderStatus.PARTIALLY_FILLED_CANCELED): OrderStatus.CANCELED,
            (OrderType.STOP_MARKET, BybitOrderStatus.FILLED): OrderStatus.FILLED,

            (OrderType.STOP_LIMIT, BybitOrderStatus.CREATED): OrderStatus.SUBMITTED,
            (OrderType.STOP_LIMIT, BybitOrderStatus.NEW): OrderStatus.ACCEPTED,
            (OrderType.STOP_LIMIT, BybitOrderStatus.REJECTED): OrderStatus.REJECTED,
            (OrderType.STOP_LIMIT, BybitOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.STOP_LIMIT, BybitOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            (OrderType.STOP_LIMIT, BybitOrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
            (OrderType.STOP_LIMIT, BybitOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            (OrderType.STOP_LIMIT, BybitOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.STOP_LIMIT, BybitOrderStatus.PARTIALLY_FILLED_CANCELED): OrderStatus.CANCELED,
            (OrderType.STOP_LIMIT, BybitOrderStatus.FILLED): OrderStatus.FILLED,

            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.CREATED): OrderStatus.SUBMITTED,
            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.NEW): OrderStatus.ACCEPTED,
            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.REJECTED): OrderStatus.REJECTED,
            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.PARTIALLY_FILLED_CANCELED): OrderStatus.CANCELED,
            (OrderType.TRAILING_STOP_MARKET, BybitOrderStatus.FILLED): OrderStatus.FILLED,

            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.CREATED): OrderStatus.SUBMITTED,
            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.NEW): OrderStatus.ACCEPTED,
            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.REJECTED): OrderStatus.REJECTED,
            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.PARTIALLY_FILLED_CANCELED): OrderStatus.CANCELED,
            (OrderType.TRAILING_STOP_LIMIT, BybitOrderStatus.FILLED): OrderStatus.FILLED,
        }

        self.bybit_to_nautilus_trigger_type = {
            BybitTriggerType.NONE: TriggerType.NO_TRIGGER,
            BybitTriggerType.LAST_PRICE: TriggerType.LAST_PRICE,
            BybitTriggerType.MARK_PRICE: TriggerType.MARK_PRICE,
            BybitTriggerType.INDEX_PRICE: TriggerType.INDEX_PRICE,
        }
        self.nautilus_to_bybit_trigger_type = {
            b: a for a, b in self.bybit_to_nautilus_trigger_type.items()
        }
        self.nautilus_to_bybit_trigger_type[TriggerType.DEFAULT] = BybitTriggerType.LAST_PRICE

        # klines
        self.minute_klines_interval = [1, 3, 5, 15, 30]
        self.hour_klines_interval = [1, 2, 4, 6, 12]
        self.aggregation_kline_mapping = {
            BarAggregation.MINUTE: lambda x: BybitKlineInterval(f"{x}"),
            BarAggregation.HOUR: lambda x: BybitKlineInterval(f"{x * 60}"),
            BarAggregation.DAY: lambda x: (
                BybitKlineInterval("D")
                if x == 1
                else raise_error(ValueError(f"Bybit incorrect day kline interval {x}"))
            ),
            BarAggregation.WEEK: lambda x: (
                BybitKlineInterval("W")
                if x == 1
                else raise_error(ValueError(f"Bybit incorrect week kline interval {x}"))
            ),
            BarAggregation.MONTH: lambda x: (
                BybitKlineInterval("M")
                if x == 1
                else raise_error(ValueError(f"Bybit incorrect month kline interval {x}"))
            ),
        }
        self.valid_time_in_force = {
            TimeInForce.GTC,
            TimeInForce.IOC,
            TimeInForce.FOK,
        }

        # trigger direction
        self.trigger_direction_map_buy = {
            OrderType.STOP_MARKET: BybitTriggerDirection.RISES_TO,
            OrderType.STOP_LIMIT: BybitTriggerDirection.RISES_TO,
            OrderType.MARKET_IF_TOUCHED: BybitTriggerDirection.RISES_TO,
            OrderType.TRAILING_STOP_MARKET: BybitTriggerDirection.RISES_TO,
            OrderType.LIMIT_IF_TOUCHED: BybitTriggerDirection.FALLS_TO,
        }

        self.trigger_direction_map_sell = {
            OrderType.STOP_MARKET: BybitTriggerDirection.FALLS_TO,
            OrderType.STOP_LIMIT: BybitTriggerDirection.FALLS_TO,
            OrderType.MARKET_IF_TOUCHED: BybitTriggerDirection.FALLS_TO,
            OrderType.TRAILING_STOP_MARKET: BybitTriggerDirection.FALLS_TO,
            OrderType.LIMIT_IF_TOUCHED: BybitTriggerDirection.RISES_TO,
        }

    def parse_bybit_order_status(
        self,
        order_type: OrderType,
        order_status: BybitOrderStatus,
    ) -> OrderStatus:
        return check_dict_keys((order_type, order_status), self.bybit_to_nautilus_order_status)

    def parse_bybit_time_in_force(self, time_in_force: BybitTimeInForce) -> TimeInForce:
        return check_dict_keys(time_in_force, self.bybit_to_nautilus_time_in_force)

    def parse_bybit_order_side(self, order_side: BybitOrderSide) -> OrderSide:
        return check_dict_keys(order_side, self.bybit_to_nautilus_order_side)

    def parse_nautilus_order_side(self, order_side: OrderSide) -> BybitOrderSide:
        return check_dict_keys(order_side, self.nautilus_to_bybit_order_side)

    def parse_bybit_order_type(
        self,
        order_type: BybitOrderType,
        stop_order_type: BybitStopOrderType,
        order_side: BybitOrderSide,
        trigger_direction: BybitTriggerDirection,
    ) -> OrderType:
        return check_dict_keys(
            (order_type, stop_order_type, order_side, trigger_direction),
            self.bybit_to_nautilus_order_type,
        )

    def parse_nautilus_time_in_force(self, time_in_force: TimeInForce) -> BybitTimeInForce:
        try:
            return self.nautilus_to_bybit_time_in_force[time_in_force]
        except KeyError as e:
            raise RuntimeError(
                f"unrecognized Bybit time in force, was {time_in_force_to_str(time_in_force)}",  # pragma: no cover
            ) from e

    def parse_nautilus_trigger_type(self, trigger_type: TriggerType) -> BybitTriggerType:
        return check_dict_keys(trigger_type, self.nautilus_to_bybit_trigger_type)

    def parse_bybit_trigger_type(self, trigger_type: BybitTriggerType) -> TriggerType:
        return check_dict_keys(trigger_type, self.bybit_to_nautilus_trigger_type)

    def parse_trigger_direction(
        self,
        order_type: OrderType,
        order_side: OrderSide,
    ) -> BybitTriggerDirection | None:
        return (
            self.trigger_direction_map_buy.get(order_type)
            if order_side == OrderSide.BUY
            else self.trigger_direction_map_sell.get(order_type)
        )

    def parse_bybit_kline(self, bar_type: BarType) -> BybitKlineInterval:
        try:
            aggregation = bar_type.spec.aggregation
            interval = int(bar_type.spec.step)
            if aggregation in self.aggregation_kline_mapping:
                result = self.aggregation_kline_mapping[aggregation](interval)
                return result
            else:
                raise ValueError(
                    f"Bybit incorrect aggregation {aggregation}",  # pragma: no cover
                )
        except KeyError as e:
            raise RuntimeError(
                f"unrecognized Bybit bar type, was {bar_type}",  # pragma: no cover
            ) from e
