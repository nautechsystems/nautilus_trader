# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceEnumParser
from nautilus_trader.adapters.binance.common.enums import BinanceExchangeFilterType
from nautilus_trader.adapters.binance.common.enums import BinanceKlineInterval
from nautilus_trader.adapters.binance.common.enums import BinanceRateLimitInterval
from nautilus_trader.adapters.binance.common.enums import BinanceRateLimitType
from nautilus_trader.adapters.binance.common.enums import BinanceSymbolFilterType
from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.adapters.binance.common.types import BinanceTicker
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


################################################################################
# HTTP responses
################################################################################


class BinanceTime(msgspec.Struct, frozen=True):
    """
    Schema of current server time GET response of `time`
    """

    serverTime: int


class BinanceExchangeFilter(msgspec.Struct):
    """
    Schema of an exchange filter, within response of GET `exchangeInfo.`
    """

    filterType: BinanceExchangeFilterType
    maxNumOrders: int | None = None
    maxNumAlgoOrders: int | None = None


class BinanceRateLimit(msgspec.Struct):
    """
    Schema of rate limit info, within response of GET `exchangeInfo.`
    """

    rateLimitType: BinanceRateLimitType
    interval: BinanceRateLimitInterval
    intervalNum: int
    limit: int
    count: int | None = None  # SPOT/MARGIN rateLimit/order response only


class BinanceSymbolFilter(msgspec.Struct):
    """
    Schema of a symbol filter, within response of GET `exchangeInfo.`
    """

    filterType: BinanceSymbolFilterType
    minPrice: str | None = None
    maxPrice: str | None = None
    tickSize: str | None = None
    multiplierUp: str | None = None
    multiplierDown: str | None = None
    multiplierDecimal: str | None = None
    avgPriceMins: int | None = None
    minQty: str | None = None
    maxQty: str | None = None
    stepSize: str | None = None
    limit: int | None = None
    maxNumOrders: int | None = None

    notional: str | None = None  # SPOT/MARGIN & USD-M FUTURES only
    minNotional: str | None = None  # SPOT/MARGIN & USD-M FUTURES only
    maxNumAlgoOrders: int | None = None  # SPOT/MARGIN & USD-M FUTURES only

    bidMultiplierUp: str | None = None  # SPOT/MARGIN only
    bidMultiplierDown: str | None = None  # SPOT/MARGIN only
    askMultiplierUp: str | None = None  # SPOT/MARGIN only
    askMultiplierDown: str | None = None  # SPOT/MARGIN only
    applyMinToMarket: bool | None = None  # SPOT/MARGIN only
    maxNotional: str | None = None  # SPOT/MARGIN only
    applyMaxToMarket: bool | None = None  # SPOT/MARGIN only
    maxNumIcebergOrders: int | None = None  # SPOT/MARGIN only
    maxPosition: str | None = None  # SPOT/MARGIN only
    minTrailingAboveDelta: int | None = None  # SPOT/MARGIN only
    maxTrailingAboveDelta: int | None = None  # SPOT/MARGIN only
    minTrailingBelowDelta: int | None = None  # SPOT/MARGIN only
    maxTrailingBelowDelta: int | None = None  # SPOT/MARGIN only


class BinanceDepth(msgspec.Struct, frozen=True):
    """
    Schema of a binance orderbook depth.

    GET response of `depth`.

    """

    lastUpdateId: int
    bids: list[tuple[str, str]]
    asks: list[tuple[str, str]]

    symbol: str | None = None  # COIN-M FUTURES only
    pair: str | None = None  # COIN-M FUTURES only

    E: int | None = None  # FUTURES only, Message output time
    T: int | None = None  # FUTURES only, Transaction time

    def parse_to_order_book_snapshot(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> OrderBookDeltas:
        bids = [
            BookOrder(OrderSide.BUY, Price.from_str(o[0]), Quantity.from_str(o[1]), 0)
            for o in self.bids or []
        ]
        asks = [
            BookOrder(OrderSide.SELL, Price.from_str(o[0]), Quantity.from_str(o[1]), 0)
            for o in self.asks or []
        ]

        deltas = [OrderBookDelta.clear(instrument_id, self.lastUpdateId, ts_init, ts_init)]
        deltas += [
            OrderBookDelta(
                instrument_id,
                BookAction.ADD,
                o,
                flags=0,
                sequence=self.lastUpdateId or 0,
                ts_event=ts_init,  # No event timestamp
                ts_init=ts_init,
            )
            for o in bids + asks
        ]
        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)


