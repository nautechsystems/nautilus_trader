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

from typing import List

import msgspec

from nautilus_trader.adapters.binance.common.schemas.market import BinanceExchangeFilter
from nautilus_trader.adapters.binance.common.schemas.market import BinanceRateLimit
from nautilus_trader.adapters.binance.common.schemas.market import BinanceSymbolFilter
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotOrderType
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotPermissions


class BinanceSpotSymbolInfo(msgspec.Struct):
    """Response 'inner struct' from `Binance` Spot GET /fapi/v1/exchangeInfo."""

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
    """Response from `Binance` Spot GET /fapi/v1/exchangeInfo."""

    timezone: str
    serverTime: int
    rateLimits: List[BinanceRateLimit]
    exchangeFilters: List[BinanceExchangeFilter]
    symbols: List[BinanceSpotSymbolInfo]


class BinanceSpotTrade(msgspec.Struct):
    """Response from `Binance` Spot GET /fapi/v1/historicalTrades."""

    id: int
    price: str
    qty: str
    quoteQty: str
    time: int
    isBuyerMaker: bool
    isBestMatch: bool
