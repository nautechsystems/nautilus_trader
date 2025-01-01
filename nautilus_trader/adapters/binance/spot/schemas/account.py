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

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.schemas.account import BinanceOrder
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money


################################################################################
# HTTP responses
################################################################################


class BinanceSpotBalanceInfo(msgspec.Struct, frozen=True):
    """
    HTTP response 'inner struct' from Binance Spot/Margin GET /api/v3/account (HMAC
    SHA256).
    """

    asset: str
    free: str
    locked: str

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.asset)
        free = Decimal(self.free)
        locked = Decimal(self.locked)
        total: Decimal = free + locked
        return AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )


class BinanceSpotAccountInfo(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Spot/Margin GET /api/v3/account (HMAC SHA256).
    """

    makerCommission: int
    takerCommission: int
    buyerCommission: int
    sellerCommission: int
    canTrade: bool
    canWithdraw: bool
    canDeposit: bool
    updateTime: int
    accountType: BinanceAccountType
    balances: list[BinanceSpotBalanceInfo]
    permissions: list[str]

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [balance.parse_to_account_balance() for balance in self.balances]


class BinanceSpotOrderOco(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Spot/Margin GET /api/v3/orderList (HMAC SHA256).

    HTTP response from Binance Spot/Margin POST /api/v3/order/oco (HMAC SHA256). HTTP
    response from Binance Spot/Margin DELETE /api/v3/orderList (HMAC SHA256).

    """

    orderListId: int
    contingencyType: str
    listStatusType: str
    listOrderStatus: str
    listClientOrderId: str
    transactionTime: int
    symbol: str
    orders: list[BinanceOrder] | None = None  # Included for ACK response type
    orderReports: list[BinanceOrder] | None = None  # Included for FULL & RESPONSE types
