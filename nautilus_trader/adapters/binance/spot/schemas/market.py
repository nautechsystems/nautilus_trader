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

from typing import List, Optional, Tuple

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceExchangeFilterType
from nautilus_trader.adapters.binance.common.enums import BinanceRateLimitInterval
from nautilus_trader.adapters.binance.common.enums import BinanceRateLimitType
from nautilus_trader.adapters.binance.common.enums import BinanceSymbolFilterType
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotOrderType
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotPermissions


################################################################################
# HTTP responses
################################################################################


class BinanceExchangeFilter(msgspec.Struct):
    """HTTP response 'inner struct' from `Binance Spot/Margin` GET /fapi/v1/exchangeInfo."""

    filterType: BinanceExchangeFilterType
    maxNumOrders: Optional[int] = None
    maxNumAlgoOrders: Optional[int] = None


class BinanceSymbolFilter(msgspec.Struct):
    """HTTP response 'inner struct' from `Binance Spot/Margin` GET /fapi/v1/exchangeInfo."""

    filterType: BinanceSymbolFilterType
    minPrice: Optional[str] = None
    maxPrice: Optional[str] = None
    tickSize: Optional[str] = None
    multiplierUp: Optional[str] = None
    multiplierDown: Optional[str] = None
    avgPriceMins: Optional[int] = None
    bidMultiplierUp: Optional[str] = None
    bidMultiplierDown: Optional[str] = None
    askMultiplierUp: Optional[str] = None
    askMultiplierDown: Optional[str] = None
    minQty: Optional[str] = None
    maxQty: Optional[str] = None
    stepSize: Optional[str] = None
    minNotional: Optional[str] = None
    applyToMarket: Optional[bool] = None
    limit: Optional[int] = None
    maxNumOrders: Optional[int] = None
    maxNumAlgoOrders: Optional[int] = None
    maxNumIcebergOrders: Optional[int] = None
    maxPosition: Optional[str] = None


class BinanceRateLimit(msgspec.Struct):
    """HTTP response 'inner struct' from `Binance Spot/Margin` GET /fapi/v1/exchangeInfo."""

    rateLimitType: BinanceRateLimitType
    interval: BinanceRateLimitInterval
    intervalNum: int
    limit: int


class BinanceSpotSymbolInfo(msgspec.Struct):
    """HTTP response 'inner struct' from `Binance Spot/Margin` GET /fapi/v1/exchangeInfo."""

    symbol: str
    status: str
    baseAsset: str
    baseAssetPrecision: int
    quoteAsset: str
    quotePrecision: int
    quoteAssetPrecision: int
    orderTypes: List[BinanceSpotOrderType]
    icebergAllowed: bool
    ocoAllowed: bool
    quoteOrderQtyMarketAllowed: bool
    allowTrailingStop: bool
    isSpotTradingAllowed: bool
    isMarginTradingAllowed: bool
    filters: List[BinanceSymbolFilter]
    permissions: List[BinanceSpotPermissions]


class BinanceSpotExchangeInfo(msgspec.Struct):
    """HTTP response from `Binance Spot/Margin` GET /fapi/v1/exchangeInfo."""

    timezone: str
    serverTime: int
    rateLimits: List[BinanceRateLimit]
    exchangeFilters: List[BinanceExchangeFilter]
    symbols: List[BinanceSpotSymbolInfo]


class BinanceSpotOrderBookDepthData(msgspec.Struct):
    """HTTP response from `Binance` GET /fapi/v1/depth."""

    lastUpdateId: int
    bids: List[Tuple[str, str]]
    asks: List[Tuple[str, str]]


################################################################################
# WebSocket messages
################################################################################


class BinanceSpotOrderBookMsg(msgspec.Struct):
    """WebSocket message."""

    stream: str
    data: BinanceSpotOrderBookDepthData


class BinanceSpotTradeData(msgspec.Struct):
    """
    WebSocket message 'inner struct' for `Binance Spot/Margin` Trade Streams.

    Fields
    ------
    - e: Event type
    - E: Event time
    - s: Symbol
    - t: Trade ID
    - p: Price
    - q: Quantity
    - b: Buyer order ID
    - a: Seller order ID
    - T: Trade time
    - m: Is the buyer the market maker?
    """

    e: str  # Event type
    E: int  # Event time
    s: str  # Symbol
    t: int  # Trade ID
    p: str  # Price
    q: str  # Quantity
    b: int  # Buyer order ID
    a: int  # Seller order ID
    T: int  # Trade time
    m: bool  # Is the buyer the market maker?


class BinanceSpotTradeMsg(msgspec.Struct):
    """WebSocket message from `Binance` Trade Streams."""

    stream: str
    data: BinanceSpotTradeData
