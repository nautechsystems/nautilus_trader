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
class BinanceOrderType(Enum):
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
