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

from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceExchangeFilterType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceRateLimitInterval
from nautilus_trader.adapters.binance.common.enums import BinanceRateLimitType
from nautilus_trader.adapters.binance.common.enums import BinanceSymbolFilterType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce


################################################################################
# HTTP responses
################################################################################


class BinanceExchangeFilter(msgspec.Struct):
    """
    HTTP response 'inner struct' from `Binance Spot/Margin`
        `GET /api/v3/exchangeInfo`
    HTTP response 'inner struct' from `Binance USD-M Futures`
        `GET /fapi/v1/exchangeInfo`
    HTTP response 'inner struct' from `Binance COIN-M Futures`
        `GET /dapi/v1/exchangeInfo`
    """

    filterType: BinanceExchangeFilterType
    maxNumOrders: Optional[int] = None
    maxNumAlgoOrders: Optional[int] = None


class BinanceRateLimit(msgspec.Struct):
    """
    HTTP response 'inner struct' from `Binance Spot/Margin`
        `GET /api/v3/exchangeInfo`
    HTTP response 'inner struct' from `Binance USD-M Futures`
        `GET /fapi/v1/exchangeInfo`
    HTTP response 'inner struct' from `Binance COIN-M Futures`
        `GET /dapi/v1/exchangeInfo`
    """

    rateLimitType: BinanceRateLimitType
    interval: BinanceRateLimitInterval
    intervalNum: int
    limit: int


class BinanceSymbolFilter(msgspec.Struct):
    """
    HTTP response 'inner struct' from `Binance Spot/Margin`
        `GET /api/v3/exchangeInfo`
    HTTP response 'inner struct' from `Binance USD-M Futures`
        `GET /fapi/v1/exchangeInfo`
    HTTP response 'inner struct' from `Binance COIN-M Futures`
        `GET /dapi/v1/exchangeInfo`
    """

    filterType: BinanceSymbolFilterType
    minPrice: Optional[str] = None
    maxPrice: Optional[str] = None
    tickSize: Optional[str] = None
    multiplierUp: Optional[str] = None
    multiplierDown: Optional[str] = None
    multiplierDecimal: Optional[int] = None
    avgPriceMins: Optional[int] = None
    minQty: Optional[str] = None
    maxQty: Optional[str] = None
    stepSize: Optional[str] = None
    limit: Optional[int] = None
    maxNumOrders: Optional[int] = None

    notional: Optional[str] = None  # SPOT/MARGIN & USD-M FUTURES only
    minNotional: Optional[str] = None  # SPOT/MARGIN & USD-M FUTURES only
    maxNumAlgoOrders: Optional[int] = None  # SPOT/MARGIN & USD-M FUTURES only

    bidMultiplierUp: Optional[str] = None  # SPOT/MARGIN only
    bidMultiplierDown: Optional[str] = None  # SPOT/MARGIN only
    askMultiplierUp: Optional[str] = None  # SPOT/MARGIN only
    askMultiplierDown: Optional[str] = None  # SPOT/MARGIN only
    applyMinToMarket: Optional[bool] = None  # SPOT/MARGIN only
    maxNotional: Optional[str] = None  # SPOT/MARGIN only
    applyMaxToMarket: Optional[bool] = None  # SPOT/MARGIN only
    maxNumIcebergOrders: Optional[int] = None  # SPOT/MARGIN only
    maxPosition: Optional[str] = None  # SPOT/MARGIN only
    minTrailingAboveDelta: Optional[int] = None  # SPOT/MARGIN only
    maxTrailingAboveDelta: Optional[int] = None  # SPOT/MARGIN only
    minTrailingBelowDelta: Optional[int] = None  # SPOT/MARGIN only
    maxTrailingBelowDetla: Optional[int] = None  # SPOT/MARGIN only


