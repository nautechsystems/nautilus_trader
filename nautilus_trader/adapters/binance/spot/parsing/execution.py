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
from typing import Any, Dict, List, Tuple

from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotOrderType
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import Order


def parse_balances(
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


def parse_time_in_force(time_in_force: str) -> TimeInForce:
    if time_in_force == "GTX":
        return TimeInForce.GTC
    else:
        return TimeInForce[time_in_force]


def parse_order_status(status: BinanceOrderStatus) -> OrderStatus:
    if status == BinanceOrderStatus.NEW:
        return OrderStatus.ACCEPTED
    elif status == BinanceOrderStatus.CANCELED:
        return OrderStatus.CANCELED
    elif status == BinanceOrderStatus.PARTIALLY_FILLED:
        return OrderStatus.PARTIALLY_FILLED
    elif status == BinanceOrderStatus.FILLED:
        return OrderStatus.FILLED
    elif status == BinanceOrderStatus.EXPIRED:
        return OrderStatus.EXPIRED
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"unrecognized order status, was {status}")


def parse_order_type(order_type: BinanceSpotOrderType) -> OrderType:
    if order_type == BinanceSpotOrderType.STOP:
        return OrderType.STOP_MARKET
    elif order_type == BinanceSpotOrderType.STOP_LOSS:
        return OrderType.STOP_MARKET
    elif order_type == BinanceSpotOrderType.STOP_LOSS_LIMIT:
        return OrderType.STOP_LIMIT
    elif order_type == BinanceSpotOrderType.TAKE_PROFIT:
        return OrderType.LIMIT
    elif order_type == BinanceSpotOrderType.TAKE_PROFIT_LIMIT:
        return OrderType.STOP_LIMIT
    elif order_type == BinanceSpotOrderType.LIMIT_MAKER:
        return OrderType.LIMIT
    elif order_type == BinanceSpotOrderType.LIMIT:
        return OrderType.LIMIT
    else:
        return OrderType.MARKET


def binance_order_type(order: Order) -> BinanceSpotOrderType:
    if order.type == OrderType.MARKET:
        return BinanceSpotOrderType.MARKET
    elif order.type == OrderType.LIMIT:
        if order.is_post_only:
            return BinanceSpotOrderType.LIMIT_MAKER
        else:
            return BinanceSpotOrderType.LIMIT
    elif order.type == OrderType.STOP_LIMIT:
        return BinanceSpotOrderType.STOP_LOSS_LIMIT
    elif order.type == OrderType.LIMIT_IF_TOUCHED:
        return BinanceSpotOrderType.TAKE_PROFIT_LIMIT
    else:  # pragma: no cover (design-time error)
        raise RuntimeError("invalid order type")


def parse_order_report_http(
    account_id: AccountId,
    instrument_id: InstrumentId,
    data: Dict[str, Any],
    report_id: UUID4,
    ts_init: int,
) -> OrderStatusReport:
    client_id_str = data.get("clientOrderId")
    order_type = data["type"].upper()
    price = data.get("price")
    trigger_price = Decimal(data["stopPrice"])
    avg_px = Decimal(data["price"])
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=instrument_id,
        client_order_id=ClientOrderId(client_id_str) if client_id_str is not None else None,
        venue_order_id=VenueOrderId(str(data["orderId"])),
        order_side=OrderSide[data["side"].upper()],
        order_type=parse_order_type(order_type),
        time_in_force=parse_time_in_force(data["timeInForce"].upper()),
        order_status=parse_order_status(BinanceOrderStatus(data["status"].upper())),
        price=Price.from_str(price) if price is not None else None,
        quantity=Quantity.from_str(data["origQty"]),
        filled_qty=Quantity.from_str(data["executedQty"]),
        avg_px=avg_px if avg_px > 0 else None,
        post_only=order_type == "LIMIT_MAKER",
        reduce_only=False,
        report_id=report_id,
        ts_accepted=millis_to_nanos(data["time"]),
        ts_last=millis_to_nanos(data["updateTime"]),
        ts_init=ts_init,
        trigger_price=Price.from_str(str(trigger_price)) if trigger_price > 0 else None,
        trigger_type=TriggerType.LAST if trigger_price > 0 else TriggerType.NONE,
    )


def parse_trade_report_http(
    account_id: AccountId,
    instrument_id: InstrumentId,
    data: Dict[str, Any],
    report_id: UUID4,
    ts_init: int,
) -> TradeReport:
    return TradeReport(
        account_id=account_id,
        instrument_id=instrument_id,
        venue_order_id=VenueOrderId(str(data["orderId"])),
        trade_id=TradeId(str(data["id"])),
        order_side=OrderSide.BUY if data["isBuyer"] else OrderSide.SELL,
        last_qty=Quantity.from_str(data["qty"]),
        last_px=Price.from_str(data["price"]),
        commission=Money(data["commission"], Currency.from_str(data["commissionAsset"])),
        liquidity_side=LiquiditySide.MAKER if data["isMaker"] else LiquiditySide.TAKER,
        report_id=report_id,
        ts_event=millis_to_nanos(data["time"]),
        ts_init=ts_init,
    )
