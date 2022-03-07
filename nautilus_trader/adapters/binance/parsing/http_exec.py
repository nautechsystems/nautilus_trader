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
from typing import Any, Dict, List

from nautilus_trader.adapters.binance.messages.futures.order import BinanceFuturesOrderMsg
from nautilus_trader.adapters.binance.parsing.common import parse_balances_futures
from nautilus_trader.adapters.binance.parsing.common import parse_balances_spot
from nautilus_trader.adapters.binance.parsing.common import parse_margins
from nautilus_trader.adapters.binance.parsing.common import parse_order_status
from nautilus_trader.adapters.binance.parsing.common import parse_order_type_futures
from nautilus_trader.adapters.binance.parsing.common import parse_order_type_spot
from nautilus_trader.adapters.binance.parsing.common import parse_time_in_force
from nautilus_trader.adapters.binance.parsing.common import parse_trigger_type
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def parse_account_balances_spot_http(raw_balances: List[Dict[str, str]]) -> List[AccountBalance]:
    return parse_balances_spot(raw_balances, "asset", "free", "locked")


def parse_account_balances_futures_http(raw_balances: List[Dict[str, str]]) -> List[AccountBalance]:
    return parse_balances_futures(
        raw_balances, "asset", "availableBalance", "initialMargin", "maintMargin"
    )


def parse_account_margins_http(raw_balances: List[Dict[str, str]]) -> List[MarginBalance]:
    return parse_margins(raw_balances, "asset", "initialMargin", "maintMargin")


def parse_order_report_spot_http(
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
        order_type=parse_order_type_spot(order_type),
        time_in_force=parse_time_in_force(data["timeInForce"].upper()),
        order_status=parse_order_status(data["status"].upper()),
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


def parse_order_report_futures_http(
    account_id: AccountId,
    instrument_id: InstrumentId,
    msg: BinanceFuturesOrderMsg,
    report_id: UUID4,
    ts_init: int,
) -> OrderStatusReport:
    price = Decimal(msg.price)
    trigger_price = Decimal(msg.stopPrice)
    avg_px = Decimal(msg.avgPrice)
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=instrument_id,
        client_order_id=ClientOrderId(msg.clientOrderId) if msg.clientOrderId != "" else None,
        venue_order_id=VenueOrderId(str(msg.orderId)),
        order_side=OrderSide[msg.side.upper()],
        order_type=parse_order_type_futures(msg.type.upper()),
        time_in_force=parse_time_in_force(msg.timeInForce.upper()),
        order_status=parse_order_status(msg.status.upper()),
        price=Price.from_str(msg.price) if price is not None else None,
        quantity=Quantity.from_str(msg.origQty),
        filled_qty=Quantity.from_str(msg.executedQty),
        avg_px=avg_px if avg_px > 0 else None,
        post_only=msg.timeInForce == "GTX",
        reduce_only=msg.reduceOnly,
        report_id=report_id,
        ts_accepted=millis_to_nanos(msg.time),
        ts_last=millis_to_nanos(msg.updateTime),
        ts_init=ts_init,
        trigger_price=Price.from_str(str(trigger_price)) if trigger_price > 0 else None,
        trigger_type=parse_trigger_type(msg.workingType),
    )


def parse_trade_report_spot_http(
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


def parse_trade_report_futures_http(
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
        order_side=OrderSide[data["side"].upper()],
        last_qty=Quantity.from_str(data["qty"]),
        last_px=Price.from_str(data["price"]),
        commission=Money(data["commission"], Currency.from_str(data["commissionAsset"])),
        liquidity_side=LiquiditySide.MAKER if data["maker"] else LiquiditySide.TAKER,
        report_id=report_id,
        ts_event=millis_to_nanos(data["time"]),
        ts_init=ts_init,
    )


def parse_position_report_futures_http(
    account_id: AccountId,
    instrument_id: InstrumentId,
    data: Dict[str, Any],
    report_id: UUID4,
    ts_init: int,
) -> PositionStatusReport:
    net_size = Decimal(data["positionAmt"])
    return PositionStatusReport(
        account_id=account_id,
        instrument_id=instrument_id,
        position_side=PositionSide.LONG if net_size > 0 else PositionSide.SHORT,
        quantity=Quantity.from_str(str(abs(net_size))),
        report_id=report_id,
        ts_last=ts_init,
        ts_init=ts_init,
    )
