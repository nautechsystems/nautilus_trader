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

from decimal import Decimal
from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BinanceTrade(msgspec.Struct, frozen=True):
    """Schema of a single trade. Used in recent and historical trade lookups."""

    id: int
    price: str
    qty: str
    quoteQty: str
    time: int
    isBuyerMaker: bool
    isBestMatch: Optional[bool] = None  # SPOT/MARGIN only

    def _parse_to_trade_tick(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> list[TradeTick]:
        """Parse Binance trade to internal TradeTick"""
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
    """Schema of a single compressed aggregate trade"""

    a: int  # Aggregate tradeId
    p: str  # Price
    q: str  # Quantity
    f: int  # First tradeId
    l: int  # Last tradeId
    T: int  # Timestamp
    m: bool  # Was the buyer the maker?
    M: Optional[bool] = None  # SPOT/MARGIN only, was the trade the best price match?


class BinanceKline(msgspec.Struct, array_like=True):
    """Array-like schema of single Binance kline"""

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

    def _parse_to_binance_bar(
        self,
        bar_type: BarType,
        ts_init: int,
    ) -> BinanceBar:
        """Parse kline to BinanceBar"""
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
            ts_event=millis_to_nanos(self.open_time),
            ts_init=ts_init,
        )


class BinanceTicker24hr(msgspec.Struct, frozen=True):
    """Schema of single Binance 24hr ticker (FULL/MINI)"""

    symbol: str
    lastPrice: str
    openPrice: str
    highPrice: str
    lowPrice: str
    volume: str
    openTime: int
    closeTime: int
    firstId: int
    lastId: int
    count: int

    priceChange: Optional[str] = None  # FULL response only (SPOT/MARGIN)
    priceChangePercent: Optional[str] = None  # FULL response only (SPOT/MARGIN)
    weightedAvgPrice: Optional[str] = None  # FULL response only (SPOT/MARGIN)
    lastQty: Optional[str] = None  # FULL response only (SPOT/MARGIN)

    prevClosePrice: Optional[str] = None  # SPOT/MARGIN only
    bidPrice: Optional[str] = None  # SPOT/MARGIN only
    bidQty: Optional[str] = None  # SPOT/MARGIN only
    askPrice: Optional[str] = None  # SPOT/MARGIN only
    askQty: Optional[str] = None  # SPOT/MARGIN only

    pair: Optional[str] = None  # COIN-M FUTURES only
    baseVolume: Optional[str] = None  # COIN-M FUTURES only

    quoteVolume: Optional[str] = None  # SPOT/MARGIN & USD-M FUTURES only


class BinanceTickerPrice(msgspec.Struct, frozen=True):
    """Schema of single Binance Price Ticker"""

    symbol: str
    price: str
    time: Optional[int] = None  # FUTURES only
    ps: Optional[str] = None  # COIN-M FUTURES only, pair


class BinanceTickerBook(msgspec.Struct):
    """Schema of a single Binance Order Book Ticker"""

    symbol: str
    bidPrice: str
    bidQty: str
    askPrice: str
    askQty: str
    pair: Optional[str] = None  # USD-M FUTURES only
    time: Optional[int] = None  # FUTURES only, transaction time
