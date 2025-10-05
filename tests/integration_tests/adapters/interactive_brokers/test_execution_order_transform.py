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

import pytest

from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.test_kit.stubs.data import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


_AAPL = TestInstrumentProvider.equity("AAPL", "NASDAQ")
_EURUSD = TestInstrumentProvider.default_fx_ccy("EUR/USD", Venue("IDEALPRO"))


# fmt: off
@pytest.mark.parametrize(
    "expected_order_type, expected_tif, nautilus_order",
    [
        ("MKT", "GTC", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.GTC)),
        ("MKT", "DAY", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.DAY)),
        ("MKT", "IOC", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.IOC)),
        ("MKT", "FOK", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.FOK)),
        ("MKT", "OPG", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.AT_THE_OPEN)),
        ("MOC", "DAY", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.AT_THE_CLOSE)),
    ],
)
@pytest.mark.asyncio
async def test_transform_order_to_ib_order_market(
    expected_order_type,
    expected_tif,
    nautilus_order,
    exec_client,
):
    # Arrange
    await exec_client._instrument_provider.load_async(nautilus_order.instrument_id)

    # Act
    ib_order = exec_client._transform_order_to_ib_order(nautilus_order)

    # Assert
    assert (
        ib_order.orderType == expected_order_type
    ), f"{expected_order_type=}, but got {ib_order.orderType=}"
    assert ib_order.tif == expected_tif, f"{expected_tif=}, but got {ib_order.tif=}"
# fmt: on


# fmt: off
@pytest.mark.parametrize(
    "expected_order_type, expected_tif, nautilus_order",
    [
        ("LMT", "GTC", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.GTC)),
        ("LMT", "DAY", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.DAY)),
        ("LMT", "IOC", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.IOC)),
        ("LMT", "FOK", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.FOK)),
        ("LMT", "OPG", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.AT_THE_OPEN)),
        ("LOC", "DAY", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.AT_THE_CLOSE)),
    ],
)
@pytest.mark.asyncio
async def test_transform_order_to_ib_order_limit(
    expected_order_type,
    expected_tif,
    nautilus_order,
    exec_client,
):
    # Arrange
    await exec_client._instrument_provider.load_async(nautilus_order.instrument_id)

    # Act
    ib_order = exec_client._transform_order_to_ib_order(nautilus_order)

    # Assert
    assert (
        ib_order.orderType == expected_order_type
    ), f"{expected_order_type=}, but got {ib_order.orderType=}"
    assert ib_order.tif == expected_tif, f"{expected_tif=}, but got {ib_order.tif=}"
# fmt: on


