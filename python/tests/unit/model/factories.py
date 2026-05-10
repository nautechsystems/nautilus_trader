# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import FillReport
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import Money
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderInitialized
from nautilus_trader.model import OrderListId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import OrderStatusReport
from nautilus_trader.model import OrderType
from nautilus_trader.model import OwnBookOrder
from nautilus_trader.model import PositionId
from nautilus_trader.model import PositionSide
from nautilus_trader.model import PositionStatusReport
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TrailingOffsetType
from nautilus_trader.model import TriggerType
from nautilus_trader.model import VenueOrderId


def make_fill_report(instrument_id):
    return FillReport(
        account_id=AccountId("SIM-001"),
        instrument_id=instrument_id,
        venue_order_id=VenueOrderId("1"),
        trade_id=TradeId("T-1"),
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("1.00010"),
        commission=Money.from_str("2.00 USD"),
        liquidity_side=LiquiditySide.TAKER,
        ts_event=10,
        ts_init=11,
        client_order_id=ClientOrderId("O-1"),
        venue_position_id=PositionId("P-1"),
    )


def make_market_order_snapshot_values(instrument_id):
    order = MarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument_id,
        client_order_id=ClientOrderId("O-9"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )
    values = order.to_dict()
    values["order_type"] = values["type"]
    values["order_side"] = values["side"]
    values["expire_time"] = values.get("expire_time_ns")
    values["commissions"] = values.get("commissions") or []
    values["is_post_only"] = values.get("is_post_only", False)
    values["is_reduce_only"] = values.get("is_reduce_only", False)
    values["is_quote_quantity"] = values.get("is_quote_quantity", False)
    return values


def make_order_initialized(instrument_id):
    return OrderInitialized(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument_id,
        client_order_id=ClientOrderId("O-1"),
        order_side=OrderSide.BUY,
        order_type=OrderType.STOP_LIMIT,
        quantity=Quantity.from_int(100_000),
        time_in_force=TimeInForce.GTC,
        post_only=True,
        reduce_only=False,
        quote_quantity=False,
        reconciliation=False,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        price=Price.from_str("1.00010"),
        trigger_price=Price.from_str("0.99990"),
        trigger_type=TriggerType.BID_ASK,
        expire_time=3,
        display_qty=Quantity.from_int(50_000),
        emulation_trigger=TriggerType.LAST_PRICE,
        trigger_instrument_id=instrument_id,
        contingency_type=ContingencyType.OCO,
        order_list_id=OrderListId("L-1"),
        linked_order_ids=[ClientOrderId("O-2")],
        parent_order_id=ClientOrderId("O-P"),
        exec_algorithm_id=ExecAlgorithmId("VWAP"),
        exec_algorithm_params={"speed": "fast"},
        exec_spawn_id=ClientOrderId("O-X"),
        tags=["tag-1", "tag-2"],
    )


def make_order_status_report(instrument_id, include_optionals):
    kwargs = {
        "account_id": AccountId("SIM-001"),
        "instrument_id": instrument_id,
        "venue_order_id": VenueOrderId("1"),
        "order_side": OrderSide.BUY,
        "order_type": OrderType.LIMIT,
        "time_in_force": TimeInForce.GTC,
        "order_status": OrderStatus.ACCEPTED,
        "quantity": Quantity.from_int(100_000),
        "filled_qty": Quantity.from_int(25_000),
        "ts_accepted": 10,
        "ts_last": 20,
        "ts_init": 30,
    }

    if not include_optionals:
        return OrderStatusReport(**kwargs)

    return OrderStatusReport(
        **kwargs,
        client_order_id=ClientOrderId("O-1"),
        report_id=UUID4(),
        order_list_id=OrderListId("L-1"),
        venue_position_id=PositionId("P-1"),
        linked_order_ids=[ClientOrderId("O-2")],
        parent_order_id=ClientOrderId("O-P"),
        contingency_type=ContingencyType.OTO,
        expire_time=40,
        price=Price.from_str("1.00010"),
        trigger_price=Price.from_str("0.99990"),
        trigger_type=TriggerType.BID_ASK,
        limit_offset=Decimal("0.0001"),
        trailing_offset=Decimal("0.0002"),
        trailing_offset_type=TrailingOffsetType.PRICE,
        avg_px=Decimal("1.00005"),
        display_qty=Quantity.from_int(50_000),
        post_only=True,
        reduce_only=False,
        cancel_reason="none",
        ts_triggered=50,
    )


def make_own_order(
    side=OrderSide.BUY,
    price="1.00000",
    size=100_000,
    client_order_id="O-001",
    status=OrderStatus.ACCEPTED,
    ts_last=0,
    ts_accepted=0,
    ts_submitted=0,
    ts_init=0,
):
    return OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId(client_order_id),
        side=side,
        price=Price.from_str(price),
        size=Quantity.from_int(size),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=status,
        ts_last=ts_last,
        ts_accepted=ts_accepted,
        ts_submitted=ts_submitted,
        ts_init=ts_init,
    )


def make_position_fill(instrument):
    return OrderFilled(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-1"),
        venue_order_id=VenueOrderId("1"),
        account_id=AccountId("SIM-001"),
        trade_id=TradeId("T-1"),
        order_side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("1.00010"),
        currency=instrument.quote_currency,
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=10,
        ts_init=11,
        reconciliation=False,
        position_id=PositionId("P-1"),
        commission=Money.from_str("2.00 USD"),
    )


def make_position_status_report(instrument_id):
    return PositionStatusReport(
        account_id=AccountId("SIM-001"),
        instrument_id=instrument_id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(100_000),
        ts_last=10,
        ts_init=20,
        report_id=UUID4(),
        venue_position_id=PositionId("P-1"),
        avg_px_open=Decimal("1.00010"),
    )
