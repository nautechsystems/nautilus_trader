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
Defines `Binance` Spot/Margin specific enums.

References
----------
https://binance-docs.github.io/apidocs/spot/en/#public-api-definitions
"""


@unique
class BinanceSpotPermissions(Enum):
    """Represents `Binance Spot/Margin` trading permissions."""

    SPOT = "SPOT"
    MARGIN = "MARGIN"
    LEVERAGED = "LEVERAGED"
    TRD_GRP_002 = "TRD_GRP_002"
    TRD_GRP_003 = "TRD_GRP_003"
    TRD_GRP_004 = "TRD_GRP_004"
    TRD_GRP_005 = "TRD_GRP_005"


@unique
class BinanceSpotSymbolStatus(Enum):
    """Represents a `Binance Spot/Margin` symbol status."""

    PRE_TRADING = "PRE_TRADING"
    TRADING = "TRADING"
    POST_TRADING = "POST_TRADING"
    END_OF_DAY = "END_OF_DAY"
    HALT = "HALT"
    AUCTION_MATCH = "AUCTION_MATCH"
    BREAK = "BREAK"


@unique
class BinanceSpotTimeInForce(Enum):
    """Represents a `Binance Spot/Margin` order time in force."""

    GTC = "GTC"
    IOC = "IOC"
    FOK = "FOK"


@unique
class BinanceSpotEventType(Enum):
    """Represents a `Binance Spot/Margin` event type."""

    outboundAccountPosition = "outboundAccountPosition"
    balanceUpdate = "balanceUpdate"
    executionReport = "executionReport"
    listStatus = "listStatus"


@unique
class BinanceSpotOrderType(Enum):
    """Represents a `Binance Spot/Margin` order type."""

    LIMIT = "LIMIT"
    MARKET = "MARKET"
    STOP = "STOP"
    STOP_LOSS = "STOP_LOSS"
    STOP_LOSS_LIMIT = "STOP_LOSS_LIMIT"
    TAKE_PROFIT = "TAKE_PROFIT"
    TAKE_PROFIT_LIMIT = "TAKE_PROFIT_LIMIT"
    LIMIT_MAKER = "LIMIT_MAKER"


@unique
class BinanceSpotOrderStatus(Enum):
    """Represents a `Binance` order status."""

    NEW = "NEW"
    PARTIALLY_FILLED = "PARTIALLY_FILLED"
    FILLED = "FILLED"
    CANCELED = "CANCELED"
    REJECTED = "REJECTED"
    EXPIRED = "EXPIRED"
