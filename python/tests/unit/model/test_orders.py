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

import pytest

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import LimitIfTouchedOrder
from nautilus_trader.model import LimitOrder
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import MarketIfTouchedOrder
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import MarketToLimitOrder
from nautilus_trader.model import Money
from nautilus_trader.model import OrderAccepted
from nautilus_trader.model import OrderCanceled
from nautilus_trader.model import OrderDenied
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderRejected
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import OrderSubmitted
from nautilus_trader.model import OrderType
from nautilus_trader.model import PositionSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StopLimitOrder
from nautilus_trader.model import StopMarketOrder
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TrailingOffsetType
from nautilus_trader.model import TrailingStopLimitOrder
from nautilus_trader.model import TrailingStopMarketOrder
from nautilus_trader.model import TriggerType
from nautilus_trader.model import VenueOrderId


def test_market_order_construction():
    order = MarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )

    assert order.trader_id == TraderId("TRADER-001")
    assert order.strategy_id == StrategyId("S-001")
    assert order.instrument_id == InstrumentId.from_str("AUD/USD.SIM")
    assert order.client_order_id == ClientOrderId("O-001")
    assert order.side == OrderSide.BUY
    assert order.quantity == Quantity.from_int(100_000)
    assert order.time_in_force == TimeInForce.GTC
    assert order.status == OrderStatus.INITIALIZED
    assert order.is_reduce_only is False
    assert order.is_quote_quantity is False
    assert order.order_type == OrderType.MARKET


def test_market_order_str_and_repr():
    order = MarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )

    assert "BUY" in str(order)
    assert "MarketOrder" in repr(order)


def test_market_order_to_dict():
    order = MarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )

    d = order.to_dict()

    assert d["type"] == "MARKET"
    assert d["side"] == "BUY"
    assert d["quantity"] == "100000"
    assert d["status"] == "INITIALIZED"


def test_market_order_opposite_side():
    assert MarketOrder.opposite_side(OrderSide.BUY) == OrderSide.SELL
    assert MarketOrder.opposite_side(OrderSide.SELL) == OrderSide.BUY


def test_market_order_closing_side():
    assert MarketOrder.closing_side(PositionSide.LONG) == OrderSide.SELL
    assert MarketOrder.closing_side(PositionSide.SHORT) == OrderSide.BUY


def test_limit_order_construction():
    order = LimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-002"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(50_000),
        price=Price.from_str("1.00010"),
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
        expire_time=0,
        display_qty=None,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )

    assert order.trader_id == TraderId("TRADER-001")
    assert order.strategy_id == StrategyId("S-001")
    assert order.instrument_id == InstrumentId.from_str("AUD/USD.SIM")
    assert order.client_order_id == ClientOrderId("O-002")
    assert order.side == OrderSide.SELL
    assert order.quantity == Quantity.from_int(50_000)
    assert order.price == Price.from_str("1.00010")
    assert order.status == OrderStatus.INITIALIZED
    assert order.order_type == OrderType.LIMIT


def test_limit_order_str_and_repr():
    order = LimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-002"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(50_000),
        price=Price.from_str("1.00010"),
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
        expire_time=0,
        display_qty=None,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )

    assert "SELL" in str(order)
    assert "LimitOrder" in repr(order)


def test_limit_order_to_dict():
    order = LimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-002"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(50_000),
        price=Price.from_str("1.00010"),
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
        expire_time=0,
        display_qty=None,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )

    d = order.to_dict()

    assert d["type"] == "LIMIT"
    assert d["side"] == "SELL"
    assert d["price"] == "1.00010"
    assert d["status"] == "INITIALIZED"


def test_stop_market_order_construction():
    order = StopMarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-003"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        trigger_price=Price.from_str("0.99500"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert order.instrument_id == InstrumentId.from_str("AUD/USD.SIM")
    assert order.client_order_id == ClientOrderId("O-003")
    assert order.side == OrderSide.SELL
    assert order.quantity == Quantity.from_int(100_000)
    assert order.trigger_price == Price.from_str("0.99500")
    assert order.status == OrderStatus.INITIALIZED
    assert order.order_type == OrderType.STOP_MARKET


