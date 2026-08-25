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
"""
Test orders behavior.
"""

from decimal import Decimal

import pytest

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import Currency
from nautilus_trader.model import ExecAlgorithmId
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
from nautilus_trader.model import OrderInitialized
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


def test_market_order_construction() -> None:
    """
    Test market order construction.
    """
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


@pytest.mark.parametrize(
    ("metadata", "expected"),
    [
        (
            {
                "contingency_type": ContingencyType.OCO,
                "linked_order_ids": [],
            },
            "`linked_order_ids` is required for contingent orders",
        ),
        (
            {"exec_algorithm_id": ExecAlgorithmId("TWAP")},
            "`exec_spawn_id` is required when `exec_algorithm_id` is set",
        ),
    ],
)
def test_direct_market_order_rejects_invalid_metadata(metadata: object, expected: object) -> None:
    """
    Test direct market order rejects invalid metadata.
    """
    with pytest.raises(ValueError, match=expected):
        _market_order(**metadata)


@pytest.mark.parametrize(
    ("metadata", "expected"),
    [
        (
            {"contingency_type": ContingencyType.OCO},
            "`linked_order_ids` is required for contingent orders",
        ),
        (
            {
                "contingency_type": ContingencyType.OCO,
                "linked_order_ids": [],
            },
            "`linked_order_ids` is required for contingent orders",
        ),
        (
            {"exec_algorithm_id": ExecAlgorithmId("TWAP")},
            "`exec_spawn_id` is required when `exec_algorithm_id` is set",
        ),
    ],
)
def test_direct_order_initialized_rejects_invalid_metadata(
    metadata: object,
    expected: object,
) -> None:
    """
    Test direct order initialized rejects invalid metadata.
    """
    with pytest.raises(ValueError, match=expected):
        _order_initialized(**metadata)


@pytest.mark.parametrize(
    ("metadata", "expected"),
    [
        (
            {
                "contingency_type": "OCO",
                "linked_order_ids": [],
            },
            "`linked_order_ids` is required for contingent orders",
        ),
        (
            {
                "exec_algorithm_id": "TWAP",
                "exec_spawn_id": None,
            },
            "`exec_spawn_id` is required when `exec_algorithm_id` is set",
        ),
    ],
)
def test_market_order_reconstruction_rejects_invalid_metadata(
    metadata: object,
    expected: object,
) -> None:
    """
    Test market order reconstruction rejects invalid metadata.
    """
    values = _order_initialized().to_dict()
    values.update(metadata)
    event = OrderInitialized.from_dict(values)

    with pytest.raises(ValueError, match=expected):
        MarketOrder.create(event)


def test_market_order_str_and_repr() -> None:
    """
    Test market order str and repr.
    """
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


def test_market_order_to_dict() -> None:
    """
    Test market order to dict.
    """
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


def test_market_order_opposite_side() -> None:
    """
    Test market order opposite side.
    """
    assert MarketOrder.opposite_side(OrderSide.BUY) == OrderSide.SELL
    assert MarketOrder.opposite_side(OrderSide.SELL) == OrderSide.BUY


def test_market_order_closing_side() -> None:
    """
    Test market order closing side.
    """
    assert MarketOrder.closing_side(PositionSide.LONG) == OrderSide.SELL
    assert MarketOrder.closing_side(PositionSide.SHORT) == OrderSide.BUY


def test_limit_order_construction() -> None:
    """
    Test limit order construction.
    """
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


def test_limit_order_str_and_repr() -> None:
    """
    Test limit order str and repr.
    """
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


def test_limit_order_to_dict() -> None:
    """
    Test limit order to dict.
    """
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


def test_stop_market_order_construction() -> None:
    """
    Test stop market order construction.
    """
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


def test_stop_market_order_str_and_repr() -> None:
    """
    Test stop market order str and repr.
    """
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


def test_stop_market_order_to_dict() -> None:
    """
    Test stop market order to dict.
    """
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


def test_stop_limit_order_construction() -> None:
    """
    Test stop limit order construction.
    """
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


