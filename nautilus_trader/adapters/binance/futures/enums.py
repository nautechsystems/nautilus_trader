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
Defines `Binance` Futures specific enums.

References
----------
https://binance-docs.github.io/apidocs/futures/en/#public-endpoints-info
"""


@unique
class BinanceFuturesContractType(Enum):
    """Represents a `Binance Futures` derivatives contract type."""

    PERPETUAL = "PERPETUAL"
    CURRENT_MONTH = "CURRENT_MONTH"
    NEXT_MONTH = "NEXT_MONTH"
    CURRENT_QUARTER = "CURRENT_QUARTER"
    NEXT_QUARTER = "NEXT_QUARTER"


@unique
class BinanceFuturesContractStatus(Enum):
    """Represents a `Binance Futures` contract status."""

    PENDING_TRADING = "PENDING_TRADING"
    TRADING = "TRADING"
    PRE_DELIVERING = "PRE_DELIVERING"
    DELIVERING = "DELIVERING"
    DELIVERED = "DELIVERED"
    PRE_SETTLE = "PRE_SETTLE"
    SETTLING = "SETTLING"
    CLOSE = "CLOSE"


@unique
class BinanceFuturesOrderType(Enum):
    """Represents a `Binance Futures` price type."""

    LIMIT = "LIMIT"
    MARKET = "MARKET"
    STOP = "STOP"
    STOP_MARKET = "STOP_MARKET"
    TAKE_PROFIT = "TAKE_PROFIT"
    TAKE_PROFIT_MARKET = "TAKE_PROFIT_MARKET"
    TRAILING_STOP_MARKET = "TRAILING_STOP_MARKET"


@unique
class BinanceFuturesPositionSide(Enum):
    """Represents a `Binance Futures` position side."""

    BOTH = "BOTH"
    LONG = "LONG"
    SHORT = "SHORT"


@unique
class BinanceFuturesTimeInForce(Enum):
    """Represents a `Binance Futures` order time in force."""

    GTC = "GTC"
    IOC = "IOC"
    FOK = "FOK"
    GTX = "GTX"  # Good Till Crossing (Post Only)


@unique
class BinanceFuturesWorkingType(Enum):
    """Represents a `Binance Futures` working type."""

    MARK_PRICE = "MARK_PRICE"
    CONTRACT_PRICE = "CONTRACT_PRICE"


@unique
class BinanceFuturesMarginType(Enum):
    """Represents a `Binance Futures` margin type."""

    ISOLATED = "isolated"
    CROSS = "cross"


@unique
class BinanceFuturesPositionUpdateReason(Enum):
    """Represents a `Binance Futures` position and balance update reason."""

    DEPOSIT = "DEPOSIT"
    WITHDRAW = "WITHDRAW"
    ORDER = "ORDER"
    FUNDING_FEE = "FUNDING_FEE"
    WITHDRAW_REJECT = "WITHDRAW_REJECT"
    ADJUSTMENT = "ADJUSTMENT"
    INSURANCE_CLEAR = "INSURANCE_CLEAR"
    ADMIN_DEPOSIT = "ADMIN_DEPOSIT"
    ADMIN_WITHDRAW = "ADMIN_WITHDRAW"
    MARGIN_TRANSFER = "MARGIN_TRANSFER"
    MARGIN_TYPE_CHANGE = "MARGIN_TYPE_CHANGE"
    ASSET_TRANSFER = "ASSET_TRANSFER"
    OPTIONS_PREMIUM_FEE = "OPTIONS_PREMIUM_FEE"
    OPTIONS_SETTLE_PROFIT = "OPTIONS_SETTLE_PROFIT"
    AUTO_EXCHANGE = "AUTO_EXCHANGE"


@unique
class BinanceFuturesEventType(Enum):
    """Represents a `Binance Futures` event type."""

    LISTEN_KEY_EXPIRED = "listenKeyExpired"
    MARGIN_CALL = "MARGIN_CALL"
    ACCOUNT_UPDATE = "ACCOUNT_UPDATE"
    ORDER_TRADE_UPDATE = "ORDER_TRADE_UPDATE"
    ACCOUNT_CONFIG_UPDATE = "ACCOUNT_CONFIG_UPDATE"