def test_stop_market_order_str_and_repr():
    order = StopMarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-003"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        trigger_price=Price.from_str("0.99500"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert "SELL" in str(order)
    assert "StopMarketOrder" in repr(order)


def test_stop_market_order_to_dict():
    order = StopMarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-003"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        trigger_price=Price.from_str("0.99500"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    d = order.to_dict()

    assert d["type"] == "STOP_MARKET"
    assert d["side"] == "SELL"
    assert d["quantity"] == "100000"
    assert d["trigger_price"] == "0.99500"
    assert d["instrument_id"] == "AUD/USD.SIM"
    assert d["status"] == "INITIALIZED"


def test_stop_limit_order_construction():
    order = StopLimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-004"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00100"),
        trigger_price=Price.from_str("1.00050"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert order.instrument_id == InstrumentId.from_str("AUD/USD.SIM")
    assert order.client_order_id == ClientOrderId("O-004")
    assert order.side == OrderSide.BUY
    assert order.quantity == Quantity.from_int(100_000)
    assert order.price == Price.from_str("1.00100")
    assert order.trigger_price == Price.from_str("1.00050")
    assert order.status == OrderStatus.INITIALIZED
    assert order.order_type == OrderType.STOP_LIMIT


def test_stop_limit_order_str_and_repr():
    order = StopLimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-004"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00100"),
        trigger_price=Price.from_str("1.00050"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert "BUY" in str(order)
    assert "StopLimitOrder" in repr(order)


def test_stop_limit_order_to_dict():
    order = StopLimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-004"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00100"),
        trigger_price=Price.from_str("1.00050"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    d = order.to_dict()

    assert d["type"] == "STOP_LIMIT"
    assert d["side"] == "BUY"
    assert d["quantity"] == "100000"
    assert d["price"] == "1.00100"
    assert d["trigger_price"] == "1.00050"
    assert d["status"] == "INITIALIZED"


def test_market_if_touched_order_construction():
    order = MarketIfTouchedOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-005"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        trigger_price=Price.from_str("0.99000"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert order.instrument_id == InstrumentId.from_str("AUD/USD.SIM")
    assert order.client_order_id == ClientOrderId("O-005")
    assert order.side == OrderSide.BUY
    assert order.quantity == Quantity.from_int(100_000)
    assert order.trigger_price == Price.from_str("0.99000")
    assert order.status == OrderStatus.INITIALIZED
    assert order.order_type == OrderType.MARKET_IF_TOUCHED


def test_market_if_touched_order_str_and_repr():
    order = MarketIfTouchedOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-005"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        trigger_price=Price.from_str("0.99000"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert "BUY" in str(order)
    assert "MarketIfTouchedOrder" in repr(order)


def test_market_if_touched_order_to_dict():
    order = MarketIfTouchedOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-005"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        trigger_price=Price.from_str("0.99000"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    d = order.to_dict()

    assert d["type"] == "MARKET_IF_TOUCHED"
    assert d["side"] == "BUY"
    assert d["quantity"] == "100000"
    assert d["trigger_price"] == "0.99000"
    assert d["status"] == "INITIALIZED"


def test_limit_if_touched_order_construction():
    order = LimitIfTouchedOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-006"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00500"),
        trigger_price=Price.from_str("1.01000"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert order.instrument_id == InstrumentId.from_str("AUD/USD.SIM")
    assert order.client_order_id == ClientOrderId("O-006")
    assert order.side == OrderSide.SELL
    assert order.quantity == Quantity.from_int(100_000)
    assert order.price == Price.from_str("1.00500")
    assert order.trigger_price == Price.from_str("1.01000")
    assert order.status == OrderStatus.INITIALIZED
    assert order.order_type == OrderType.LIMIT_IF_TOUCHED


def test_limit_if_touched_order_str_and_repr():
    order = LimitIfTouchedOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-006"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00500"),
        trigger_price=Price.from_str("1.01000"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert "SELL" in str(order)
    assert "LimitIfTouchedOrder" in repr(order)


def test_limit_if_touched_order_to_dict():
    order = LimitIfTouchedOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-006"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00500"),
        trigger_price=Price.from_str("1.01000"),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    d = order.to_dict()

    assert d["type"] == "LIMIT_IF_TOUCHED"
    assert d["side"] == "SELL"
    assert d["quantity"] == "100000"
    assert d["price"] == "1.00500"
    assert d["trigger_price"] == "1.01000"
    assert d["status"] == "INITIALIZED"


