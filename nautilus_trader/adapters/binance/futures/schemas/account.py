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
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEnumParser
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity


################################################################################
# HTTP responses
################################################################################


class BinanceFuturesBalanceInfo(msgspec.Struct, frozen=True):
    """
    HTTP response 'inner struct' from Binance Futures GET /fapi/v2/account (HMAC
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
    marginAvailable: bool | None = None
    updateTime: int | None = None  # last update time

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.asset)
        # This calculation is currently mixing wallet cash balance and the available balance after
        # considering margin collateral. As a temporary measure we're taking the `min` to
        # disregard free amounts above the cash balance, but still considering where not all
        # balance is available (so locked in some way, i.e. allocated as collateral).
        total = Decimal(self.walletBalance)
        free = min(Decimal(self.availableBalance), total)
        locked = total - free
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
    HTTP response from Binance Futures GET /fapi/v2/account (HMAC SHA256).
    """

    feeTier: int  # account commission tier
    canTrade: bool  # if can trade
    canDeposit: bool  # if can transfer in asset
    canWithdraw: bool  # if can transfer out asset
    updateTime: int
    totalInitialMargin: str | None = (
        None  # total initial margin required with current mark price (useless with isolated positions), only for USDT
    )
    totalMaintMargin: str | None = None  # total maintenance margin required, only for USDT asset
    totalWalletBalance: str | None = None  # total wallet balance, only for USDT asset
    totalUnrealizedProfit: str | None = None  # total unrealized profit, only for USDT asset
    totalMarginBalance: str | None = None  # total margin balance, only for USDT asset
    # initial margin required for positions with current mark price, only for USDT asset
    totalPositionInitialMargin: str | None = None
    # initial margin required for open orders with current mark price, only for USDT asset
    totalOpenOrderInitialMargin: str | None = None
    totalCrossWalletBalance: str | None = None  # crossed wallet balance, only for USDT asset
    # unrealized profit of crossed positions, only for USDT asset
    totalCrossUnPnl: str | None = None
    availableBalance: str | None = None  # available balance, only for USDT asset
    maxWithdrawAmount: str | None = None  # maximum amount for transfer out, only for USDT asset
    assets: list[BinanceFuturesBalanceInfo]

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [asset.parse_to_account_balance() for asset in self.assets]

    def parse_to_margin_balances(self) -> list[MarginBalance]:
        return [asset.parse_to_margin_balance() for asset in self.assets]


class BinanceFuturesPositionRisk(msgspec.Struct, kw_only=True, frozen=True):
    """
    HTTP response from Binance Futures GET /fapi/v3/positionRisk (HMAC SHA256).

    Supports both v2 and v3 schemas. v2 fields (marginType, isAutoAddMargin,
    leverage, maxNotionalValue) are optional for backward compatibility.
    v3 adds: breakEvenPrice, notional, marginAsset, isolatedWallet, initialMargin,
    maintMargin, positionInitialMargin, openOrderInitialMargin, adl, bidNotional,
    askNotional.

    """

    # Core fields (present in both v2 and v3)
    symbol: str
    positionSide: BinanceFuturesPositionSide
    positionAmt: str
    entryPrice: str
    markPrice: str
    unRealizedProfit: str
    liquidationPrice: str
    isolatedMargin: str
    updateTime: int

    # v2 fields (may not be present in v3)
    marginType: str | None = None
    isAutoAddMargin: str | None = None
    leverage: str | None = None
    maxNotionalValue: str | None = None

    # v3-specific fields
    breakEvenPrice: str | None = None
    notional: str | None = None
    marginAsset: str | None = None
    isolatedWallet: str | None = None
    initialMargin: str | None = None
    maintMargin: str | None = None
    positionInitialMargin: str | None = None
    openOrderInitialMargin: str | None = None
    adl: int | None = None
    bidNotional: str | None = None
    askNotional: str | None = None

    def parse_to_position_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        enum_parser: BinanceFuturesEnumParser,
        report_id: UUID4,
        ts_init: int,
    ) -> PositionStatusReport:
        net_size = Decimal(self.positionAmt)

        venue_position_id: PositionId | None = None

        if self.positionSide in (
            BinanceFuturesPositionSide.LONG,
            BinanceFuturesPositionSide.SHORT,
        ):
            position_side = (
                PositionSide.LONG
                if self.positionSide == BinanceFuturesPositionSide.LONG
                else PositionSide.SHORT
            )
            venue_position_id = PositionId(f"{instrument_id}-{self.positionSide.value}")
        else:
            position_side = enum_parser.parse_futures_position_side(net_size)

        avg_px_open = Decimal(self.entryPrice) if self.entryPrice else None

        return PositionStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            position_side=position_side,
            quantity=Quantity.from_str(str(abs(net_size))),
            report_id=report_id,
            ts_last=ts_init,
            ts_init=ts_init,
            venue_position_id=venue_position_id,
            avg_px_open=avg_px_open,
        )


class BinanceFuturesDualSidePosition(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures GET /fapi/v1/positionSide/dual (HMAC SHA256).
    """

    dualSidePosition: bool


class BinanceFuturesFeeRates(msgspec.Struct, frozen=True):
    """
    Represents a Binance Futures fee tier.

    https://www.binance.com/en/fee/futureFee

    """

    feeTier: int
    maker: str
    taker: str


class BinanceFuturesLeverage(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures POST /fapi/v1/leverage.
    """

    leverage: int
    maxNotionalValue: str
    symbol: str


class BinanceFuturesSymbolConfig(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures GET /fapi/v1/symbolConfig.

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/account/rest-api/Symbol-Config

    """

    symbol: str
    marginType: str
    isAutoAddMargin: bool
    leverage: int
    maxNotionalValue: str


class BinanceFuturesMarginTypeResponse(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures `POST /fapi/v1/marginType`.
    """

    code: int
    msg: str
