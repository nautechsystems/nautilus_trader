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
    Test that OCO orders are properly transformed with OCA group settings.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    order_list_id = OrderListId("OL-123")
    parent_order_id = ClientOrderId("PARENT-1")
    tp_order_id = ClientOrderId("TP-1")
    sl_order_id = ClientOrderId("SL-1")

    # Create take-profit order with OCO contingency
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
        tags=None,
    )

    # Create stop-loss order with OCO contingency
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
        tags=None,
    )

    # Act
    ib_tp_order = exec_client._transform_order_to_ib_order(tp_order)
    ib_sl_order = exec_client._transform_order_to_ib_order(sl_order)

    # Assert
    expected_oca_group = f"OCA_{order_list_id.value}"

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
    Test that OUO orders are properly transformed with OCA group settings.
    """
    # Arrange
    instrument = _EURUSD
    await exec_client._instrument_provider.load_async(instrument.id)

    order_list_id = OrderListId("OL-456")
    parent_order_id = ClientOrderId("PARENT-2")
    tp_order_id = ClientOrderId("TP-2")
    sl_order_id = ClientOrderId("SL-2")

    # Create take-profit order with OUO contingency (default for bracket orders)
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
        tags=None,
    )

    # Create stop-loss order with OUO contingency
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
        tags=None,
    )

    # Act
    ib_tp_order = exec_client._transform_order_to_ib_order(tp_order)
    ib_sl_order = exec_client._transform_order_to_ib_order(sl_order)

    # Assert
    expected_oca_group = f"OCA_{order_list_id.value}"

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