def test_market_to_limit_order_construction():
    order = MarketToLimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-007"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert order.instrument_id == InstrumentId.from_str("AUD/USD.SIM")
    assert order.client_order_id == ClientOrderId("O-007")
    assert order.side == OrderSide.BUY
    assert order.quantity == Quantity.from_int(100_000)
    assert order.status == OrderStatus.INITIALIZED
    assert order.order_type == OrderType.MARKET_TO_LIMIT


def test_market_to_limit_order_str_and_repr():
    order = MarketToLimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-007"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert "BUY" in str(order)
    assert "MarketToLimitOrder" in repr(order)


def test_market_to_limit_order_to_dict():
    order = MarketToLimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-007"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    d = order.to_dict()

    assert d["type"] == "MARKET_TO_LIMIT"
    assert d["side"] == "BUY"
    assert d["quantity"] == "100000"
    assert d["instrument_id"] == "AUD/USD.SIM"
    assert d["status"] == "INITIALIZED"


def test_trailing_stop_market_order_construction():
    order = TrailingStopMarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-008"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        trigger_price=Price.from_str("0.99000"),
        trigger_type=TriggerType.DEFAULT,
        trailing_offset=Decimal("0.00100"),
        trailing_offset_type=TrailingOffsetType.PRICE,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert order.instrument_id == InstrumentId.from_str("AUD/USD.SIM")
    assert order.client_order_id == ClientOrderId("O-008")
    assert order.side == OrderSide.SELL
    assert order.quantity == Quantity.from_int(100_000)
    assert order.trigger_price == Price.from_str("0.99000")
    assert order.trailing_offset == Decimal("0.00100")
    assert order.trailing_offset_type == TrailingOffsetType.PRICE
    assert order.status == OrderStatus.INITIALIZED
    assert order.order_type == OrderType.TRAILING_STOP_MARKET


def test_trailing_stop_market_order_str_and_repr():
    order = TrailingStopMarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-008"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        trigger_price=Price.from_str("0.99000"),
        trigger_type=TriggerType.DEFAULT,
        trailing_offset=Decimal("0.00100"),
        trailing_offset_type=TrailingOffsetType.PRICE,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert "SELL" in str(order)
    assert "TrailingStopMarketOrder" in repr(order)


def test_trailing_stop_market_order_to_dict():
    order = TrailingStopMarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-008"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        trigger_price=Price.from_str("0.99000"),
        trigger_type=TriggerType.DEFAULT,
        trailing_offset=Decimal("0.00100"),
        trailing_offset_type=TrailingOffsetType.PRICE,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    d = order.to_dict()

    assert d["type"] == "TRAILING_STOP_MARKET"
    assert d["side"] == "SELL"
    assert d["quantity"] == "100000"
    assert d["trigger_price"] == "0.99000"
    assert d["trailing_offset"] == "0.00100"
    assert d["status"] == "INITIALIZED"