def test_stop_limit_order_str_and_repr() -> None:
    """
    Test stop limit order str and repr.
    """
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


def test_stop_limit_order_to_dict() -> None:
    """
    Test stop limit order to dict.
    """
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


def test_market_if_touched_order_construction() -> None:
    """
    Test market if touched order construction.
    """
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


def test_market_if_touched_order_str_and_repr() -> None:
    """
    Test market if touched order str and repr.
    """
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


def test_market_if_touched_order_to_dict() -> None:
    """
    Test market if touched order to dict.
    """
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


def test_limit_if_touched_order_construction() -> None:
    """
    Test limit if touched order construction.
    """
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


def test_limit_if_touched_order_str_and_repr() -> None:
    """
    Test limit if touched order str and repr.
    """
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


def test_limit_if_touched_order_to_dict() -> None:
    """
    Test limit if touched order to dict.
    """
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


def test_market_to_limit_order_construction() -> None:
    """
    Test market to limit order construction.
    """
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


def test_market_to_limit_order_str_and_repr() -> None:
    """
    Test market to limit order str and repr.
    """
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


def test_market_to_limit_order_to_dict() -> None:
    """
    Test market to limit order to dict.
    """
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


def test_trailing_stop_market_order_construction() -> None:
    """
    Test trailing stop market order construction.
    """
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


def test_trailing_stop_market_order_str_and_repr() -> None:
    """
    Test trailing stop market order str and repr.
    """
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


def test_trailing_stop_market_order_to_dict() -> None:
    """
    Test trailing stop market order to dict.
    """
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


def test_trailing_stop_limit_order_construction() -> None:
    """
    Test trailing stop limit order construction.
    """
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


def test_trailing_stop_limit_order_str_and_repr() -> None:
    """
    Test trailing stop limit order str and repr.
    """
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


def test_trailing_stop_limit_order_to_dict() -> None:
    """
    Test trailing stop limit order to dict.
    """
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
def test_opposite_side(side: object, expected: object) -> None:
    """
    Test opposite side.
    """
    assert MarketOrder.opposite_side(side) == expected


@pytest.mark.parametrize(
    ("position_side", "expected"),
    [
        (PositionSide.LONG, OrderSide.SELL),
        (PositionSide.SHORT, OrderSide.BUY),
    ],
)
def test_closing_side(position_side: object, expected: object) -> None:
    """
    Test closing side.
    """
    assert MarketOrder.closing_side(position_side) == expected


TRADER_ID = TraderId("TRADER-001")
STRATEGY_ID = StrategyId("S-001")
AUDUSD_SIM = InstrumentId.from_str("AUD/USD.SIM")
ACCOUNT_ID = AccountId("SIM-000")


def _market_order(
    side: object = OrderSide.BUY,
    qty: object = 100_000,
    client_order_id: ClientOrderId = "O-001",
    **metadata: object,
) -> object:
    values = {
        "trader_id": TRADER_ID,
        "strategy_id": STRATEGY_ID,
        "instrument_id": AUDUSD_SIM,
        "client_order_id": ClientOrderId(client_order_id),
        "order_side": side,
        "quantity": Quantity.from_int(qty),
        "init_id": UUID4(),
        "ts_init": 0,
        "time_in_force": TimeInForce.GTC,
        "reduce_only": False,
        "quote_quantity": False,
    }
    values.update(metadata)
    return MarketOrder(**values)


def _order_initialized(**metadata: object) -> object:
    values = {
        "trader_id": TRADER_ID,
        "strategy_id": STRATEGY_ID,
        "instrument_id": AUDUSD_SIM,
        "client_order_id": ClientOrderId("O-001"),
        "order_side": OrderSide.BUY,
        "order_type": OrderType.MARKET,
        "quantity": Quantity.from_int(100_000),
        "time_in_force": TimeInForce.GTC,
        "post_only": False,
        "reduce_only": False,
        "quote_quantity": False,
        "reconciliation": False,
        "event_id": UUID4(),
        "ts_event": 0,
        "ts_init": 0,
        "contingency_type": ContingencyType.NO_CONTINGENCY,
    }
    values.update(metadata)
    return OrderInitialized(**values)


