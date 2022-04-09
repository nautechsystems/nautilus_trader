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