def test_trailing_stop_limit_order_construction():
    order = TrailingStopLimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-009"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("0.98900"),
        trigger_price=Price.from_str("0.99000"),
        trigger_type=TriggerType.DEFAULT,
        limit_offset=Decimal("0.00100"),
        trailing_offset=Decimal("0.00200"),
        trailing_offset_type=TrailingOffsetType.PRICE,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert order.instrument_id == InstrumentId.from_str("AUD/USD.SIM")
    assert order.client_order_id == ClientOrderId("O-009")
    assert order.side == OrderSide.SELL
    assert order.quantity == Quantity.from_int(100_000)
    assert order.price == Price.from_str("0.98900")
    assert order.trigger_price == Price.from_str("0.99000")
    assert order.limit_offset == Decimal("0.00100")
    assert order.trailing_offset == Decimal("0.00200")
    assert order.trailing_offset_type == TrailingOffsetType.PRICE
    assert order.status == OrderStatus.INITIALIZED
    assert order.order_type == OrderType.TRAILING_STOP_LIMIT


def test_trailing_stop_limit_order_str_and_repr():
    order = TrailingStopLimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-009"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("0.98900"),
        trigger_price=Price.from_str("0.99000"),
        trigger_type=TriggerType.DEFAULT,
        limit_offset=Decimal("0.00100"),
        trailing_offset=Decimal("0.00200"),
        trailing_offset_type=TrailingOffsetType.PRICE,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    assert "SELL" in str(order)
    assert "TrailingStopLimitOrder" in repr(order)


def test_trailing_stop_limit_order_to_dict():
    order = TrailingStopLimitOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-009"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("0.98900"),
        trigger_price=Price.from_str("0.99000"),
        trigger_type=TriggerType.DEFAULT,
        limit_offset=Decimal("0.00100"),
        trailing_offset=Decimal("0.00200"),
        trailing_offset_type=TrailingOffsetType.PRICE,
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )

    d = order.to_dict()

    assert d["type"] == "TRAILING_STOP_LIMIT"
    assert d["side"] == "SELL"
    assert d["quantity"] == "100000"
    assert d["price"] == "0.98900"
    assert d["trigger_price"] == "0.99000"
    assert d["trailing_offset"] == "0.00200"
    assert d["limit_offset"] == "0.00100"
    assert d["status"] == "INITIALIZED"


@pytest.mark.parametrize(
    ("side", "expected"),
    [
        (OrderSide.BUY, OrderSide.SELL),
        (OrderSide.SELL, OrderSide.BUY),
    ],
)
def test_opposite_side(side, expected):
    assert MarketOrder.opposite_side(side) == expected


@pytest.mark.parametrize(
    ("position_side", "expected"),
    [
        (PositionSide.LONG, OrderSide.SELL),
        (PositionSide.SHORT, OrderSide.BUY),
    ],
)
def test_closing_side(position_side, expected):
    assert MarketOrder.closing_side(position_side) == expected


TRADER_ID = TraderId("TRADER-001")
STRATEGY_ID = StrategyId("S-001")
AUDUSD_SIM = InstrumentId.from_str("AUD/USD.SIM")
ACCOUNT_ID = AccountId("SIM-000")


def _market_order(side=OrderSide.BUY, qty=100_000, client_order_id="O-001"):
    return MarketOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=ClientOrderId(client_order_id),
        order_side=side,
        quantity=Quantity.from_int(qty),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
    )


def test_apply_submitted():
    order = _market_order()
    submitted = OrderSubmitted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=1,
        ts_init=1,
    )

    order.apply(submitted)

    assert order.status == OrderStatus.SUBMITTED
    assert order.account_id == ACCOUNT_ID
    assert len(order.events()) == 2


def test_apply_accepted():
    order = _market_order()
    submitted = OrderSubmitted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=1,
        ts_init=1,
    )
    accepted = OrderAccepted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-001"),
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=2,
        ts_init=2,
        reconciliation=False,
    )

    order.apply(submitted)
    order.apply(accepted)

    assert order.status == OrderStatus.ACCEPTED
    assert len(order.events()) == 3