class BinanceOrder(msgspec.Struct):
    """
    HTTP response from `Binance Spot/Margin`
        `GET /api/v3/order`
    HTTP response from `Binance USD-M Futures`
        `GET /fapi/v1/order`
    HTTP response from `Binance COIN-M Futures`
        `GET /dapi/v1/order`
    """

    symbol: str
    orderId: int
    clientOrderId: str
    price: str
    origQty: str
    executedQty: str
    status: BinanceOrderStatus
    timeInForce: BinanceTimeInForce
    type: BinanceOrderType
    side: BinanceOrderSide
    stopPrice: str  # please ignore when order type is TRAILING_STOP_MARKET
    time: int
    updateTime: int

    orderListId: Optional[int] = None  # SPOT/MARGIN only. Unless OCO, the value will always be -1
    cumulativeQuoteQty: Optional[str] = None  # SPOT/MARGIN only, cumulative quote qty
    icebergQty: Optional[str] = None  # SPOT/MARGIN only
    isWorking: Optional[bool] = None  # SPOT/MARGIN only
    workingTime: Optional[int] = None  # SPOT/MARGIN only
    origQuoteOrderQty: Optional[str] = None  # SPOT/MARGIN only
    selfTradePreventionMode: Optional[str] = None  # SPOT/MARGIN only

    avgPrice: Optional[str] = None  # FUTURES only
    origType: Optional[BinanceOrderType] = None  # FUTURES only
    reduceOnly: Optional[bool] = None  # FUTURES only
    positionSide: Optional[str] = None  # FUTURES only
    closePosition: Optional[bool] = None  # FUTURES only, if Close-All
    activatePrice: Optional[
        str
    ] = None  # FUTURES only, activation price, only return with TRAILING_STOP_MARKET order
    priceRate: Optional[
        str
    ] = None  # FUTURES only, callback rate, only return with TRAILING_STOP_MARKET order
    workingType: Optional[str] = None  # FUTURES only
    priceProtect: Optional[bool] = None  # FUTURES only, if conditional order trigger is protected

    cumQuote: Optional[str] = None  # USD-M FUTURES only

    cumBase: Optional[str] = None  # COIN-M FUTURES only
    pair: Optional[str] = None  # COIN-M FUTURES only


class BinanceUserTrade(msgspec.Struct):
    """
    HTTP response from `Binance Spot/Margin`
        `GET /api/v3/myTrades`
    HTTP response from `Binance USD-M Futures`
        `GET /fapi/v1/userTrades`
    HTTP response from `Binance COIN-M Futures`
        `GET /dapi/v1/userTrades`
    """

    symbol: str
    id: int
    orderId: int
    commission: str
    commissionAsset: str
    price: str
    qty: str
    time: int

    quoteQty: Optional[str] = None  # SPOT/MARGIN & USD-M FUTURES only

    orderListId: Optional[int] = None  # SPOT/MARGIN only. Unless OCO, the value will always be -1
    isBuyer: Optional[bool] = None  # SPOT/MARGIN only
    isMaker: Optional[bool] = None  # SPOT/MARGIN only
    isBestMatch: Optional[bool] = None  # SPOT/MARGIN only

    buyer: Optional[bool] = None  # FUTURES only
    maker: Optional[bool] = None  # FUTURES only
    realizedPnl: Optional[str] = None  # FUTURES only
    side: Optional[BinanceOrderSide] = None  # FUTURES only
    positionSide: Optional[str] = None  # FUTURES only

    baseQty: Optional[str] = None  # COIN-M FUTURES only
    pair: Optional[str] = None  # COIN-M FUTURES only


################################################################################
# WebSocket messages
################################################################################


class BinanceDataMsgWrapper(msgspec.Struct):
    """
    Provides a wrapper for data WebSocket messages from `Binance`.
    """

    stream: str


class BinanceOrderBookData(msgspec.Struct, kw_only=True):
    """WebSocket message 'inner struct' for `Binance` Partial & Diff. Book Depth Streams."""

    e: str  # Event type
    E: int  # Event time
    s: str  # Symbol
    U: int  # First update ID in event
    u: int  # Final update ID in event
    b: list[tuple[str, str]]  # Bids to be updated
    a: list[tuple[str, str]]  # Asks to be updated

    T: Optional[int] = None  # FUTURES only, transaction time
    pu: Optional[int] = None  # FUTURES only, previous final update ID

    ps: Optional[str] = None  # COIN-M FUTURES only, pair


class BinanceOrderBookMsg(msgspec.Struct):
    """WebSocket message from `Binance` Partial & Diff. Book Depth Streams."""

    stream: str
    data: BinanceOrderBookData