@pytest.mark.asyncio
async def test_transform_order_to_ib_order_oco_orders(exec_client):
    """
    Test that OCO orders with explicit OCA tags are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    order_list_id = OrderListId("OL-123")
    parent_order_id = ClientOrderId("PARENT-1")
    tp_order_id = ClientOrderId("TP-1")
    sl_order_id = ClientOrderId("SL-1")

    # Create explicit OCA tags for OCO orders
    expected_oca_group = f"OCA_{order_list_id.value}"
    oca_tags = IBOrderTags(ocaGroup=expected_oca_group, ocaType=1)

    # Create take-profit order with OCO contingency and explicit OCA tags
    tp_order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=tp_order_id,
        order_side=OrderSide.SELL,
        quantity=instrument.make_qty(100),
        price=instrument.make_price(1.1000),
        time_in_force=TimeInForce.GTC,
        expire_time_ns=0,
        init_id=UUID4(),
        ts_init=0,
        post_only=False,
        reduce_only=True,
        display_qty=None,
        contingency_type=ContingencyType.OCO,
        order_list_id=order_list_id,
        linked_order_ids=[sl_order_id],
        parent_order_id=parent_order_id,
        tags=[oca_tags.value],
    )

    # Create stop-loss order with OCO contingency and explicit OCA tags
    sl_order = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=sl_order_id,
        order_side=OrderSide.SELL,
        quantity=instrument.make_qty(100),
        trigger_price=instrument.make_price(1.0500),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        init_id=UUID4(),
        ts_init=0,
        reduce_only=True,
        contingency_type=ContingencyType.OCO,
        order_list_id=order_list_id,
        linked_order_ids=[tp_order_id],
        parent_order_id=parent_order_id,
        tags=[oca_tags.value],
    )

    # Act
    ib_tp_order = exec_client._transform_order_to_ib_order(tp_order)
    ib_sl_order = exec_client._transform_order_to_ib_order(sl_order)

    # Assert
    # Both orders should have the same OCA group
    assert ib_tp_order.ocaGroup == expected_oca_group
    assert ib_sl_order.ocaGroup == expected_oca_group

    # Both orders should have OCA type 1 (cancel all with block)
    assert ib_tp_order.ocaType == 1
    assert ib_sl_order.ocaType == 1

    # Parent order IDs should be set correctly
    assert ib_tp_order.parentId == parent_order_id.value
    assert ib_sl_order.parentId == parent_order_id.value


@pytest.mark.asyncio
async def test_transform_order_to_ib_order_ouo_orders(exec_client):
    """
    Test that OUO orders with explicit OCA tags are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    order_list_id = OrderListId("OL-456")
    parent_order_id = ClientOrderId("PARENT-2")
    tp_order_id = ClientOrderId("TP-2")
    sl_order_id = ClientOrderId("SL-2")

    # Create explicit OCA tags for OUO orders
    expected_oca_group = f"OCA_{order_list_id.value}"
    oca_tags = IBOrderTags(ocaGroup=expected_oca_group, ocaType=1)

    # Create take-profit order with OUO contingency and explicit OCA tags
    tp_order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=tp_order_id,
        order_side=OrderSide.SELL,
        quantity=instrument.make_qty(100),
        price=instrument.make_price(1.1000),
        time_in_force=TimeInForce.GTC,
        expire_time_ns=0,
        init_id=UUID4(),
        ts_init=0,
        post_only=False,
        reduce_only=True,
        display_qty=None,
        contingency_type=ContingencyType.OUO,
        order_list_id=order_list_id,
        linked_order_ids=[sl_order_id],
        parent_order_id=parent_order_id,
        tags=[oca_tags.value],
    )

    # Create stop-loss order with OUO contingency and explicit OCA tags
    sl_order = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=sl_order_id,
        order_side=OrderSide.SELL,
        quantity=instrument.make_qty(100),
        trigger_price=instrument.make_price(1.0500),
        trigger_type=TriggerType.DEFAULT,
        time_in_force=TimeInForce.GTC,
        init_id=UUID4(),
        ts_init=0,
        reduce_only=True,
        contingency_type=ContingencyType.OUO,
        order_list_id=order_list_id,
        linked_order_ids=[tp_order_id],
        parent_order_id=parent_order_id,
        tags=[oca_tags.value],
    )

    # Act
    ib_tp_order = exec_client._transform_order_to_ib_order(tp_order)
    ib_sl_order = exec_client._transform_order_to_ib_order(sl_order)

    # Assert
    # Both orders should have the same OCA group
    assert ib_tp_order.ocaGroup == expected_oca_group
    assert ib_sl_order.ocaGroup == expected_oca_group

    # Both orders should have OCA type 1 (cancel all with block)
    assert ib_tp_order.ocaType == 1
    assert ib_sl_order.ocaType == 1


@pytest.mark.asyncio
async def test_transform_order_with_custom_oca_tags(exec_client):
    """
    Test that custom OCA settings from IBOrderTags are properly applied.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create custom OCA tags
    custom_oca_tags = IBOrderTags(
        ocaGroup="CUSTOM_OCA_GROUP_123",
        ocaType=2,  # REDUCE_WITH_BLOCK
    )

    # Create order with custom OCA tags
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CUSTOM-OCA-1"),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(100),
        price=instrument.make_price(1.0500),
        time_in_force=TimeInForce.GTC,
        expire_time_ns=0,
        init_id=UUID4(),
        ts_init=0,
        post_only=False,
        reduce_only=False,
        display_qty=None,
        contingency_type=ContingencyType.NO_CONTINGENCY,  # No automatic contingency
        order_list_id=None,
        linked_order_ids=None,
        parent_order_id=None,
        tags=[custom_oca_tags.value],  # Apply custom OCA tags
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.ocaGroup == "CUSTOM_OCA_GROUP_123"
    assert ib_order.ocaType == 2


@pytest.mark.asyncio
async def test_transform_order_with_partial_oca_tags(exec_client):
    """
    Test that partial OCA settings from IBOrderTags work with defaults.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create OCA tags with only group specified (type should default to 1)
    partial_oca_tags = IBOrderTags(
        ocaGroup="PARTIAL_OCA_GROUP",
        ocaType=0,  # Explicitly set to 0 to test default behavior
    )

    # Create order with partial OCA tags
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("PARTIAL-OCA-1"),
        order_side=OrderSide.SELL,
        quantity=instrument.make_qty(50),
        price=instrument.make_price(1.1500),
        time_in_force=TimeInForce.GTC,
        expire_time_ns=0,
        init_id=UUID4(),
        ts_init=0,
        post_only=False,
        reduce_only=False,
        display_qty=None,
        contingency_type=ContingencyType.NO_CONTINGENCY,
        order_list_id=None,
        linked_order_ids=None,
        parent_order_id=None,
        tags=[partial_oca_tags.value],
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.ocaGroup == "PARTIAL_OCA_GROUP"
    assert ib_order.ocaType == 1  # Should default to 1


