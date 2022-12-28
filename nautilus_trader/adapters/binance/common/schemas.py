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

from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceExchangeFilterType
from nautilus_trader.adapters.binance.common.enums import BinanceRateLimitInterval
from nautilus_trader.adapters.binance.common.enums import BinanceRateLimitType
from nautilus_trader.adapters.binance.common.enums import BinanceSymbolFilterType


################################################################################
# HTTP responses
################################################################################


class BinanceListenKey(msgspec.Struct):
    """HTTP response from creating a new `Binance` user listen key."""

    listenKey: str


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
    bidMultiplierUp: Optional[str] = None  # SPOT/MARGIN only
    bidMultiplierDown: Optional[str] = None  # SPOT/MARGIN only
    askMultiplierUp: Optional[str] = None  # SPOT/MARGIN only
    askMultiplierDown: Optional[str] = None  # SPOT/MARGIN only
    minQty: Optional[str] = None
    maxQty: Optional[str] = None
    stepSize: Optional[str] = None
    notional: Optional[str] = None  # SPOT/MARGIN & USD-M FUTURES only
    minNotional: Optional[str] = None  # SPOT/MARGIN & USD-M FUTURES only
    applyMinToMarket: Optional[bool] = None  # SPOT/MARGIN only
    maxNotional: Optional[str] = None  # SPOT/MARGIN only
    applyMaxToMarket: Optional[bool] = None  # SPOT/MARGIN only
    limit: Optional[int] = None
    maxNumOrders: Optional[int] = None
    maxNumAlgoOrders: Optional[int] = None  # SPOT/MARGIN & USD-M FUTURES only
    maxNumIcebergOrders: Optional[int] = None  # SPOT/MARGIN only
    maxPosition: Optional[str] = None  # SPOT/MARGIN only
    minTrailingAboveDelta: Optional[int] = None  # SPOT/MARGIN only
    maxTrailingAboveDelta: Optional[int] = None  # SPOT/MARGIN only
    minTrailingBelowDelta: Optional[int] = None  # SPOT/MARGIN only
    maxTrailingBelowDetla: Optional[int] = None  # SPOT/MARGIN only


class BinanceQuote(msgspec.Struct):
    """
    HTTP response from `Binance Spot/Margin`
        `GET /api/v3/ticker/bookTicker`
    HTTP response from `Binance USD-M Futures`
        `GET /fapi/v1/ticker/bookTicker`
    HTTP response from `Binance COIN-M Futures`
        `GET /dapi/v1/ticker/bookTicker`
    """

    symbol: str
    bidPrice: str
    bidQty: str
    askPrice: str
    askQty: str
    pair: Optional[str] = None  # USD-M FUTURES only
    time: Optional[int] = None  # FUTURES only, transaction time


class BinanceTrade(msgspec.Struct):
    """
    HTTP response from `Binance Spot/Margin`
        `GET /api/v3/trades`
    HTTP response from `Binance USD-M Futures`
        `GET /fapi/v1/trades`
    HTTP response from `Binance COIN-M Futures`
        `GET /dapi/v1/trades`
    """

    id: int
    price: str
    qty: str
    quoteQty: str
    time: int
    isBuyerMaker: bool
    isBestMatch: Optional[bool] = True  # SPOT/MARGIN only


class BinanceTicker(msgspec.Struct, kw_only=True):
    """
    HTTP response from `Binance Spot/Margin`
        `GET /api/v3/ticker/24h`
    HTTP response from `Binance USD-M Futures`
        `GET /fapi/v1/ticker/24h`
    HTTP response from `Binance COIN-M Futures`
        `GET /dapi/v1/ticker/24h`
    """

    symbol: str
    pair: Optional[str]  # COIN-M FUTURES only
    priceChange: str
    priceChangePercent: str
    weightedAvgPrice: str
    prevClosePrice: Optional[str] = None  # SPOT/MARGIN only
    lastPrice: str
    lastQty: str
    bidPrice: Optional[str] = None  # SPOT/MARGIN only
    bidQty: Optional[str] = None  # SPOT/MARGIN only
    askPrice: Optional[str] = None  # SPOT/MARGIN only
    askQty: Optional[str] = None  # SPOT/MARGIN only
    openPrice: str
    highPrice: str
    lowPrice: str
    volume: str
    baseVolume: Optional[str] = None  # COIN-M FUTURES only
    quoteVolume: Optional[str] = None  # SPOT/MARGIN & USD-M FUTURES only
    openTime: int
    closeTime: int
    firstId: int
    lastId: int
    count: int


class BinanceDepth(msgspec.Struct):
    """
    HTTP response from `Binance Spot/Margin`
        `GET /api/v3/depth`
    HTTP response from `Binance USD-M Futures`
        `GET /fapi/v1/depth`
    HTTP response from `Binance COIN-M Futures`
        `GET /dapi/v1/depth`
    """

    lastUpdateId: int
    bids: list[tuple[str, str]]
    asks: list[tuple[str, str]]
    symbol: Optional[str] = None  # COIN-M FUTURES only
    pair: Optional[str] = None  # COIN-M FUTURES only
    E: Optional[int] = None  # FUTURES only, Message output time
    T: Optional[int] = None  # FUTURES only, Transaction time


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
    T: Optional[int] = None  # FUTURES only, transaction time
    s: str  # Symbol
    ps: Optional[str] = None  # COIN-M FUTURES only, pair
    U: int  # First update ID in event
    u: int  # Final update ID in event
    pu: Optional[int] = None  # FUTURES only, previous final update ID
    b: list[tuple[str, str]]  # Bids to be updated
    a: list[tuple[str, str]]  # Asks to be updated


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
