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

from enum import Enum
from enum import unique

from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import bar_aggregation_to_str
from nautilus_trader.model.orders.base import Order


"""
Defines `Binance` common enums.


References
----------
https://binance-docs.github.io/apidocs/spot/en/#public-api-definitions
https://binance-docs.github.io/apidocs/futures/en/#public-endpoints-info
"""


@unique
class BinanceRateLimitType(Enum):
    """Represents a `Binance` rate limit type."""

    REQUEST_WEIGHT = "REQUEST_WEIGHT"
    ORDERS = "ORDERS"
    RAW_REQUESTS = "RAW_REQUESTS"


@unique
class BinanceRateLimitInterval(Enum):
    """Represents a `Binance` rate limit interval."""

    SECOND = "SECOND"
    MINUTE = "MINUTE"
    DAY = "DAY"


@unique
class BinanceKlineInterval(Enum):
    """Represents a `Binance` kline chart interval."""

    SECOND_1 = "1s"
    MINUTE_1 = "1m"
    MINUTE_3 = "3m"
    MINUTE_5 = "5m"
    MINUTE_15 = "15m"
    MINUTE_30 = "30m"
    HOUR_1 = "1h"
    HOUR_2 = "2h"
    HOUR_4 = "4h"
    HOUR_6 = "6h"
    HOUR_8 = "8h"
    HOUR_12 = "12h"
    DAY_1 = "1d"
    DAY_3 = "3d"
    WEEK_1 = "1w"
    MONTH_1 = "1M"


@unique
class BinanceExchangeFilterType(Enum):
    """Represents a `Binance` exchange filter type."""

    EXCHANGE_MAX_NUM_ORDERS = "EXCHANGE_MAX_NUM_ORDERS"
    EXCHANGE_MAX_NUM_ALGO_ORDERS = "EXCHANGE_MAX_NUM_ALGO_ORDERS"


@unique
class BinanceSymbolFilterType(Enum):
    """Represents a `Binance` symbol filter type."""

    PRICE_FILTER = "PRICE_FILTER"
    PERCENT_PRICE = "PERCENT_PRICE"
    PERCENT_PRICE_BY_SIDE = "PERCENT_PRICE_BY_SIDE"
    LOT_SIZE = "LOT_SIZE"
    MIN_NOTIONAL = "MIN_NOTIONAL"
    ICEBERG_PARTS = "ICEBERG_PARTS"
    MARKET_LOT_SIZE = "MARKET_LOT_SIZE"
    MAX_NUM_ORDERS = "MAX_NUM_ORDERS"
    MAX_NUM_ALGO_ORDERS = "MAX_NUM_ALGO_ORDERS"
    MAX_NUM_ICEBERG_ORDERS = "MAX_NUM_ICEBERG_ORDERS"
    MAX_POSITION = "MAX_POSITION"
    TRAILING_DELTA = "TRAILING_DELTA"


@unique
class BinanceAccountType(Enum):
    """Represents a `Binance` account type."""

    SPOT = "SPOT"
    MARGIN_CROSS = "MARGIN_CROSS"
    MARGIN_ISOLATED = "MARGIN_ISOLATED"
    FUTURES_USDT = "FUTURES_USDT"
    FUTURES_COIN = "FUTURES_COIN"

    @property
    def is_spot(self):
        return self == BinanceAccountType.SPOT

    @property
    def is_margin(self):
        return self in (
            BinanceAccountType.MARGIN_CROSS,
            BinanceAccountType.MARGIN_ISOLATED,
        )

    @property
    def is_spot_or_margin(self):
        return self in (
            BinanceAccountType.SPOT,
            BinanceAccountType.MARGIN_CROSS,
            BinanceAccountType.MARGIN_ISOLATED,
        )

    @property
    def is_futures(self) -> bool:
        return self in (
            BinanceAccountType.FUTURES_USDT,
            BinanceAccountType.FUTURES_COIN,
        )


