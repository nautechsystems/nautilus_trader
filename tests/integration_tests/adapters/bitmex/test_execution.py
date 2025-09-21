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

from unittest.mock import MagicMock

import pytest

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


pytestmark = pytest.mark.asyncio


# ============================================================================
# CLIENT CONNECTION AND LIFECYCLE TESTS
# ============================================================================


async def test_connect_success(exec_client):
    """
    Test successful client connection.
    """
    # Act
    await exec_client._connect()

    # Assert
    exec_client._mock_http_client.http_get_margin.assert_called_once_with("XBt")
    exec_client._mock_http_client.request_account_state.assert_called_once()
    exec_client._mock_ws_client.connect.assert_called_once()
    exec_client._mock_ws_client.wait_until_active.assert_called_once_with(timeout_secs=10.0)
    exec_client._mock_ws_client.subscribe_orders.assert_called_once()
    exec_client._mock_ws_client.subscribe_executions.assert_called_once()
    exec_client._mock_ws_client.subscribe_positions.assert_called_once()
    exec_client._mock_ws_client.subscribe_margin.assert_called_once()
    exec_client._mock_ws_client.subscribe_wallet.assert_called_once()


async def test_disconnect_success(exec_client):
    """
    Test successful client disconnection.
    """
    # Arrange
    await exec_client._connect()

    # Act
    await exec_client._disconnect()

    # Assert
    exec_client._mock_ws_client.unsubscribe_orders.assert_called_once()
    exec_client._mock_ws_client.unsubscribe_executions.assert_called_once()
    exec_client._mock_ws_client.unsubscribe_positions.assert_called_once()
    exec_client._mock_ws_client.unsubscribe_margin.assert_called_once()
    exec_client._mock_ws_client.unsubscribe_wallet.assert_called_once()
    exec_client._mock_ws_client.close.assert_called_once()


async def test_account_id_updated_on_connect(exec_client):
    """
    Test that account ID is updated with actual account number on connect.
    """
    # Act
    await exec_client._connect()

    # Assert
    assert exec_client.account_id.value == "BITMEX-1234567"
    exec_client._mock_ws_client.set_account_id.assert_called()


# ============================================================================
# ORDER SUBMISSION TESTS
# ============================================================================


async def test_submit_market_order(exec_client, instrument, strategy):
    """
    Test submitting a market order.
    """
    # Arrange
    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_called_once()
    call_args = exec_client._mock_http_client.submit_order.call_args[1]
    assert call_args["order_side"] == nautilus_pyo3.OrderSide.BUY
    assert call_args["order_type"] == nautilus_pyo3.OrderType.MARKET
    assert call_args["quantity"].as_double() == 100.0


async def test_submit_limit_order_post_only(exec_client, instrument, strategy):
    """
    Test submitting a limit order with post_only flag.
    """
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-002"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(50),
        price=Price.from_str("50000.0"),
        post_only=True,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_called_once()
    call_args = exec_client._mock_http_client.submit_order.call_args[1]
    assert call_args["order_side"] == nautilus_pyo3.OrderSide.SELL
    assert call_args["order_type"] == nautilus_pyo3.OrderType.LIMIT
    assert call_args["quantity"].as_double() == 50.0
    assert call_args["price"].as_double() == 50000.0
    assert call_args["post_only"] is True


async def test_submit_stop_order(exec_client, instrument, strategy):
    """
    Test submitting a stop market order.
    """
    # Arrange
    order = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-003"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(200),
        trigger_price=Price.from_str("51000.0"),
        trigger_type=TriggerType.DEFAULT,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_called_once()
    call_args = exec_client._mock_http_client.submit_order.call_args[1]
    assert call_args["order_type"] == nautilus_pyo3.OrderType.STOP_MARKET
    assert call_args["trigger_price"].as_double() == 51000.0


async def test_submit_order_rejection(exec_client, instrument, strategy):
    """
    Test order submission rejection handling.
    """
    # Arrange
    exec_client._mock_http_client.submit_order.side_effect = Exception("Insufficient margin")

    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-004"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(1000000),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._submit_order(command)

    # Assert - order rejection is handled internally, exception is caught
    exec_client._mock_http_client.submit_order.assert_called_once()


# ============================================================================
# ORDER MODIFICATION TESTS
# ============================================================================