class BinanceTrade(msgspec.Struct, frozen=True):
    """
    Schema of a single trade.
    """

    id: int
    price: str
    qty: str
    quoteQty: str
    time: int
    isBuyerMaker: bool
    isBestMatch: bool | None = None  # SPOT/MARGIN only

    def parse_to_trade_tick(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> TradeTick:
        """
        Parse Binance trade to internal TradeTick.
        """
        return TradeTick(
            instrument_id=instrument_id,
            price=Price.from_str(self.price),
            size=Quantity.from_str(self.qty),
            aggressor_side=AggressorSide.SELLER if self.isBuyerMaker else AggressorSide.BUYER,
            trade_id=TradeId(str(self.id)),
            ts_event=millis_to_nanos(self.time),
            ts_init=ts_init,
        )


class BinanceAggTrade(msgspec.Struct, frozen=True):
    """
    Schema of a single compressed aggregate trade.
    """

    a: int  # Aggregate tradeId
    p: str  # Price
    q: str  # Quantity
    f: int  # First tradeId
    l: int  # Last tradeId
    T: int  # Timestamp
    m: bool  # Was the buyer the maker?
    M: bool | None = None  # SPOT/MARGIN only, was the trade the best price match?

    def parse_to_trade_tick(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> TradeTick:
        """
        Parse Binance trade to internal TradeTick.
        """
        return TradeTick(
            instrument_id=instrument_id,
            price=Price.from_str(self.p),
            size=Quantity.from_str(self.q),
            aggressor_side=AggressorSide.SELLER if self.m else AggressorSide.BUYER,
            trade_id=TradeId(str(self.a)),
            ts_event=millis_to_nanos(self.T),
            ts_init=ts_init,
        )


class BinanceKline(msgspec.Struct, array_like=True):
    """
    Array-like schema of single Binance kline.
    """

    open_time: int
    open: str
    high: str
    low: str
    close: str
    volume: str
    close_time: int
    asset_volume: str
    trades_count: int
    taker_base_volume: str
    taker_quote_volume: str
    ignore: str

    def parse_to_binance_bar(
        self,
        bar_type: BarType,
        ts_init: int,
    ) -> BinanceBar:
        """
        Parse kline to BinanceBar.
        """
        return BinanceBar(
            bar_type=bar_type,
            open=Price.from_str(self.open),
            high=Price.from_str(self.high),
            low=Price.from_str(self.low),
            close=Price.from_str(self.close),
            volume=Quantity.from_str(self.volume),
            quote_volume=Decimal(self.asset_volume),
            count=self.trades_count,
            taker_buy_base_volume=Decimal(self.taker_base_volume),
            taker_buy_quote_volume=Decimal(self.taker_quote_volume),
            ts_event=millis_to_nanos(self.close_time),
            ts_init=ts_init,
        )


class BinanceTicker24hr(msgspec.Struct, frozen=True):
    """
    Schema of single Binance 24hr ticker (FULL/MINI).
    """

    symbol: str | None
    lastPrice: str | None
    openPrice: str | None
    highPrice: str | None
    lowPrice: str | None
    volume: str | None
    openTime: int | None
    closeTime: int | None
    firstId: int | None
    lastId: int | None
    count: int | None

    priceChange: str | None = None  # FULL response only (SPOT/MARGIN)
    priceChangePercent: str | None = None  # FULL response only (SPOT/MARGIN)
    weightedAvgPrice: str | None = None  # FULL response only (SPOT/MARGIN)
    lastQty: str | None = None  # FULL response only (SPOT/MARGIN)

    prevClosePrice: str | None = None  # SPOT/MARGIN only
    bidPrice: str | None = None  # SPOT/MARGIN only
    bidQty: str | None = None  # SPOT/MARGIN only
    askPrice: str | None = None  # SPOT/MARGIN only
    askQty: str | None = None  # SPOT/MARGIN only

    pair: str | None = None  # COIN-M FUTURES only
    baseVolume: str | None = None  # COIN-M FUTURES only

    quoteVolume: str | None = None  # SPOT/MARGIN & USD-M FUTURES only


class BinanceTickerPrice(msgspec.Struct, frozen=True):
    """
    Schema of single Binance Price Ticker.
    """

    symbol: str | None
    price: str | None
    time: int | None = None  # FUTURES only
    ps: str | None = None  # COIN-M FUTURES only, pair


class BinanceTickerBook(msgspec.Struct, frozen=True):
    """
    Schema of a single Binance Order Book Ticker.
    """

    symbol: str | None
    bidPrice: str | None
    bidQty: str | None
    askPrice: str | None
    askQty: str | None
    pair: str | None = None  # USD-M FUTURES only
    time: int | None = None  # FUTURES only, transaction time


################################################################################
# WebSocket messages
################################################################################


class BinanceDataMsgWrapper(msgspec.Struct):
    """
    Provides a wrapper for data WebSocket messages from Binance.
    """

    stream: str | None = None
    id: int | None = None


class BinanceOrderBookDelta(msgspec.Struct, array_like=True):
    """
    Schema of single ask/bid delta.
    """

    price: str
    size: str

    def parse_to_order_book_delta(
        self,
        instrument_id: InstrumentId,
        side: OrderSide,
        flags: int,
        sequence: int,
        ts_event: int,
        ts_init: int,
    ) -> OrderBookDelta:
        size = Quantity.from_str(self.size)
        order = BookOrder(
            side=side,
            price=Price.from_str(self.price),
            size=Quantity.from_str(self.size),
            order_id=0,
        )

        return OrderBookDelta(
            instrument_id=instrument_id,
            action=BookAction.UPDATE if size > 0 else BookAction.DELETE,
            order=order,
            flags=flags,
            sequence=sequence,
            ts_event=ts_event,
            ts_init=ts_init,
        )


class BinanceOrderBookData(msgspec.Struct, frozen=True):
    """
    WebSocket message 'inner struct' for Binance Partial & Diff.

    Book Depth Streams.

    """

    e: str  # Event type
    E: int  # Event time
    s: str  # Symbol
    U: int  # First update ID in event
    u: int  # Final update ID in event
    b: list[BinanceOrderBookDelta]  # Bids to be updated
    a: list[BinanceOrderBookDelta]  # Asks to be updated

    T: int | None = None  # FUTURES only, transaction time
    pu: int | None = None  # FUTURES only, previous final update ID
    ps: str | None = None  # COIN-M FUTURES only, pair

    def parse_to_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
        snapshot: bool = False,
    ) -> OrderBookDeltas:
        ts_event: int = millis_to_nanos(self.T) if self.T is not None else millis_to_nanos(self.E)

        deltas: list[OrderBookDelta] = []

        if snapshot:
            deltas.append(OrderBookDelta.clear(instrument_id, 0, ts_event, ts_init))

        bids_len = len(self.b)
        asks_len = len(self.a)

        for idx, bid in enumerate(self.b):
            flags = 0
            if idx == bids_len - 1 and asks_len == 0:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = bid.parse_to_order_book_delta(
                instrument_id=instrument_id,
                side=OrderSide.BUY,
                flags=RecordFlag.SNAPSHOT if snapshot else flags,
                sequence=self.u,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        for idx, ask in enumerate(self.a):
            flags = 0
            if idx == asks_len - 1:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = ask.parse_to_order_book_delta(
                instrument_id=instrument_id,
                side=OrderSide.SELL,
                flags=RecordFlag.SNAPSHOT if snapshot else flags,
                sequence=self.u,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)


class BinanceOrderBookMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message from Binance Partial & Diff.

    Book Depth Streams.

    """

    stream: str
    data: BinanceOrderBookData


class BinanceQuoteData(msgspec.Struct, frozen=True):
    """
    WebSocket message from Binance Individual Symbol Book Ticker Streams.
    """

    s: str  # symbol
    u: int  # order book updateId
    b: str  # best bid price
    B: str  # best bid qty
    a: str  # best ask price
    A: str  # best ask qty
    T: int | None = None  # event time

    def parse_to_quote_tick(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> QuoteTick:
        return QuoteTick(
            instrument_id=instrument_id,
            bid_price=Price.from_str(self.b),
            ask_price=Price.from_str(self.a),
            bid_size=Quantity.from_str(self.B),
            ask_size=Quantity.from_str(self.A),
            ts_event=millis_to_nanos(self.T) if self.T else ts_init,
            ts_init=ts_init,
        )


class BinanceQuoteMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message from Binance Individual Symbol Book Ticker Streams.
    """

    stream: str
    data: BinanceQuoteData


class BinanceAggregatedTradeData(msgspec.Struct, frozen=True):
    """
    WebSocket message from Binance Aggregate Trade Streams.
    """

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

    def parse_to_trade_tick(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> TradeTick:
        return TradeTick(
            instrument_id=instrument_id,
            price=Price.from_str(self.p),
            size=Quantity.from_str(self.q),
            aggressor_side=AggressorSide.SELLER if self.m else AggressorSide.BUYER,
            trade_id=TradeId(str(self.a)),
            ts_event=millis_to_nanos(self.T),
            ts_init=ts_init,
        )


class BinanceAggregatedTradeMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message.
    """

    stream: str
    data: BinanceAggregatedTradeData


class BinanceTickerData(msgspec.Struct, kw_only=True, frozen=True):
    """
    WebSocket message from Binance 24hr Ticker.

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
    x: str | None = None  # First trade(F)-1 price (first trade before the 24hr rolling window)
    c: str  # Last price
    Q: str  # Last quantity
    b: str | None = None  # Best bid price
    B: str | None = None  # Best bid quantity
    a: str | None = None  # Best ask price
    A: str | None = None  # Best ask quantity
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

    def parse_to_binance_ticker(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> BinanceTicker:
        return BinanceTicker(
            instrument_id=instrument_id,
            price_change=Decimal(self.p),
            price_change_percent=Decimal(self.P),
            weighted_avg_price=Decimal(self.w),
            prev_close_price=Decimal(self.x) if self.x is not None else None,
            last_price=Decimal(self.c),
            last_qty=Decimal(self.Q),
            bid_price=Decimal(self.b) if self.b is not None else None,
            bid_qty=Decimal(self.B) if self.B is not None else None,
            ask_price=Decimal(self.a) if self.a is not None else None,
            ask_qty=Decimal(self.A) if self.A is not None else None,
            open_price=Decimal(self.o),
            high_price=Decimal(self.h),
            low_price=Decimal(self.l),
            volume=Decimal(self.v),
            quote_volume=Decimal(self.q),
            open_time_ms=self.O,
            close_time_ms=self.C,
            first_id=self.F,
            last_id=self.L,
            count=self.n,
            ts_event=millis_to_nanos(self.E),
            ts_init=ts_init,
        )


class BinanceTickerMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message.
    """

    stream: str
    data: BinanceTickerData


class BinanceCandlestick(msgspec.Struct, frozen=True):
    """
    WebSocket message 'inner struct' for Binance Kline/Candlestick Streams.

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
    i: BinanceKlineInterval  # Interval
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

    def parse_to_binance_bar(
        self,
        instrument_id: InstrumentId,
        enum_parser: BinanceEnumParser,
        ts_init: int,
    ) -> BinanceBar:
        bar_type = BarType(
            instrument_id=instrument_id,
            bar_spec=enum_parser.parse_binance_kline_interval_to_bar_spec(self.i),
            aggregation_source=AggregationSource.EXTERNAL,
        )
        return BinanceBar(
            bar_type=bar_type,
            open=Price.from_str(self.o),
            high=Price.from_str(self.h),
            low=Price.from_str(self.l),
            close=Price.from_str(self.c),
            volume=Quantity.from_str(self.v),
            quote_volume=Decimal(self.q),
            count=self.n,
            taker_buy_base_volume=Decimal(self.V),
            taker_buy_quote_volume=Decimal(self.Q),
            ts_event=millis_to_nanos(self.T),
            ts_init=ts_init,
        )


class BinanceCandlestickData(msgspec.Struct, frozen=True):
    """
    WebSocket message 'inner struct'.
    """

    e: str
    E: int
    s: str
    k: BinanceCandlestick


class BinanceCandlestickMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message for Binance Kline/Candlestick Streams.
    """

    stream: str
    data: BinanceCandlestickData