class BinanceQuoteData(msgspec.Struct):
    """WebSocket message from `Binance` Individual Symbol Book Ticker Streams."""

    s: str  # symbol
    u: int  # order book updateId
    b: str  # best bid price
    B: str  # best bid qty
    a: str  # best ask price
    A: str  # best ask qty


class BinanceQuoteMsg(msgspec.Struct):
    """WebSocket message from `Binance` Individual Symbol Book Ticker Streams."""

    stream: str
    data: BinanceQuoteData


class BinanceAggregatedTradeData(msgspec.Struct):
    """WebSocket message from `Binance` Aggregate Trade Streams."""

    e: str  # Event type
    E: int  # Event time
    s: str  # Symbol
    a: int  # Aggregate trade ID
    p: str  # Price
    q: str  # Quantity
    f: int  # First trade ID
    l: int  # Last trade ID
    T: int  # Trade time
    m: bool  # Is the buyer the market maker?


class BinanceAggregatedTradeMsg(msgspec.Struct):
    """WebSocket message."""

    stream: str
    data: BinanceAggregatedTradeData


class BinanceTickerData(msgspec.Struct, kw_only=True):
    """
    WebSocker message from `Binance` 24hr Ticker

    Fields
    ------
    - e: Event type
    - E: Event time
    - s: Symbol
    - p: Price change
    - P: Price change percent
    - w: Weighted average price
    - x: Previous close price
    - c: Last price
    - Q: Last quantity
    - b: Best bid price
    - B: Best bid quantity
    - a: Best ask price
    - A: Best ask quantity
    - o: Open price
    - h: High price
    - l: Low price
    - v: Total traded base asset volume
    - q: Total traded quote asset volume
    - O: Statistics open time
    - C: Statistics close time
    - F: First trade ID
    - L: Last trade ID
    - n: Total number of trades
    """

    e: str  # Event type
    E: int  # Event time
    s: str  # Symbol
    p: str  # Price change
    P: str  # Price change percent
    w: str  # Weighted average price
    x: Optional[str] = None  # First trade(F)-1 price (first trade before the 24hr rolling window)
    c: str  # Last price
    Q: str  # Last quantity
    b: Optional[str] = None  # Best bid price
    B: Optional[str] = None  # Best bid quantity
    a: Optional[str] = None  # Best ask price
    A: Optional[str] = None  # Best ask quantity
    o: str  # Open price
    h: str  # High price
    l: str  # Low price
    v: str  # Total traded base asset volume
    q: str  # Total traded quote asset volume
    O: int  # Statistics open time
    C: int  # Statistics close time
    F: int  # First trade ID
    L: int  # Last trade ID
    n: int  # Total number of trades


class BinanceTickerMsg(msgspec.Struct):
    """WebSocket message."""

    stream: str
    data: BinanceTickerData


class BinanceCandlestick(msgspec.Struct):
    """
    WebSocket message 'inner struct' for `Binance` Kline/Candlestick Streams.

    Fields
    ------
    - t: Kline start time
    - T: Kline close time
    - s: Symbol
    - i: Interval
    - f: First trade ID
    - L: Last trade ID
    - o: Open price
    - c: Close price
    - h: High price
    - l: Low price
    - v: Base asset volume
    - n: Number of trades
    - x: Is this kline closed?
    - q: Quote asset volume
    - V: Taker buy base asset volume
    - Q: Taker buy quote asset volume
    - B: Ignore
    """

    t: int  # Kline start time
    T: int  # Kline close time
    s: str  # Symbol
    i: str  # Interval
    f: int  # First trade ID
    L: int  # Last trade ID
    o: str  # Open price
    c: str  # Close price
    h: str  # High price
    l: str  # Low price
    v: str  # Base asset volume
    n: int  # Number of trades
    x: bool  # Is this kline closed?
    q: str  # Quote asset volume
    V: str  # Taker buy base asset volume
    Q: str  # Taker buy quote asset volume
    B: str  # Ignore


class BinanceCandlestickData(msgspec.Struct):
    """WebSocket message 'inner struct'."""

    e: str
    E: int
    s: str
    k: BinanceCandlestick


class BinanceCandlestickMsg(msgspec.Struct):
    """WebSocket message for `Binance` Kline/Candlestick Streams."""

    stream: str
    data: BinanceCandlestickData
