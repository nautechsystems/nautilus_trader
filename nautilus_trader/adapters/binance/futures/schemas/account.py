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

from typing import List, Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesOrderStatus
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesOrderType
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesPositionSide


################################################################################
# HTTP responses
################################################################################


class BinanceFuturesAssetInfo(msgspec.Struct):
    """
    HTTP response 'inner struct' from `Binance Futures` GET /fapi/v2/account (HMAC SHA256).
    """

    asset: str  # asset name
    walletBalance: str  # wallet balance
    unrealizedProfit: str  # unrealized profit
    marginBalance: str  # margin balance
    maintMargin: str  # maintenance margin required
    initialMargin: str  # total initial margin required with current mark price
    positionInitialMargin: str  # initial margin required for positions with current mark price
    openOrderInitialMargin: str  # initial margin required for open orders with current mark price
    crossWalletBalance: str  # crossed wallet balance
    crossUnPnl: str  # unrealized profit of crossed positions
    availableBalance: str  # available balance
    maxWithdrawAmount: str  # maximum amount for transfer out
    marginAvailable: bool  # whether the asset can be used as margin in Multi - Assets mode
    updateTime: int  # last update time


class BinanceFuturesAccountInfo(msgspec.Struct):
    """
    HTTP response from `Binance Futures` GET /fapi/v2/account (HMAC SHA256).
    """

    feeTier: int  # account commission tier
    canTrade: bool  # if can trade
    canDeposit: bool  # if can transfer in asset
    canWithdraw: bool  # if can transfer out asset
    updateTime: int
    totalInitialMargin: str  # total initial margin required with current mark price (useless with isolated positions), only for USDT asset
    totalMaintMargin: str  # total maintenance margin required, only for USDT asset
    totalWalletBalance: str  # total wallet balance, only for USDT asset
    totalUnrealizedProfit: str  # total unrealized profit, only for USDT asset
    totalMarginBalance: str  # total margin balance, only for USDT asset
    totalPositionInitialMargin: str  # initial margin required for positions with current mark price, only for USDT asset
    totalOpenOrderInitialMargin: str  # initial margin required for open orders with current mark price, only for USDT asset
    totalCrossWalletBalance: str  # crossed wallet balance, only for USDT asset
    totalCrossUnPnl: str  # unrealized profit of crossed positions, only for USDT asset
    availableBalance: str  # available balance, only for USDT asset
    maxWithdrawAmount: str  # maximum amount for transfer out, only for USDT asset
    assets: List[BinanceFuturesAssetInfo]


class BinanceFuturesOrder(msgspec.Struct):
    """
    HTTP response from `Binance Futures` GET /fapi/v1/order (HMAC SHA256).
    """

    avgPrice: str
    clientOrderId: str
    cumQuote: str
    executedQty: str
    orderId: int
    origQty: str
    origType: str
    price: str
    reduceOnly: bool
    side: str
    positionSide: str
    status: BinanceFuturesOrderStatus
    stopPrice: str
    closePosition: bool
    symbol: str
    time: int
    timeInForce: str
    type: BinanceFuturesOrderType
    activatePrice: Optional[str] = None
    priceRate: Optional[str] = None
    updateTime: int
    workingType: str
    priceProtect: bool


class BinanceFuturesAccountTrade(msgspec.Struct):
    """
    HTTP response from ` Binance Futures` GET /fapi/v1/userTrades (HMAC SHA256).
    """

    buyer: bool
    commission: str
    commissionAsset: str
    id: int
    maker: bool
    orderId: int
    price: str
    qty: str
    quoteQty: str
    realizedPnl: str
    side: BinanceOrderSide
    positionSide: BinanceFuturesPositionSide
    symbol: str
    time: int


class BinanceFuturesPositionRisk(msgspec.Struct):
    """
    HTTP response from ` Binance Futures` GET /fapi/v2/positionRisk (HMAC SHA256).
    """

    entryPrice: str
    marginType: str
    isAutoAddMargin: str
    isolatedMargin: str
    leverage: str
    liquidationPrice: str
    markPrice: str
    maxNotionalValue: str
    positionAmt: str
    symbol: str
    unRealizedProfit: str
    positionSide: BinanceFuturesPositionSide
    updateTime: int
