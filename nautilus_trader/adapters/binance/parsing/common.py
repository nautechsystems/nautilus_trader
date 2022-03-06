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
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import OrderTypeParser
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.data import Order
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


def parse_book_snapshot(
    instrument_id: InstrumentId, msg: Dict, update_id: int, ts_init: int
) -> OrderBookSnapshot:
    ts_event: int = ts_init

    return OrderBookSnapshot(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        bids=[[float(o[0]), float(o[1])] for o in msg.get("bids")],
        asks=[[float(o[0]), float(o[1])] for o in msg.get("asks")],
        ts_event=ts_event,
        ts_init=ts_init,
        update_id=update_id,
    )


def parse_balances_spot(
    raw_balances: List[Dict[str, str]],
    asset_key: str,
    free_key: str,
    locked_key: str,
) -> List[AccountBalance]:
    parsed_balances: Dict[Currency, Tuple[Decimal, Decimal, Decimal]] = {}
    for b in raw_balances:
        currency = Currency.from_str(b[asset_key])
        free = Decimal(b[free_key])
        locked = Decimal(b[locked_key])
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


def parse_order_type_spot(order_type: str) -> OrderType:
    if order_type in ("STOP", "STOP_LOSS"):
        return OrderType.STOP_MARKET
    elif order_type == "STOP_LOSS_LIMIT":
        return OrderType.STOP_LIMIT
    elif order_type == "TAKE_PROFIT":
        return OrderType.LIMIT
    elif order_type == "TAKE_PROFIT_LIMIT":
        return OrderType.STOP_LIMIT
    elif order_type == "TAKE_PROFIT_MARKET":
        return OrderType.MARKET_IF_TOUCHED
    elif order_type == "LIMIT_MAKER":
        return OrderType.LIMIT
    else:
        return OrderTypeParser.from_str_py(order_type)


def binance_order_type_spot(order: Order) -> str:
    if order.type == OrderType.MARKET:
        return "MARKET"
    elif order.type == OrderType.LIMIT:
        if order.is_post_only:
            return "LIMIT_MAKER"
        else:
            return "LIMIT"
    elif order.type == OrderType.STOP_LIMIT:
        return "STOP_LOSS_LIMIT"
    elif order.type == OrderType.LIMIT_IF_TOUCHED:
        return "TAKE_PROFIT_LIMIT"
    else:  # pragma: no cover (design-time error)
        raise RuntimeError("invalid order type")


def binance_order_type_futures(order: Order) -> str:
    if order.type == OrderType.MARKET:
        return "MARKET"
    elif order.type == OrderType.LIMIT:
        return "LIMIT"
    elif order.type == OrderType.STOP_MARKET:
        return "STOP_MARKET"
    elif order.type == OrderType.STOP_LIMIT:
        return "STOP"
    elif order.type == OrderType.MARKET_IF_TOUCHED:
        return "TAKE_PROFIT_MARKET"
    elif order.type == OrderType.LIMIT_IF_TOUCHED:
        return "TAKE_PROFIT"
    elif order.type == OrderType.TRAILING_STOP_MARKET:
        return "TRAILING_STOP_MARKET"
    else:  # pragma: no cover (design-time error)
        raise RuntimeError("invalid order type")


def parse_order_type_futures(order_type: str) -> OrderType:
    if order_type == "STOP":
        return OrderType.STOP_LIMIT
    elif order_type == "STOP_LOSS_LIMIT":
        return OrderType.STOP_LIMIT
    elif order_type == "TAKE_PROFIT":
        return OrderType.LIMIT_IF_TOUCHED
    elif order_type == "TAKE_PROFIT_LIMIT":
        return OrderType.STOP_LIMIT
    elif order_type == "TAKE_PROFIT_MARKET":
        return OrderType.MARKET_IF_TOUCHED
    else:
        return OrderType[order_type]


def parse_order_status(status: str) -> OrderStatus:
    if status == "NEW":
        return OrderStatus.ACCEPTED
    elif status == "CANCELED":
        return OrderStatus.CANCELED
    elif status == "PARTIALLY_FILLED":
        return OrderStatus.PARTIALLY_FILLED
    elif status == "FILLED":
        return OrderStatus.FILLED
    elif status == "EXPIRED":
        return OrderStatus.EXPIRED
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"unrecognized order status, was {status}")


def parse_time_in_force(time_in_force: str) -> TimeInForce:
    if time_in_force == "GTX":
        return TimeInForce.GTC
    else:
        return TimeInForce[time_in_force]


def parse_trigger_type(working_type: str) -> TriggerType:
    if working_type == "CONTRACT_PRICE":
        return TriggerType.LAST
    elif working_type == "MARK_PRICE":
        return TriggerType.MARK
    else:  # pragma: no cover (design-time error)
        return TriggerType.NONE
