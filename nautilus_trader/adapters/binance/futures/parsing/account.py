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
from typing import Dict, List, Tuple

from nautilus_trader.model.currency import Currency
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money


def parse_account_balances_futures_http(raw_balances: List[Dict[str, str]]) -> List[AccountBalance]:
    return parse_balances_futures(
        raw_balances, "asset", "availableBalance", "initialMargin", "maintMargin"
    )


def parse_account_margins_http(raw_balances: List[Dict[str, str]]) -> List[MarginBalance]:
    return parse_margins(raw_balances, "asset", "initialMargin", "maintMargin")


def parse_account_balances_futures_ws(raw_balances: List[Dict[str, str]]) -> List[AccountBalance]:
    return parse_balances_futures(raw_balances, "a", "wb", "bc", "bc")  # TODO(cs): Implement


def parse_balances_futures(
    raw_balances: List[Dict[str, str]],
    asset_key: str,
    free_key: str,
    margin_init_key: str,
    margin_maint_key: str,
) -> List[AccountBalance]:
    parsed_balances: Dict[Currency, Tuple[Decimal, Decimal, Decimal]] = {}
    for b in raw_balances:
        currency = Currency.from_str(b[asset_key])
        free = Decimal(b[free_key])
        locked = Decimal(b[margin_init_key]) + Decimal(b[margin_maint_key])
        total: Decimal = free + locked
        parsed_balances[currency] = (total, locked, free)

    balances: List[AccountBalance] = [
        AccountBalance(
            total=Money(values[0], currency),
            locked=Money(values[1], currency),
            free=Money(values[2], currency),
        )
        for currency, values in parsed_balances.items()
    ]

    return balances


def parse_margins(
    raw_balances: List[Dict[str, str]],
    asset_key: str,
    margin_init_key: str,
    margin_maint_key: str,
) -> List[MarginBalance]:
    parsed_margins: Dict[Currency, Tuple[Decimal, Decimal]] = {}
    for b in raw_balances:
        currency = Currency.from_str(b[asset_key])
        initial = Decimal(b[margin_init_key])
        maintenance = Decimal(b[margin_maint_key])
        parsed_margins[currency] = (initial, maintenance)

    margins: List[MarginBalance] = [
        MarginBalance(
            initial=Money(values[0], currency),
            maintenance=Money(values[1], currency),
        )
        for currency, values in parsed_margins.items()
    ]

    return margins
