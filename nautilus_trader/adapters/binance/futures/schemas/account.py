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

from decimal import Decimal
from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEnumParser
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesPositionSide
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity


################################################################################
# HTTP responses
################################################################################


class BinanceFuturesBalanceInfo(msgspec.Struct, frozen=True):
    """
    HTTP response 'inner struct' from `Binance Futures` GET /fapi/v2/account (HMAC
    SHA256).
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
    # whether the asset can be used as margin in Multi - Assets mode
    marginAvailable: Optional[bool] = None
    updateTime: Optional[int] = None  # last update time

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.asset)
        total = Decimal(self.walletBalance)
        locked = Decimal(self.initialMargin) + Decimal(self.maintMargin)
        free = total - locked
        return AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )

    def parse_to_margin_balance(self) -> MarginBalance:
        currency: Currency = Currency.from_str(self.asset)
        return MarginBalance(
            initial=Money(Decimal(self.initialMargin), currency),
            maintenance=Money(Decimal(self.maintMargin), currency),
        )


class BinanceFuturesAccountInfo(msgspec.Struct, kw_only=True, frozen=True):
    """
    HTTP response from `Binance Futures` GET /fapi/v2/account (HMAC SHA256).
    """

    feeTier: int  # account commission tier
    canTrade: bool  # if can trade
    canDeposit: bool  # if can transfer in asset
    canWithdraw: bool  # if can transfer out asset
    updateTime: int
    totalInitialMargin: Optional[
        str
    ] = None  # total initial margin required with current mark price (useless with isolated positions), only for USDT asset
    totalMaintMargin: Optional[str] = None  # total maintenance margin required, only for USDT asset
    totalWalletBalance: Optional[str] = None  # total wallet balance, only for USDT asset
    totalUnrealizedProfit: Optional[str] = None  # total unrealized profit, only for USDT asset
    totalMarginBalance: Optional[str] = None  # total margin balance, only for USDT asset
    # initial margin required for positions with current mark price, only for USDT asset
    totalPositionInitialMargin: Optional[str] = None
    # initial margin required for open orders with current mark price, only for USDT asset
    totalOpenOrderInitialMargin: Optional[str] = None
    totalCrossWalletBalance: Optional[str] = None  # crossed wallet balance, only for USDT asset
    # unrealized profit of crossed positions, only for USDT asset
    totalCrossUnPnl: Optional[str] = None
    availableBalance: Optional[str] = None  # available balance, only for USDT asset
    maxWithdrawAmount: Optional[str] = None  # maximum amount for transfer out, only for USDT asset
    assets: list[BinanceFuturesBalanceInfo]

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [asset.parse_to_account_balance() for asset in self.assets]

    def parse_to_margin_balances(self) -> list[MarginBalance]:
        return [asset.parse_to_margin_balance() for asset in self.assets]


class BinanceFuturesPositionRisk(msgspec.Struct, kw_only=True, frozen=True):
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
    maxNotionalValue: Optional[str] = None
    positionAmt: str
    symbol: str
    unRealizedProfit: str
    positionSide: BinanceFuturesPositionSide
    updateTime: int

    def parse_to_position_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        enum_parser: BinanceFuturesEnumParser,
        report_id: UUID4,
        ts_init: int,
    ) -> PositionStatusReport:
        net_size = Decimal(self.positionAmt)
        position_side = enum_parser.parse_futures_position_side(net_size)

        return PositionStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            position_side=position_side,
            quantity=Quantity.from_str(str(abs(net_size))),
            report_id=report_id,
            ts_last=ts_init,
            ts_init=ts_init,
        )


class BinanceFuturesDualSidePosition(msgspec.Struct, frozen=True):
    """
    HTTP response from `Binance Futures` GET /fapi/v1/positionSide/dual (HMAC SHA256).
    """

    dualSidePosition: bool


class BinanceFuturesFeeRates(msgspec.Struct, frozen=True):
    """
    Represents a `BinanceFutures` fee tier.

    https://www.binance.com/en/fee/futureFee

    """

    feeTier: int
    maker: str
    taker: str