@pytest.mark.asyncio
async def test_custom_oca_tags_override_contingency_type(exec_client):
    """
    Test that custom OCA tags override automatic contingency type detection.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    order_list_id = OrderListId("OL-OVERRIDE")
    parent_order_id = ClientOrderId("PARENT-OVERRIDE")
    linked_order_id = ClientOrderId("LINKED-OVERRIDE")

    # Create custom OCA tags that should override automatic detection
    override_oca_tags = IBOrderTags(
        ocaGroup="OVERRIDE_GROUP",
        ocaType=3,  # REDUCE_NON_BLOCK
    )

    # Create order with OCO contingency AND custom tags (tags should win)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("OVERRIDE-1"),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(200),
        price=instrument.make_price(1.0800),
        time_in_force=TimeInForce.GTC,
        expire_time_ns=0,
        init_id=UUID4(),
        ts_init=0,
        post_only=False,
        reduce_only=True,
        display_qty=None,
        contingency_type=ContingencyType.OCO,  # This should be overridden
        order_list_id=order_list_id,
        linked_order_ids=[linked_order_id],
        parent_order_id=parent_order_id,
        tags=[override_oca_tags.value],  # This should take precedence
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert - custom tags should override automatic contingency detection
    assert ib_order.ocaGroup == "OVERRIDE_GROUP"  # Not "OCA_OL-OVERRIDE"
    assert ib_order.ocaType == 3  # Not 1


@pytest.mark.asyncio
async def test_transform_order_with_conditions(exec_client):
    """
    Test that orders with conditions are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create conditions data
    conditions_data = [
        {
            "type": "price",
            "conId": 265598,
            "exchange": "SMART",
            "isMore": True,
            "price": 1.1000,
            "triggerMethod": 0,
            "conjunction": "and",
        },
        {
            "type": "time",
            "time": "20250315-09:30:00",
            "isMore": True,
            "conjunction": "or",
        },
        {
            "type": "volume",
            "conId": 265598,
            "exchange": "SMART",
            "isMore": True,
            "volume": 1000000,
            "conjunction": "and",
        },
    ]

    # Create order tags with conditions
    order_tags = IBOrderTags(
        conditions=conditions_data,
        conditionsCancelOrder=False,
    )

    # Create order with conditions
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CONDITIONS-1"),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(100),
        price=instrument.make_price(1.0500),
        time_in_force=TimeInForce.GTC,
        expire_time_ns=0,
        init_id=UUID4(),
        ts_init=0,
        post_only=False,
        reduce_only=False,
        display_qty=None,
        contingency_type=ContingencyType.NO_CONTINGENCY,
        order_list_id=None,
        linked_order_ids=None,
        parent_order_id=None,
        tags=[order_tags.value],
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.conditions is not None
    assert len(ib_order.conditions) == 3
    assert ib_order.conditionsCancelOrder is False

    # Check price condition
    price_condition = ib_order.conditions[0]
    assert price_condition.type() == 1  # OrderCondition.Price
    assert price_condition.conId == 265598
    assert price_condition.exchange == "SMART"
    assert price_condition.isMore is True
    assert price_condition.price == 1.1000
    assert price_condition.triggerMethod == 0
    assert price_condition.isConjunctionConnection is True  # AND

    # Check time condition
    time_condition = ib_order.conditions[1]
    assert time_condition.type() == 3  # OrderCondition.Time
    assert time_condition.time == "20250315-09:30:00"
    assert time_condition.isMore is True
    assert time_condition.isConjunctionConnection is False  # OR

    # Check volume condition
    volume_condition = ib_order.conditions[2]
    assert volume_condition.type() == 6  # OrderCondition.Volume
    assert volume_condition.conId == 265598
    assert volume_condition.exchange == "SMART"
    assert volume_condition.isMore is True
    assert volume_condition.volume == 1000000
    assert volume_condition.isConjunctionConnection is True  # AND


