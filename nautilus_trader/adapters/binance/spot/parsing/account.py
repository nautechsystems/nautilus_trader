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
from typing import List

from nautilus_trader.adapters.binance.spot.schemas.account import BinanceSpotBalanceInfo
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotBalance
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money


def parse_account_balances_ws(raw_balances: List[BinanceSpotBalance]) -> List[AccountBalance]:
    balances: List[AccountBalance] = []
    for b in raw_balances:
        currency = Currency.from_str(b.a)
        free = Decimal(b.f)
        locked = Decimal(b.l)
        total: Decimal = free + locked

        balance = AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )
        balances.append(balance)

    return balances


def parse_account_balances_http(raw_balances: List[BinanceSpotBalanceInfo]) -> List[AccountBalance]:
    balances: List[AccountBalance] = []
    for b in raw_balances:
        currency = Currency.from_str(b.asset)
        free = Decimal(b.free)
        locked = Decimal(b.locked)
        total: Decimal = free + locked

        balance = AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )
        balances.append(balance)

    return balances