@unique
class BinanceOrderSide(Enum):
    """Represents a `Binance` order side."""

    BUY = "BUY"
    SELL = "SELL"


@unique
class BinanceExecutionType(Enum):
    """Represents a `Binance` execution type."""

    NEW = "NEW"
    CANCELED = "CANCELED"
    CALCULATED = "CALCULATED"  # Liquidation Execution
    REJECTED = "REJECTED"
    TRADE = "TRADE"
    EXPIRED = "EXPIRED"


@unique
class BinanceOrderStatus(Enum):
    """Represents a `Binance` order status."""

    NEW = "NEW"
    PARTIALLY_FILLED = "PARTIALLY_FILLED"
    FILLED = "FILLED"
    CANCELED = "CANCELED"
    PENDING_CANCEL = "PENDING_CANCEL"
    REJECTED = "REJECTED"
    EXPIRED = "EXPIRED"
    NEW_INSURANCE = "NEW_INSURANCE"  # Liquidation with Insurance Fund
    NEW_ADL = "NEW_ADL"  # Counterparty Liquidation


@unique
class BinanceTimeInForce(Enum):
    """Represents a `Binance` order time in force."""

    GTC = "GTC"
    IOC = "IOC"
    FOK = "FOK"
    GTX = "GTX"  # FUTURES only, Good Till Crossing (Post Only)


@unique
class BinanceOrderType(Enum):
    """Represents a `Binance` order type."""

    LIMIT = "LIMIT"
    MARKET = "MARKET"
    STOP = "STOP"  # FUTURES only
    STOP_LOSS = "STOP_LOSS"  # SPOT/MARGIN only
    STOP_LOSS_LIMIT = "STOP_LOSS_LIMIT"  # SPOT/MARGIN only
    TAKE_PROFIT = "TAKE_PROFIT"
    TAKE_PROFIT_LIMIT = "TAKE_PROFIT_LIMIT"  # SPOT/MARGIN only
    LIMIT_MAKER = "LIMIT_MAKER"  # SPOT/MARGIN only
    STOP_MARKET = "STOP_MARKET"  # FUTURES only
    TAKE_PROFIT_MARKET = "TAKE_PROFIT_MARKET"  # FUTURES only
    TRAILING_STOP_MARKET = "TRAILING_STOP_MARKET"  # FUTURES only


@unique
class BinanceSecurityType(Enum):
    """Represents a `Binance` endpoint security type."""

    NONE = "NONE"
    TRADE = "TRADE"
    MARGIN = "MARGIN"  # SPOT/MARGIN only
    USER_DATA = "USER_DATA"
    USER_STREAM = "USER_STREAM"
    MARKET_DATA = "MARKET_DATA"


@unique
class BinanceMethodType(Enum):
    """Represents a `Binance` endpoint method type."""

    GET = "GET"
    POST = "POST"
    PUT = "PUT"
    DELETE = "DELETE"


@unique
class BinanceNewOrderRespType(Enum):
    """
    Represents a `Binance` newOrderRespType.
    """

    ACK = "ACK"
    RESULT = "RESULT"
    FULL = "FULL"


