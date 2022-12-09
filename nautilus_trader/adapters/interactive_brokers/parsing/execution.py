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
from itertools import groupby

from ib_insync import AccountValue
from ib_insync import LimitOrder as IBLimitOrder
from ib_insync import MarketOrder as IBMarketOrder
from ib_insync import Order as IBOrder
from ib_insync import StopLimitOrder as IBStopLimitOrder
from ib_insync import StopOrder as IBStopOrder

from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.order_side import OrderSideParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orders.base import Order as NautilusOrder
from nautilus_trader.model.orders.limit import LimitOrder as NautilusLimitOrder
from nautilus_trader.model.orders.market import MarketOrder as NautilusMarketOrder


def nautilus_order_to_ib_order(order: NautilusOrder) -> IBOrder:
    if isinstance(order, NautilusMarketOrder):
        return IBMarketOrder(
            action=OrderSideParser.to_str_py(order.side),
            totalQuantity=order.quantity.as_double(),
            orderRef=order.client_order_id.value,
        )
    elif isinstance(order, NautilusLimitOrder):
        # TODO - Time in force, etc
        return IBLimitOrder(
            action=OrderSideParser.to_str_py(order.side),
            lmtPrice=order.price.as_double(),
            totalQuantity=order.quantity.as_double(),
            orderRef=order.client_order_id.value,
        )
    else:
        raise NotImplementedError(f"IB order type not implemented {type(order)} for {order}")


def ib_order_side_to_nautilus_side(action: str) -> OrderSide:
    return OrderSideParser.from_str(action.upper())


def ib_order_to_nautilus_order_type(order: IBOrder) -> OrderType:
    if isinstance(order, IBMarketOrder):
        return OrderType.MARKET
    elif isinstance(order, IBLimitOrder):
        return OrderType.LIMIT
    elif isinstance(order, IBStopOrder):
        return OrderType.STOP_MARKET
    elif isinstance(order, IBStopLimitOrder):
        return OrderType.STOP_LIMIT


def account_values_to_nautilus_account_info(
    account_values: list[AccountValue],
    account_id: str,
) -> tuple[list[AccountBalance], list[MarginBalance]]:
    """
    When querying for account information, ib_insync returns a list of individual fields for potentially multiple
    accounts. Parse these individual fields and return a list of balances and margin balances.
    """

    def group_key(x: AccountValue):
        return (x.account, x.currency)

    balances = []
    margin_balances = []
    for (account, currency), fields in groupby(
        sorted(account_values, key=group_key),
        key=group_key,
    ):
        if not (account == account_id):
            continue
        if currency in ("", "BASE"):
            # Only report in base currency
            continue
        account_fields = {f.tag: f.value for f in fields}
        if "FullAvailableFunds" in account_fields:
            total_cash = float(account_fields["NetLiquidation"])
            free = float(account_fields["FullAvailableFunds"])
            balance = AccountBalance(
                total=Money(total_cash, Currency.from_str(currency)),
                free=Money(free, Currency.from_str(currency)),
                locked=Money(total_cash - free, Currency.from_str(currency)),
            )
            balances.append(balance)
        if "InitMarginReq" in account_fields:
            margin_balance = MarginBalance(
                initial=Money(
                    float(account_fields["InitMarginReq"]),
                    currency=Currency.from_str(currency),
                ),
                maintenance=Money(
                    float(account_fields["MaintMarginReq"]),
                    currency=Currency.from_str(currency),
                ),
            )
            margin_balances.append(margin_balance)

    return balances, margin_balances