def _limit_order(client_order_id: ClientOrderId = "O-002") -> object:
    return LimitOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=ClientOrderId(client_order_id),
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
    )


def _stop_market_order(client_order_id: ClientOrderId = "O-003") -> object:
    return StopMarketOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=ClientOrderId(client_order_id),
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


def _stop_limit_order(client_order_id: ClientOrderId = "O-004") -> object:
    return StopLimitOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=ClientOrderId(client_order_id),
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


def _market_if_touched_order(client_order_id: ClientOrderId = "O-005") -> object:
    return MarketIfTouchedOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=ClientOrderId(client_order_id),
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


def _limit_if_touched_order(client_order_id: ClientOrderId = "O-006") -> object:
    return LimitIfTouchedOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=ClientOrderId(client_order_id),
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


def _market_to_limit_order(client_order_id: ClientOrderId = "O-007") -> object:
    return MarketToLimitOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=ClientOrderId(client_order_id),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        time_in_force=TimeInForce.GTC,
        post_only=False,
        reduce_only=False,
        quote_quantity=False,
        init_id=UUID4(),
        ts_init=0,
    )


def _trailing_stop_market_order(client_order_id: ClientOrderId = "O-008") -> object:
    return TrailingStopMarketOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=ClientOrderId(client_order_id),
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


def _trailing_stop_limit_order(client_order_id: ClientOrderId = "O-009") -> object:
    return TrailingStopLimitOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD_SIM,
        client_order_id=ClientOrderId(client_order_id),
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


ORDER_FACTORIES = [
    _market_order,
    _limit_order,
    _stop_market_order,
    _stop_limit_order,
    _market_if_touched_order,
    _limit_if_touched_order,
    _market_to_limit_order,
    _trailing_stop_market_order,
    _trailing_stop_limit_order,
]


@pytest.mark.parametrize(
    "order_factory",
    [
        _limit_if_touched_order,
        _market_to_limit_order,
        _stop_limit_order,
        _stop_market_order,
    ],
)
@pytest.mark.parametrize(
    ("metadata", "expected"),
    [
        (
            {
                "contingency_type": "OCO",
                "linked_order_ids": [],
            },
            "`linked_order_ids` is required for contingent orders",
        ),
        (
            {
                "exec_algorithm_id": "TWAP",
                "exec_spawn_id": None,
            },
            "`exec_spawn_id` is required when `exec_algorithm_id` is set",
        ),
    ],
)
def test_order_from_dict_rejects_invalid_metadata(
    order_factory: object,
    metadata: object,
    expected: object,
) -> None:
    """
    Test order from dict rejects invalid metadata.
    """
    order = order_factory()
    values = order.to_dict()
    values.update(metadata)

    with pytest.raises(ValueError, match=expected):
        type(order).from_dict(values)


@pytest.mark.parametrize("order_factory", ORDER_FACTORIES)
def test_order_inspection_properties_are_consistent(order_factory: object) -> None:
    """
    Test order inspection properties are consistent.
    """
    order = order_factory()

    assert order.avg_px is None
    assert order.event_count == 1
    assert order.is_buy is (order.side == OrderSide.BUY)
    assert order.is_sell is (order.side == OrderSide.SELL)
    assert order.is_canceled is False
    assert order.is_inflight is False
    assert order.is_open is False
    assert order.is_closed is False
    assert order.is_pending_cancel is False
    assert order.is_pending_update is False
    assert isinstance(order.init_event, OrderInitialized)
    assert isinstance(order.last_event, OrderInitialized)
    assert order.leaves_qty == order.quantity
    assert order.filled_qty == Quantity.from_int(0)
    assert order.overfill_qty == Quantity.from_int(0)
    assert order.slippage is None
    assert order.trade_ids == []
    assert order.venue_order_ids == []
    assert order.ts_submitted is None
    assert order.ts_accepted is None
    assert order.ts_closed is None
    assert order.ts_last == order.ts_init
    assert order.venue_order_id is None
    assert order.position_id is None
    assert order.account_id is None
    assert order.last_trade_id is None
    assert order.liquidity_side == LiquiditySide.NO_LIQUIDITY_SIDE
    assert order.is_active_local is True
    assert order.is_emulated is False
    assert order.is_primary is False
    assert order.is_spawned is False


