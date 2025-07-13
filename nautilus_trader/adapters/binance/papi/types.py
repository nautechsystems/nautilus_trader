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

from nautilus_trader.adapters.binance.common.enums import BinanceFuturesPositionSide


class BinancePortfolioMarginBalance(msgspec.Struct, kw_only=True, frozen=True):
    """
    Represents a Binance Portfolio Margin account balance.
    
    References
    ----------
    https://developers.binance.com/docs/derivatives/portfolio-margin/account
    """

    asset: str
    totalWalletBalance: str  # wallet balance = cross margin free + cross margin locked + UM wallet balance + CM wallet balance
    crossMarginAsset: str  # crossMarginAsset = crossMarginFree + crossMarginLocked
    crossMarginBorrowed: str  # principal of cross margin
    crossMarginFree: str  # free asset of cross margin
    crossMarginInterest: str  # interest of cross margin
    crossMarginLocked: str  # lock asset of cross margin
    umWalletBalance: str  # wallet balance of um
    umUnrealizedPNL: str  # unrealized profit of um
    cmWalletBalance: str  # wallet balance of cm
    cmUnrealizedPNL: str | None = None  # unrealized profit of cm
    updateTime: int
    negativeBalance: str | None = None


class BinancePortfolioMarginPositionRisk(msgspec.Struct, kw_only=True, frozen=True):
    """
    Represents a Binance Portfolio Margin position risk.
    
    References
    ----------
    https://developers.binance.com/docs/derivatives/portfolio-margin/account/Query-UM-Position-Information
    https://developers.binance.com/docs/derivatives/portfolio-margin/account/Query-CM-Position-Information
    """

    symbol: str
    markPrice: str
    entryPrice: str
    unRealizedProfit: str
    positionAmt: str
    positionSide: BinanceFuturesPositionSide
    liquidationPrice: str
    updateTime: int
    leverage: str

    # UM specific fields
    notional: str | None = None
    maxNotionalValue: str | None = None

    # CM specific fields
    maxQty: str | None = None
    notionalValue: str | None = None


class BinancePortfolioMarginAccount(msgspec.Struct, kw_only=True, frozen=True):
    """
    Represents a Binance Portfolio Margin account information.
    """

    totalWalletBalance: str
    totalCrossMarginWalletBalance: str
    totalPositionValue: str
    totalAvailable: str
    totalUnrealizedProfit: str
    totalMarginLevel: str
    assets: list[BinancePortfolioMarginBalance]
    positions: list[BinancePortfolioMarginPositionRisk]
    updateTime: int
