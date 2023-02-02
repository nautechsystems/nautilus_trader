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


import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.schemas.market import BinanceExchangeFilter
from nautilus_trader.adapters.binance.common.schemas.market import BinanceOrderBookDelta
from nautilus_trader.adapters.binance.common.schemas.market import BinanceRateLimit
from nautilus_trader.adapters.binance.common.schemas.market import BinanceSymbolFilter
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotPermissions
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


################################################################################
# HTTP responses
################################################################################


class BinanceSpotSymbolInfo(msgspec.Struct, frozen=True):
    """HTTP response 'inner struct' from `Binance Spot/Margin` GET /api/v3/exchangeInfo."""

    symbol: str
    status: str
    baseAsset: str
    baseAssetPrecision: int
    quoteAsset: str
    quotePrecision: int
    quoteAssetPrecision: int
    orderTypes: list[BinanceOrderType]
    icebergAllowed: bool
    ocoAllowed: bool
    quoteOrderQtyMarketAllowed: bool
    allowTrailingStop: bool
    isSpotTradingAllowed: bool
    isMarginTradingAllowed: bool
    filters: list[BinanceSymbolFilter]
    permissions: list[BinanceSpotPermissions]

    def parse_to_base_asset(self):
        return Currency(
            code=self.baseAsset,
            precision=self.baseAssetPrecision,
            iso4217=0,  # Currently undetermined for crypto assets
            name=self.baseAsset,
            currency_type=CurrencyType.CRYPTO,
        )

    def parse_to_quote_asset(self):
        return Currency(
            code=self.baseAsset,
            precision=self.baseAssetPrecision,
            iso4217=0,  # Currently undetermined for crypto assets
            name=self.baseAsset,
            currency_type=CurrencyType.CRYPTO,
        )


class BinanceSpotExchangeInfo(msgspec.Struct, frozen=True):
    """HTTP response from `Binance Spot/Margin` GET /api/v3/exchangeInfo."""

    timezone: str
    serverTime: int
    rateLimits: list[BinanceRateLimit]
    exchangeFilters: list[BinanceExchangeFilter]
    symbols: list[BinanceSpotSymbolInfo]


class BinanceSpotAvgPrice(msgspec.Struct, frozen=True):
    """HTTP response from `Binance Spot/Margin` GET /api/v3/avgPrice."""

    mins: int
    price: str


################################################################################
# WebSocket messages
################################################################################


class BinanceSpotOrderBookPartialDepthData(msgspec.Struct):
    """Websocket message 'inner struct' for 'Binance Spot/Margin Partial Book Depth Streams.'"""

    lastUpdateId: int
    bids: list[BinanceOrderBookDelta]
    asks: list[BinanceOrderBookDelta]

    def parse_to_order_book_snapshot(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> OrderBookSnapshot:
        return OrderBookSnapshot(
            instrument_id=instrument_id,
            book_type=BookType.L2_MBP,
            bids=[[float(o.price), float(o.size)] for o in self.bids],
            asks=[[float(o.price), float(o.size)] for o in self.asks],
            ts_event=ts_init,
            ts_init=ts_init,
            sequence=self.lastUpdateId,
        )


class BinanceSpotOrderBookPartialDepthMsg(msgspec.Struct):
    """WebSocket message for 'Binance Spot/Margin' Partial Book Depth Streams."""

    stream: str
    data: BinanceSpotOrderBookPartialDepthData


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
            trade_id=TradeId(str(self.t)),
            ts_event=millis_to_nanos(self.T),
            ts_init=ts_init,
        )


class BinanceSpotTradeMsg(msgspec.Struct):
    """WebSocket message from `Binance` Trade Streams."""

    stream: str
    data: BinanceSpotTradeData
