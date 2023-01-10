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

from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol


################################################################################
# HTTP responses
################################################################################


class BinanceOrder(msgspec.Struct, frozen=True):
    """
    HTTP response from `Binance Spot/Margin`
        `GET /api/v3/order`
    HTTP response from `Binance USD-M Futures`
        `GET /fapi/v1/order`
    HTTP response from `Binance COIN-M Futures`
        `GET /dapi/v1/order`
    """

    symbol: BinanceSymbol
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

    symbol: BinanceSymbol
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
