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

from nautilus_trader.adapters.bybit.common.parsing import parse_str_to_raw
from nautilus_trader.adapters.bybit.schemas.common import BybitListResult
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money


class BybitCoinBalance(msgspec.Struct):
    availableToBorrow: str
    bonus: str
    accruedInterest: str
    availableToWithdraw: str
    totalOrderIM: str
    equity: str
    usdValue: str
    borrowAmount: str
    # Sum of maintenance margin for all positions.
    totalPositionMM: str
    # Sum of initial margin of all positions + Pre-occupied liquidation fee.
    totalPositionIM: str
    walletBalance: str
    # Unrealised P&L
    unrealisedPnl: str
    # Cumulative Realised P&L
    cumRealisedPnl: str
    locked: str
    # Whether it can be used as a margin collateral currency (platform)
    collateralSwitch: bool
    # Whether the collateral is turned on by the user
    marginCollateral: bool
    coin: str

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.coin)

        total_raw = parse_str_to_raw(self.walletBalance)
        locked_raw = parse_str_to_raw(self.locked)  # TODO: Locked only valid for Spot
        free_raw = total_raw - locked_raw

        return AccountBalance(
            total=Money.from_raw(total_raw, currency),
            locked=Money.from_raw(locked_raw, currency),
            free=Money.from_raw(free_raw, currency),
        )

    def parse_to_margin_balance(self) -> MarginBalance:
        currency = Currency.from_str(self.coin)

        initial_raw = parse_str_to_raw(self.totalPositionIM)
        maintenance_raw = parse_str_to_raw(self.totalPositionMM)

        return MarginBalance(
            initial=Money.from_raw(initial_raw, currency),
            maintenance=Money.from_raw(maintenance_raw, currency),
        )


class BybitWalletBalance(msgspec.Struct):
    totalEquity: str
    accountIMRate: str
    totalMarginBalance: str
    totalInitialMargin: str
    accountType: str
    totalAvailableBalance: str
    accountMMRate: str
    totalPerpUPL: str
    totalWalletBalance: str
    accountLTV: str
    totalMaintenanceMargin: str
    coin: list[BybitCoinBalance]

    def parse_to_account_balance(self) -> list[AccountBalance]:
        return [coin.parse_to_account_balance() for coin in self.coin]

    def parse_to_margin_balance(self) -> list[MarginBalance]:
        return [coin.parse_to_margin_balance() for coin in self.coin]


class BybitWalletBalanceResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitWalletBalance]
    time: int