@pytest.mark.asyncio
async def test_transform_order_with_execution_condition(exec_client):
    """
    Test that orders with execution conditions are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create execution condition data
    conditions_data = [
        {
            "type": "execution",
            "symbol": "AAPL",
            "secType": "STK",
            "exchange": "SMART",
            "conjunction": "and",
        },
    ]

    # Create order tags with execution condition
    order_tags = IBOrderTags(
        conditions=conditions_data,
        conditionsCancelOrder=True,  # Cancel order when condition is met
    )

    # Create order with execution condition
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("EXECUTION-CONDITION-1"),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(100),
        price=instrument.make_price(1.0500),
        time_in_force=TimeInForce.GTC,
        expire_time_ns=0,
        init_id=UUID4(),
        ts_init=0,
        post_only=False,
        reduce_only=False,
        display_qty=None,
        contingency_type=ContingencyType.NO_CONTINGENCY,
        order_list_id=None,
        linked_order_ids=None,
        parent_order_id=None,
        tags=[order_tags.value],
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.conditions is not None
    assert len(ib_order.conditions) == 1
    assert ib_order.conditionsCancelOrder is True

    # Check execution condition
    execution_condition = ib_order.conditions[0]
    assert execution_condition.type() == 5  # OrderCondition.Execution
    assert execution_condition.symbol == "AAPL"
    assert execution_condition.secType == "STK"
    assert execution_condition.exchange == "SMART"
    assert execution_condition.isConjunctionConnection is True  # AND


@pytest.mark.asyncio
async def test_transform_order_with_price_condition(exec_client):
    """
    Test that orders with price conditions are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create price condition data
    conditions_data = [
        {
            "type": "price",
            "conId": 265598,
            "exchange": "SMART",
            "isMore": True,
            "price": 1.1000,
            "triggerMethod": 0,
            "conjunction": "and",
        },
    ]

    # Create order tags with price condition
    order_tags = IBOrderTags(
        conditions=conditions_data,
        conditionsCancelOrder=False,
    )

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=TestIdStubs.client_order_id(),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(100_000),
        price=instrument.make_price(1.00050),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        tags=[order_tags.value],
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.conditions is not None
    assert len(ib_order.conditions) == 1
    assert ib_order.conditionsCancelOrder is False

    # Check price condition
    price_condition = ib_order.conditions[0]
    assert price_condition.type() == 1  # OrderCondition.Price
    assert price_condition.conId == 265598
    assert price_condition.exchange == "SMART"
    assert price_condition.isMore is True
    assert price_condition.price == 1.1000
    assert price_condition.triggerMethod == 0
    assert price_condition.isConjunctionConnection is True  # AND


@pytest.mark.asyncio
async def test_transform_order_with_time_condition(exec_client):
    """
    Test that orders with time conditions are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create time condition data
    conditions_data = [
        {
            "type": "time",
            "time": "20250315-09:30:00",
            "isMore": True,
            "conjunction": "and",
        },
    ]

    # Create order tags with time condition
    order_tags = IBOrderTags(
        conditions=conditions_data,
        conditionsCancelOrder=False,
    )

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=TestIdStubs.client_order_id(),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(100_000),
        price=instrument.make_price(1.00050),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        tags=[order_tags.value],
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.conditions is not None
    assert len(ib_order.conditions) == 1
    assert ib_order.conditionsCancelOrder is False

    # Check time condition
    time_condition = ib_order.conditions[0]
    assert time_condition.type() == 3  # OrderCondition.Time
    assert time_condition.time == "20250315-09:30:00"
    assert time_condition.isMore is True
    assert time_condition.isConjunctionConnection is True  # AND


@pytest.mark.asyncio
async def test_transform_order_with_volume_condition(exec_client):
    """
    Test that orders with volume conditions are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create volume condition data
    conditions_data = [
        {
            "type": "volume",
            "conId": 265598,
            "exchange": "SMART",
            "isMore": True,
            "volume": 1000000,
            "conjunction": "and",
        },
    ]

    # Create order tags with volume condition
    order_tags = IBOrderTags(
        conditions=conditions_data,
        conditionsCancelOrder=False,
    )

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=TestIdStubs.client_order_id(),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(100_000),
        price=instrument.make_price(1.00050),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        tags=[order_tags.value],
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.conditions is not None
    assert len(ib_order.conditions) == 1
    assert ib_order.conditionsCancelOrder is False

    # Check volume condition
    volume_condition = ib_order.conditions[0]
    assert volume_condition.type() == 6  # OrderCondition.Volume
    assert volume_condition.conId == 265598
    assert volume_condition.exchange == "SMART"
    assert volume_condition.isMore is True
    assert volume_condition.volume == 1000000
    assert volume_condition.isConjunctionConnection is True  # AND