def test_apply_denied():
    order = _market_order()
    denied = OrderDenied(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        reason="Exceeded rate limit",
        event_id=UUID4(),
        ts_event=1,
        ts_init=1,
    )

    order.apply(denied)

    assert order.status == OrderStatus.DENIED


def test_apply_rejected():
    order = _market_order()
    submitted = OrderSubmitted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=1,
        ts_init=1,
    )
    rejected = OrderRejected(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        account_id=ACCOUNT_ID,
        reason="Insufficient margin",
        event_id=UUID4(),
        ts_event=2,
        ts_init=2,
        reconciliation=False,
    )

    order.apply(submitted)
    order.apply(rejected)

    assert order.status == OrderStatus.REJECTED


def test_apply_canceled():
    order = _market_order()
    submitted = OrderSubmitted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=1,
        ts_init=1,
    )
    accepted = OrderAccepted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-001"),
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=2,
        ts_init=2,
        reconciliation=False,
    )
    canceled = OrderCanceled(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        event_id=UUID4(),
        ts_event=3,
        ts_init=3,
        reconciliation=False,
        venue_order_id=VenueOrderId("V-001"),
        account_id=ACCOUNT_ID,
    )

    order.apply(submitted)
    order.apply(accepted)
    order.apply(canceled)

    assert order.status == OrderStatus.CANCELED


def test_apply_filled():
    order = _market_order()
    submitted = OrderSubmitted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=1,
        ts_init=1,
    )
    accepted = OrderAccepted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-001"),
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=2,
        ts_init=2,
        reconciliation=False,
    )
    filled = OrderFilled(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-001"),
        account_id=ACCOUNT_ID,
        trade_id=TradeId("T-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("1.00000"),
        currency=Currency.from_str("USD"),
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=3,
        ts_init=3,
        reconciliation=False,
        commission=Money.from_str("2.00 USD"),
    )

    order.apply(submitted)
    order.apply(accepted)
    order.apply(filled)

    assert order.status == OrderStatus.FILLED
    assert order.quantity == Quantity.from_int(100_000)
    assert len(order.events()) == 4


def test_apply_partial_fill():
    order = _market_order()
    submitted = OrderSubmitted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=1,
        ts_init=1,
    )
    accepted = OrderAccepted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-001"),
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=2,
        ts_init=2,
        reconciliation=False,
    )
    partial = OrderFilled(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-001"),
        account_id=ACCOUNT_ID,
        trade_id=TradeId("T-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_int(50_000),
        last_px=Price.from_str("1.00000"),
        currency=Currency.from_str("USD"),
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=3,
        ts_init=3,
        reconciliation=False,
    )

    order.apply(submitted)
    order.apply(accepted)
    order.apply(partial)

    assert order.status == OrderStatus.PARTIALLY_FILLED


def test_would_reduce_only():
    order = _market_order(side=OrderSide.SELL, qty=50_000)

    assert order.would_reduce_only(PositionSide.LONG, Quantity.from_int(100_000))
    assert not order.would_reduce_only(PositionSide.SHORT, Quantity.from_int(100_000))
    assert not order.would_reduce_only(PositionSide.FLAT, Quantity.from_int(0))


def test_signed_decimal_qty():
    buy_order = _market_order(side=OrderSide.BUY, qty=100_000)
    sell_order = _market_order(side=OrderSide.SELL, qty=100_000)

    assert buy_order.signed_decimal_qty() == Decimal(100000)
    assert sell_order.signed_decimal_qty() == Decimal(-100000)


def test_order_to_dict_from_dict_roundtrip():
    order = _market_order()

    d = order.to_dict()
    restored = MarketOrder.from_dict(d)

    assert restored.client_order_id == order.client_order_id
    assert restored.side == order.side
    assert restored.quantity == order.quantity
    assert restored.status == OrderStatus.INITIALIZED
