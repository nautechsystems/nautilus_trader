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


################################################################################
# HTTP responses
################################################################################


class BinanceListenKey(msgspec.Struct):
    """HTTP response from creating a new `Binance` user listen key."""

    listenKey: str


class BinanceQuote(msgspec.Struct):
    """HTTP response from `Binance` GET /fapi/v1/ticker/bookTicker."""

    symbol: str
    bidPrice: str
    bidQty: str
    askPrice: str
    askQty: str
    time: int  # Transaction time


class BinanceTrade(msgspec.Struct):
    """HTTP response from `Binance` GET /fapi/v1/trades."""

    id: int
    price: str
    qty: str
    quoteQty: str
    time: int
    isBuyerMaker: bool
    isBestMatch: Optional[bool] = True


class BinanceTicker(msgspec.Struct):
    """HTTP response from `Binance` GET /fapi/v1/ticker/24hr ."""

    symbol: str
    priceChange: str
    priceChangePercent: str
    weightedAvgPrice: str
    prevClosePrice: Optional[str] = None
    lastPrice: str
    lastQty: str
    bidPrice: str
    bidQty: str
    askPrice: str
    askQty: str
    openPrice: str
    highPrice: str
    lowPrice: str
    volume: str
    quoteVolume: str
    openTime: int
    closeTime: int
    firstId: int
    lastId: int
    count: int


################################################################################
# WebSocket messages
################################################################################


class BinanceDataMsgWrapper(msgspec.Struct):
    """
    Provides a wrapper for data WebSocket messages from `Binance`.
    """

    stream: str


class BinanceOrderBookData(msgspec.Struct):
    """WebSocket message 'inner struct' for `Binance` Diff. Book Depth Streams."""

    e: str  # Event type
    E: int  # Event time
    T: Optional[int] = None  # Transaction time (Binance Futures only)
    s: str  # Symbol
    U: int  # First update ID in event
    u: int  # Final update ID in event
    pu: Optional[int] = None  # ?? (Binance Futures only)
    b: List[Tuple[str, str]]  # Bids to be updated
    a: List[Tuple[str, str]]  # Asks to be updated


class BinanceOrderBookMsg(msgspec.Struct):
    """WebSocket message from `Binance` Diff. Book Depth Streams."""

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


class BinanceTickerData(msgspec.Struct):
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