class BinanceEnumParser:
    """
    Provides common parsing methods for enums used by the 'Binance' exchange.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self) -> None:
        # Construct dictionary hashmaps
        self.ext_to_int_status = {
            BinanceOrderStatus.NEW: OrderStatus.ACCEPTED,
            BinanceOrderStatus.CANCELED: OrderStatus.CANCELED,
            BinanceOrderStatus.PARTIALLY_FILLED: OrderStatus.PARTIALLY_FILLED,
            BinanceOrderStatus.FILLED: OrderStatus.FILLED,
            BinanceOrderStatus.NEW_ADL: OrderStatus.FILLED,
            BinanceOrderStatus.NEW_INSURANCE: OrderStatus.FILLED,
            BinanceOrderStatus.EXPIRED: OrderStatus.EXPIRED,
        }

        self.ext_to_int_order_side = {
            BinanceOrderSide.BUY: OrderSide.BUY,
            BinanceOrderSide.SELL: OrderSide.SELL,
        }
        self.int_to_ext_order_side = {b: a for a, b in self.ext_to_int_order_side.items()}

        self.ext_to_int_bar_agg = {
            "s": BarAggregation.SECOND,
            "m": BarAggregation.MINUTE,
            "h": BarAggregation.HOUR,
            "d": BarAggregation.DAY,
            "w": BarAggregation.WEEK,
            "M": BarAggregation.MONTH,
        }
        self.int_to_ext_bar_agg = {b: a for a, b in self.ext_to_int_bar_agg.items()}

        self.ext_to_int_time_in_force = {
            BinanceTimeInForce.FOK: TimeInForce.FOK,
            BinanceTimeInForce.GTC: TimeInForce.GTC,
            BinanceTimeInForce.GTX: TimeInForce.GTC,  # Convert GTX to GTC
            BinanceTimeInForce.IOC: TimeInForce.IOC,
        }
        self.int_to_ext_time_in_force = {
            TimeInForce.GTC: BinanceTimeInForce.GTC,
            TimeInForce.GTD: BinanceTimeInForce.GTC,  # Convert GTD to GTC
            TimeInForce.FOK: BinanceTimeInForce.FOK,
            TimeInForce.IOC: BinanceTimeInForce.IOC,
        }

    def parse_binance_order_side(self, order_side: BinanceOrderSide) -> OrderSide:
        try:
            return self.ext_to_int_order_side[order_side]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Binance order side, was {order_side}",  # pragma: no cover
            )

    def parse_internal_order_side(self, order_side: OrderSide) -> BinanceOrderSide:
        try:
            return self.int_to_ext_order_side[order_side]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Nautilus order side, was {order_side}",  # pragma: no cover
            )

    def parse_binance_time_in_force(self, time_in_force: BinanceTimeInForce) -> TimeInForce:
        try:
            return self.ext_to_int_time_in_force[time_in_force]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Binance time in force, was {time_in_force}",  # pragma: no cover
            )

    def parse_internal_time_in_force(self, time_in_force: TimeInForce) -> BinanceTimeInForce:
        try:
            return self.int_to_ext_time_in_force[time_in_force]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Nautilus time in force, was {time_in_force}",  # pragma: no cover
            )

    def parse_binance_order_status(self, order_status: BinanceOrderStatus) -> OrderStatus:
        try:
            return self.ext_to_int_status[order_status]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized binance order status, was {order_status}",  # pragma: no cover
            )

    def parse_binance_order_type(self, order_type: BinanceOrderType) -> OrderType:
        # Implement in child class
        raise NotImplementedError

    def parse_internal_order_type(self, order: Order) -> BinanceOrderType:
        # Implement in child class
        raise NotImplementedError

    def parse_binance_bar_agg(self, bar_agg: str) -> BarAggregation:
        try:
            return self.ext_to_int_bar_agg[bar_agg]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Binance kline resolution, was {bar_agg}",
            )

    def parse_internal_bar_agg(self, bar_agg: BarAggregation) -> str:
        try:
            return self.int_to_ext_bar_agg[bar_agg]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                "unrecognized or non-supported Nautilus BarAggregation,",
                f"was {bar_aggregation_to_str(bar_agg)}",  # pragma: no cover
            )

    def parse_binance_kline_interval_to_bar_spec(
        self,
        kline_interval: BinanceKlineInterval,
    ) -> BarSpecification:
        step = kline_interval.value[:-1]
        binance_bar_agg = kline_interval.value[-1]
        return BarSpecification(
            step=int(step),
            aggregation=self.parse_binance_bar_agg(binance_bar_agg),
            price_type=PriceType.LAST,
        )

    def parse_binance_trigger_type(self, trigger_type: str) -> TriggerType:
        # Replace method in child class, if compatible
        raise NotImplementedError(  # pragma: no cover (design-time error)
            "Cannot parse binance trigger type (not implemented).",  # pragma: no cover
        )