@pytest.mark.asyncio
async def test_transform_order_with_margin_condition(exec_client):
    """
    Test that orders with margin conditions are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create margin condition data
    conditions_data = [
        {
            "type": "margin",
            "percent": 75,
            "isMore": True,
            "conjunction": "and",
        },
    ]

    # Create order tags with margin condition
    order_tags = IBOrderTags(
        conditions=conditions_data,
        conditionsCancelOrder=False,
    )

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=TestIdStubs.client_order_id(),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(100_000),
        price=instrument.make_price(1.00050),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        tags=[order_tags.value],
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.conditions is not None
    assert len(ib_order.conditions) == 1
    assert ib_order.conditionsCancelOrder is False

    # Check margin condition
    margin_condition = ib_order.conditions[0]
    assert margin_condition.type() == 4  # OrderCondition.Margin
    assert margin_condition.percent == 75
    assert margin_condition.isMore is True
    assert margin_condition.isConjunctionConnection is True  # AND


@pytest.mark.asyncio
async def test_transform_order_with_percent_change_condition(exec_client):
    """
    Test that orders with percent change conditions are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create percent change condition data
    conditions_data = [
        {
            "type": "percent_change",
            "conId": 265598,
            "exchange": "SMART",
            "changePercent": 5.0,
            "isMore": True,
            "conjunction": "and",
        },
    ]

    # Create order tags with percent change condition
    order_tags = IBOrderTags(
        conditions=conditions_data,
        conditionsCancelOrder=False,
    )

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=TestIdStubs.client_order_id(),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(100_000),
        price=instrument.make_price(1.00050),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        tags=[order_tags.value],
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.conditions is not None
    assert len(ib_order.conditions) == 1
    assert ib_order.conditionsCancelOrder is False

    # Check percent change condition
    percent_change_condition = ib_order.conditions[0]
    assert percent_change_condition.type() == 7  # OrderCondition.PercentChange
    assert percent_change_condition.conId == 265598
    assert percent_change_condition.exchange == "SMART"
    assert percent_change_condition.changePercent == 5.0
    assert percent_change_condition.isMore is True
    assert percent_change_condition.isConjunctionConnection is True  # AND


@pytest.mark.asyncio
async def test_transform_order_with_all_condition_types(exec_client):
    """
    Test that orders with all 6 condition types are properly transformed.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create all 6 condition types
    conditions_data = [
        {
            "type": "price",
            "conId": 265598,
            "exchange": "SMART",
            "isMore": True,
            "price": 1.1000,
            "triggerMethod": 0,
            "conjunction": "and",
        },
        {
            "type": "time",
            "time": "20250315-09:30:00",
            "isMore": True,
            "conjunction": "or",
        },
        {
            "type": "volume",
            "conId": 265598,
            "exchange": "SMART",
            "isMore": True,
            "volume": 1000000,
            "conjunction": "and",
        },
        {
            "type": "execution",
            "symbol": "AAPL",
            "secType": "STK",
            "exchange": "SMART",
            "conjunction": "and",
        },
        {
            "type": "margin",
            "percent": 75,
            "isMore": True,
            "conjunction": "or",
        },
        {
            "type": "percent_change",
            "conId": 265598,
            "exchange": "SMART",
            "changePercent": 5.0,
            "isMore": True,
            "conjunction": "and",
        },
    ]

    # Create order tags with all conditions
    order_tags = IBOrderTags(
        conditions=conditions_data,
        conditionsCancelOrder=False,
    )

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=TestIdStubs.client_order_id(),
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(100_000),
        price=instrument.make_price(1.00050),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        tags=[order_tags.value],
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(order)

    # Assert
    assert ib_order.conditions is not None
    assert len(ib_order.conditions) == 6
    assert ib_order.conditionsCancelOrder is False

    # Verify all condition types are present
    condition_types = [condition.type() for condition in ib_order.conditions]
    assert 1 in condition_types  # Price
    assert 3 in condition_types  # Time
    assert 6 in condition_types  # Volume
    assert 5 in condition_types  # Execution
    assert 4 in condition_types  # Margin
    assert 7 in condition_types  # PercentChange