async def test_modify_order_price(exec_client, instrument, strategy, cache):
    """
    Test modifying an order's price.
    """
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-005"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("49000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order)

    command = ModifyOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-12345"),
        quantity=None,
        price=Price.from_str("49500.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._modify_order(command)

    # Assert
    exec_client._mock_http_client.modify_order.assert_called_once()
    call_args = exec_client._mock_http_client.modify_order.call_args[1]
    assert call_args["price"].as_double() == 49500.0


async def test_modify_order_quantity(exec_client, instrument, strategy, cache):
    """
    Test modifying an order's quantity.
    """
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-006"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100),
        price=Price.from_str("51000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order)

    command = ModifyOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
        quantity=Quantity.from_int(150),
        price=None,
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._modify_order(command)

    # Assert
    exec_client._mock_http_client.modify_order.assert_called_once()
    call_args = exec_client._mock_http_client.modify_order.call_args[1]
    assert call_args["quantity"].as_double() == 150.0


async def test_modify_order_rejection(exec_client, instrument, strategy, cache):
    """
    Test order modification rejection handling.
    """
    # Arrange
    exec_client._mock_http_client.modify_order.side_effect = Exception("Order not found")

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-007"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("49000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order)

    command = ModifyOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
        quantity=None,
        price=Price.from_str("49500.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._modify_order(command)

    # Assert - rejection is handled internally
    exec_client._mock_http_client.modify_order.assert_called_once()


# ============================================================================
# ORDER CANCELLATION TESTS
# ============================================================================


async def test_cancel_order_by_client_id(exec_client, instrument, strategy, cache):
    """
    Test canceling an order by client order ID.
    """
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-008"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("49000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order)

    command = CancelOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._cancel_order(command)

    # Assert
    exec_client._mock_http_client.cancel_order.assert_called_once()
    call_args = exec_client._mock_http_client.cancel_order.call_args[1]
    assert call_args["client_order_id"].value == "O-008"


async def test_cancel_order_by_venue_id(exec_client, instrument, strategy, cache):
    """
    Test canceling an order by venue order ID.
    """
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-009"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(50),
        price=Price.from_str("51000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order)

    command = CancelOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-67890"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._cancel_order(command)

    # Assert
    exec_client._mock_http_client.cancel_order.assert_called_once()
    call_args = exec_client._mock_http_client.cancel_order.call_args[1]
    assert call_args["venue_order_id"].value == "V-67890"


async def test_cancel_all_orders(exec_client, instrument, strategy):
    """
    Test canceling all orders.
    """
    # Arrange
    command = CancelAllOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._cancel_all_orders(command)

    # Assert
    exec_client._mock_http_client.cancel_all_orders.assert_called_once()
    call_args = exec_client._mock_http_client.cancel_all_orders.call_args[1]
    assert call_args["instrument_id"].value == "XBTUSD.BITMEX"


async def test_cancel_order_rejection(exec_client, instrument, strategy, cache):
    """
    Test order cancellation rejection handling.
    """
    # Arrange
    exec_client._mock_http_client.cancel_order.side_effect = Exception("Order already filled")

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-011"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("49000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order)

    command = CancelOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._cancel_order(command)

    # Assert - rejection is handled internally
    exec_client._mock_http_client.cancel_order.assert_called_once()


# ============================================================================
# WEBSOCKET MESSAGE HANDLING TESTS
# ============================================================================


async def test_handle_order_update_message(exec_client):
    """
    Test handling order update WebSocket message.
    """
    # Arrange
    await exec_client._connect()

    # Create mock order update event
    mock_event = MagicMock()
    mock_event.__class__.__name__ = "OrderAccepted"
    mock_event.to_dict = MagicMock(
        return_value={
            "trader_id": "TRADER-001",
            "strategy_id": "S-001",
            "instrument_id": "XBTUSD.BITMEX",
            "client_order_id": "O-014",
            "venue_order_id": "V-77777",
            "account_id": "BITMEX-1234567",
            "event_id": str(TestIdStubs.uuid()),
            "ts_event": 1000000000,
            "ts_init": 0,
        },
    )

    # Act
    # Simulate receiving message through WebSocket
    handler = exec_client._mock_ws_client.connect.call_args[0][1]
    handler(mock_event)

    # Assert - handler was called without error
    assert handler is not None


async def test_handle_execution_message(exec_client):
    """
    Test handling execution/fill WebSocket message.
    """
    # Arrange
    await exec_client._connect()

    # Create mock fill event
    mock_event = MagicMock()
    mock_event.__class__.__name__ = "OrderFilled"
    mock_event.to_dict = MagicMock(
        return_value={
            "trader_id": "TRADER-001",
            "strategy_id": "S-001",
            "instrument_id": "XBTUSD.BITMEX",
            "client_order_id": "O-015",
            "venue_order_id": "V-66666",
            "account_id": "BITMEX-1234567",
            "trade_id": "T-22222",
            "order_side": "BUY",
            "last_qty": "100",
            "last_px": "49500.0",
            "commission": "0.00075",
            "commission_currency": "XBt",
            "liquidity_side": "TAKER",
            "event_id": str(TestIdStubs.uuid()),
            "ts_event": 1000000000,
            "ts_init": 0,
        },
    )

    # Act
    handler = exec_client._mock_ws_client.connect.call_args[0][1]
    handler(mock_event)

    # Assert - handler was called without error
    assert handler is not None


async def test_handle_position_update_message(exec_client):
    """
    Test handling position update WebSocket message.
    """
    # Arrange
    await exec_client._connect()

    # Create mock position event
    mock_event = MagicMock()
    mock_event.__class__.__name__ = "PositionOpened"
    mock_event.to_dict = MagicMock(
        return_value={
            "trader_id": "TRADER-001",
            "strategy_id": "S-001",
            "instrument_id": "XBTUSD.BITMEX",
            "account_id": "BITMEX-1234567",
            "opening_order_id": "O-016",
            "position_id": "P-001",
            "entry": "SELL",
            "side": "SHORT",
            "signed_qty": "-500.0",
            "quantity": "500",
            "avg_px_open": "50000.0",
            "event_id": str(TestIdStubs.uuid()),
            "ts_event": 1000000000,
            "ts_init": 0,
        },
    )

    # Act
    handler = exec_client._mock_ws_client.connect.call_args[0][1]
    handler(mock_event)

    # Assert - handler was called without error
    assert handler is not None


async def test_handle_account_state_update(exec_client):
    """
    Test handling account state update via WebSocket.
    """
    # Arrange
    await exec_client._connect()

    # Create mock account state event
    mock_event = MagicMock()
    mock_event.__class__.__name__ = "AccountState"
    mock_event.to_dict = MagicMock(
        return_value={
            "account_id": "BITMEX-1234567",
            "account_type": "MARGIN",
            "base_currency": "XBt",
            "reported": True,
            "balances": [],
            "margins": [],
            "info": {},
            "event_id": str(TestIdStubs.uuid()),
            "ts_event": 1000000000,
            "ts_init": 0,
        },
    )

    # Act
    handler = exec_client._mock_ws_client.connect.call_args[0][1]
    handler(mock_event)

    # Assert - handler was called without error
    assert handler is not None
