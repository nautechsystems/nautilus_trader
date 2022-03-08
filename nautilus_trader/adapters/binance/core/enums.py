# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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


"""
Defines `Binance` specific enums.


References
----------
https://binance-docs.github.io/apidocs/spot/en/#public-api-definitions
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


@unique
class BinancePermissions(Enum):
    """Represents `Binance` trading market permissions."""

    SPOT = "SPOT"
    MARGIN = "MARGIN"
    LEVERAGED = "LEVERAGED"
    TRD_GRP_002 = "TRD_GRP_002"
    TRD_GRP_003 = "TRD_GRP_003"


@unique
class BinanceAccountType(Enum):
    """Represents a `Binance` account type."""

    SPOT = "SPOT"
    MARGIN = "MARGIN"
    FUTURES_USDT = "FUTURES_USDT"
    FUTURES_COIN = "FUTURES_COIN"

    @property
    def is_spot(self):
        return self == BinanceAccountType.SPOT

    @property
    def is_margin(self):
        return self == BinanceAccountType.MARGIN

    @property
    def is_futures(self) -> bool:
        return self in (BinanceAccountType.FUTURES_USDT, BinanceAccountType.FUTURES_COIN)


@unique
class BinanceSpotSymbolStatus(Enum):
    """Represents a `Binance` spot symbol status."""

    PRE_TRADING = "PRE_TRADING"
    TRADING = "TRADING"
    POST_TRADING = "POST_TRADING"
    END_OF_DAY = "END_OF_DAY"
    HALT = "HALT"
    AUCTION_MATCH = "AUCTION_MATCH"
    BREAK = "BREAK"


@unique
class BinanceContractType(Enum):
    """Represents a `Binance` derivatives contract type."""

    PERPETUAL = "PERPETUAL"
    CURRENT_MONTH = "CURRENT_MONTH"
    NEXT_MONTH = "NEXT_MONTH"
    CURRENT_QUARTER = "CURRENT_QUARTER"
    NEXT_QUARTER = "NEXT_QUARTER"


@unique
class BinanceContractStatus(Enum):
    """Represents a `Binance` contract status."""

    PENDING_TRADING = "PENDING_TRADING"
    TRADING = "TRADING"
    PRE_DELIVERING = "PRE_DELIVERING"
    DELIVERING = "DELIVERING"
    DELIVERED = "DELIVERED"
    PRE_SETTLE = "PRE_SETTLE"
    SETTLING = "SETTLING"
    CLOSE = "CLOSE"


@unique
class BinanceOrderStatus(Enum):
    """Represents a `Binance` order status."""

    NEW = "NEW"
    PARTIALLY_FILLED = "PARTIALLY_FILLED"
    FILLED = "FILLED"
    CANCELED = "CANCELED"
    REJECTED = "REJECTED"
    EXPIRED = "EXPIRED"


@unique
class BinanceSpotOrderType(Enum):
    """Represents a `Binance` trigger price type."""

    LIMIT = "LIMIT"
    MARKET = "MARKET"
    STOP_LOSS = "STOP_LOSS"
    STOP_LOSS_LIMIT = "STOP_LOSS_LIMIT"
    TAKE_PROFIT = "TAKE_PROFIT"
    TAKE_PROFIT_LIMIT = "TAKE_PROFIT_LIMIT"
    LIMIT_MAKER = "LIMIT_MAKER"


@unique
class BinanceFuturesOrderType(Enum):
    """Represents a `Binance` trigger price type."""

    LIMIT = "LIMIT"
    MARKET = "MARKET"
    STOP = "STOP"
    STOP_MARKET = "STOP_MARKET"
    TAKE_PROFIT = "TAKE_PROFIT"
    TAKE_PROFIT_MARKET = "TAKE_PROFIT_MARKET"
    TRAILING_STOP_MARKET = "TRAILING_STOP_MARKET"


@unique
class BinancePositionSide(Enum):
    """Represents a `Binance` position side."""

    BOTH = "BOTH"
    LONG = "LONG"
    SHORT = "SHORT"


@unique
class BinanceTimeInForce(Enum):
    """Represents a `Binance` order time in force."""

    GTC = "GTC"
    IOC = "IOC"
    FOK = "FOK"
    GTX = "GTX"  # Good Till Crossing (Post Only)


@unique
class BinanceWorkingType(Enum):
    """Represents a `Binance` trigger price type."""

    MARK_PRICE = "MARK_PRICE"
    CONTRACT_PRICE = "CONTRACT_PRICE"