@pytest.mark.parametrize(
    "order_factory",
    [
        _stop_market_order,
        _stop_limit_order,
        _market_if_touched_order,
        _limit_if_touched_order,
        _trailing_stop_market_order,
        _trailing_stop_limit_order,
    ],
)
def test_trigger_order_inspection_properties_are_consistent(order_factory: object) -> None:
    """
    Test trigger order inspection properties are consistent.
    """
    order = order_factory()

    assert order.is_triggered is False
    assert order.ts_triggered is None


@pytest.mark.parametrize(
    "order_factory",
    [_trailing_stop_market_order, _trailing_stop_limit_order],
)
def test_trailing_order_activation_property_is_consistent(order_factory: object) -> None:
    """
    Test trailing order activation property is consistent.
    """
    order = order_factory()

    assert order.is_activated is False


@pytest.mark.parametrize(
    "order_factory",
    ORDER_FACTORIES,
)
def test_apply_rejects_unsupported_event(order_factory: object) -> None:
    """
    Test apply rejects unsupported event.
    """
    order = order_factory()

    with pytest.raises(ValueError, match="OrderEventAny"):
        order.apply(object())


def test_apply_submitted() -> None:
    """
    Test apply submitted.
    """
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


def test_apply_accepted() -> None:
    """
    Test apply accepted.
    """
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


def test_apply_denied() -> None:
    """
    Test apply denied.
    """
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


def test_apply_rejected() -> None:
    """
    Test apply rejected.
    """
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


def test_apply_canceled() -> None:
    """
    Test apply canceled.
    """
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


def test_apply_filled() -> None:
    """
    Test apply filled.
    """
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
    assert order.avg_px == Decimal("1.00000")
    assert order.event_count == 4
    assert isinstance(order.last_event, OrderFilled)
    assert order.leaves_qty == Quantity.from_int(0)
    assert order.filled_qty == Quantity.from_int(100_000)
    assert order.trade_ids == [TradeId("T-001")]
    assert order.venue_order_ids == [VenueOrderId("V-001")]
    assert order.ts_submitted == 1
    assert order.ts_accepted == 2
    assert order.ts_closed == 3


def test_apply_partial_fill() -> None:
    """
    Test apply partial fill.
    """
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


def test_would_reduce_only() -> None:
    """
    Test would reduce only.
    """
    order = _market_order(side=OrderSide.SELL, qty=50_000)

    assert order.would_reduce_only(PositionSide.LONG, Quantity.from_int(100_000))
    assert not order.would_reduce_only(PositionSide.SHORT, Quantity.from_int(100_000))
    assert not order.would_reduce_only(PositionSide.FLAT, Quantity.from_int(0))


def test_signed_decimal_qty() -> None:
    """
    Test signed decimal qty.
    """
    buy_order = _market_order(side=OrderSide.BUY, qty=100_000)
    sell_order = _market_order(side=OrderSide.SELL, qty=100_000)

    assert buy_order.signed_decimal_qty() == Decimal(100000)
    assert sell_order.signed_decimal_qty() == Decimal(-100000)


def test_order_to_dict_from_dict_roundtrip() -> None:
    """
    Test order to dict from dict roundtrip.
    """
    order = _market_order()

    d = order.to_dict()
    restored = MarketOrder.from_dict(d)

    assert restored.client_order_id == order.client_order_id
    assert restored.side == order.side
    assert restored.quantity == order.quantity
    assert restored.status == OrderStatus.INITIALIZED
