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

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType


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


class BinanceEnumParser:
    """
    Provides common parsing methods for enums used by the 'Binance' exchange.

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

        self.ext_order_side_to_int_order_side = {
            BinanceOrderSide.BUY: OrderSide.BUY,
            BinanceOrderSide.SELL: OrderSide.SELL,
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

    def parse_binance_order_side(self, order_side: BinanceOrderSide) -> OrderSide:
        try:
            return self.ext_order_side_to_int_order_side[order_side]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized binance order side, was {order_side}",  # pragma: no cover
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
