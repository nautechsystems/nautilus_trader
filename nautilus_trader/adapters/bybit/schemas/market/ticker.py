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

import msgspec

from nautilus_trader.adapters.bybit.schemas.common import BybitListResult
from nautilus_trader.core.data import Data


class BybitTickerData(Data):
    symbol: str
    bid1Price: str
    bid1Size: str
    ask1Price: str
    ask1Size: str
    lastPrice: str
    highPrice24h: str
    lowPrice24h: str
    turnover24h: str
    volume24h: str

    def __init__(
        self,
        symbol: str,
        bid1Price: str,
        bid1Size: str,
        ask1Price: str,
        ask1Size: str,
        lastPrice: str,
        highPrice24h: str,
        lowPrice24h: str,
        turnover24h: str,
        volume24h: str,
    ):
        self.symbol = symbol
        self.bid1Price = bid1Price
        self.bid1Size = bid1Size
        self.ask1Price = ask1Price
        self.ask1Size = ask1Size
        self.lastPrice = lastPrice
        self.highPrice24h = highPrice24h
        self.lowPrice24h = lowPrice24h
        self.turnover24h = turnover24h
        self.volume24h = volume24h

    def __repr__(self):
        return (
            f"{self.__class__.__name__}("
            f"symbol={self.symbol!r}, "
            f"bid1Price={self.bid1Price!r}, "
            f"bid1Size={self.bid1Size!r}, "
            f"ask1Price={self.ask1Price!r}, "
            f"ask1Size={self.ask1Size!r}, "
            f"lastPrice={self.lastPrice!r}, "
            f"highPrice24h={self.highPrice24h!r}, "
            f"lowPrice24h={self.lowPrice24h!r}, "
            f"turnover24h={self.turnover24h!r}, "
            f"volume24h={self.volume24h!r})"
        )


class BybitTickerSpot(msgspec.Struct):
    symbol: str
    bid1Price: str
    bid1Size: str
    ask1Price: str
    ask1Size: str
    lastPrice: str
    prevPrice24h: str
    price24hPcnt: str
    highPrice24h: str
    lowPrice24h: str
    turnover24h: str
    volume24h: str
    usdIndexPrice: str


class BybitTickerOption(msgspec.Struct):
    symbol: str
    bid1Price: str
    bid1Size: str
    bid1Iv: str
    ask1Price: str
    ask1Size: str
    ask1Iv: str
    lastPrice: str
    highPrice24h: str
    lowPrice24h: str
    markPrice: str
    indexPrice: str
    markIv: str
    underlyingPrice: str
    openInterest: str
    turnover24h: str
    volume24h: str
    totalVolume: str
    totalTurnover: str
    delta: str
    gamma: str
    vega: str
    theta: str
    predictedDeliveryPrice: str
    change24h: str


class BybitTickerLinear(msgspec.Struct):
    symbol: str
    lastPrice: str
    indexPrice: str
    markPrice: str
    prevPrice24h: str
    price24hPcnt: str
    highPrice24h: str
    lowPrice24h: str
    prevPrice1h: str
    openInterest: str
    openInterestValue: str
    turnover24h: str
    volume24h: str
    fundingRate: str
    nextFundingTime: str
    predictedDeliveryPrice: str
    basisRate: str
    deliveryFeeRate: str
    deliveryTime: str
    ask1Size: str
    bid1Price: str
    ask1Price: str
    bid1Size: str
    basis: str


class BybitTickersLinearResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitTickerLinear]


class BybitTickersOptionResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitTickerOption]


class BybitTickersSpotResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitTickerSpot]


BybitTicker = BybitTickerLinear | BybitTickerOption | BybitTickerSpot

BybitTickerList = list[BybitTickerLinear] | list[BybitTickerOption] | list[BybitTickerSpot]

BybitTickersResponse = (
    BybitTickersLinearResponse | BybitTickersSpotResponse | BybitTickersOptionResponse
)
