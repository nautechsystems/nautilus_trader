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

from unittest.mock import MagicMock

import pytest

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import ContingencyType
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


@pytest.mark.asyncio
async def test_connect_success(exec_client):
    # Arrange, Act
    await exec_client._connect()

    # Assert
    exec_client._mock_http_client.get_account_number.assert_called_once()
    exec_client._mock_http_client.request_account_state.assert_called_once()
    exec_client._mock_ws_client.connect.assert_called_once()
    exec_client._mock_ws_client.wait_until_active.assert_called_once_with(timeout_secs=10.0)
    exec_client._mock_ws_client.subscribe_orders.assert_called_once()
    exec_client._mock_ws_client.subscribe_executions.assert_called_once()
    exec_client._mock_ws_client.subscribe_positions.assert_called_once()
    exec_client._mock_ws_client.subscribe_margin.assert_called_once()
    exec_client._mock_ws_client.subscribe_wallet.assert_called_once()


@pytest.mark.asyncio
async def test_disconnect_success(exec_client):
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


@pytest.mark.asyncio
async def test_account_id_updated_on_connect(exec_client):
    # Act
    await exec_client._connect()

    # Assert
    assert exec_client.account_id.value == "BITMEX-1234567"
    exec_client._mock_ws_client.set_account_id.assert_called()


@pytest.mark.asyncio
async def test_submit_market_order(exec_client, instrument, strategy):
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


@pytest.mark.asyncio
async def test_submit_limit_order_post_only(exec_client, instrument, strategy):
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


@pytest.mark.asyncio
async def test_submit_order_for_contingency(exec_client, instrument, strategy):
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-010"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(10),
        price=Price.from_str("48000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
        contingency_type=ContingencyType.OCO,
        order_list_id=TestIdStubs.order_list_id(),
        linked_order_ids=[ClientOrderId("O-011")],
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
    assert call_args["contingency_type"] == nautilus_pyo3.ContingencyType.OCO
    assert call_args["order_list_id"].value == order.order_list_id.value


@pytest.mark.asyncio
async def test_submit_stop_order(exec_client, instrument, strategy):
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


@pytest.mark.asyncio
async def test_submit_order_rejection(exec_client, instrument, strategy):
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


@pytest.mark.asyncio
async def test_submit_order_with_quote_quantity_denied(exec_client, instrument, strategy, mocker):
    # Arrange
    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-005"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(1000),
        quote_quantity=True,
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

    denied_spy = mocker.spy(exec_client, "generate_order_denied")

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_not_called()
    denied_spy.assert_called_once()
    denied_kwargs = denied_spy.call_args.kwargs
    assert denied_kwargs["client_order_id"] == order.client_order_id
    assert denied_kwargs["reason"] == "UNSUPPORTED_QUOTE_QUANTITY"


@pytest.mark.asyncio
async def test_modify_order_price(exec_client, instrument, strategy, cache):
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


@pytest.mark.asyncio
async def test_modify_order_quantity(exec_client, instrument, strategy, cache):
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


@pytest.mark.asyncio
async def test_modify_order_rejection(exec_client, instrument, strategy, cache):
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


@pytest.mark.asyncio
async def test_cancel_order_by_client_id(exec_client, instrument, strategy, cache):
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
    exec_client._mock_canceller.broadcast_cancel.assert_called_once()
    call_args = exec_client._mock_canceller.broadcast_cancel.call_args[1]
    assert call_args["client_order_id"].value == "O-008"


@pytest.mark.asyncio
async def test_cancel_order_by_venue_id(exec_client, instrument, strategy, cache):
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
    exec_client._mock_canceller.broadcast_cancel.assert_called_once()
    call_args = exec_client._mock_canceller.broadcast_cancel.call_args[1]
    assert call_args["venue_order_id"].value == "V-67890"


@pytest.mark.asyncio
async def test_cancel_all_orders(exec_client, instrument, strategy):
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
    exec_client._mock_canceller.broadcast_cancel_all.assert_called_once()
    call_args = exec_client._mock_canceller.broadcast_cancel_all.call_args[1]
    assert call_args["instrument_id"].value == "XBTUSD.BITMEX"


@pytest.mark.asyncio
async def test_cancel_order_rejection(exec_client, instrument, strategy, cache):
    # Arrange
    exec_client._mock_canceller.broadcast_cancel.side_effect = Exception("Order already filled")

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
    exec_client._mock_canceller.broadcast_cancel.assert_called_once()


@pytest.mark.asyncio
async def test_handle_order_update_message(exec_client):
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
    handler = exec_client._mock_ws_client.connect.call_args[0][2]
    handler(mock_event)

    # Assert - handler was called without error
    assert handler is not None


@pytest.mark.asyncio
async def test_handle_execution_message(exec_client):
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
    handler = exec_client._mock_ws_client.connect.call_args[0][2]
    handler(mock_event)

    # Assert - handler was called without error
    assert handler is not None


@pytest.mark.asyncio
async def test_handle_position_update_message(exec_client):
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
    handler = exec_client._mock_ws_client.connect.call_args[0][2]
    handler(mock_event)

    # Assert - handler was called without error
    assert handler is not None


@pytest.mark.asyncio
async def test_handle_account_state_update(exec_client):
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
    handler = exec_client._mock_ws_client.connect.call_args[0][2]
    handler(mock_event)

    # Assert - handler was called without error
    assert handler is not None


@pytest.mark.asyncio
async def test_generate_order_status_reports_converts_results(
    exec_client,
    instrument,
    monkeypatch,
):
    # Arrange
    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.bitmex.execution.OrderStatusReport.from_pyo3",
        lambda _obj: expected_report,
    )

    pyo3_report = MagicMock()
    exec_client._mock_http_client.request_order_status_reports.return_value = [pyo3_report]

    command = GenerateOrderStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        open_only=True,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await exec_client.generate_order_status_reports(command)

    # Assert
    exec_client._mock_http_client.request_order_status_reports.assert_awaited_once_with(
        instrument_id=instrument.id,
        open_only=True,
        limit=None,
    )
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_generate_order_status_reports_handles_failure(exec_client, instrument):
    # Arrange
    exec_client._mock_http_client.request_order_status_reports.side_effect = Exception("boom")

    reports = None
    try:
        command = GenerateOrderStatusReports(
            instrument_id=instrument.id,
            start=None,
            end=None,
            open_only=False,
            command_id=TestIdStubs.uuid(),
            ts_init=0,
        )

        # Act
        reports = await exec_client.generate_order_status_reports(command)
    finally:
        exec_client._mock_http_client.request_order_status_reports.side_effect = None

    # Assert
    assert reports == []


@pytest.mark.asyncio
async def test_generate_fill_reports_converts_results(
    exec_client,
    instrument,
    monkeypatch,
):
    # Arrange
    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.bitmex.execution.FillReport.from_pyo3",
        lambda _obj: expected_report,
    )

    pyo3_report = MagicMock()
    exec_client._mock_http_client.request_fill_reports.return_value = [pyo3_report]

    command = GenerateFillReports(
        instrument_id=instrument.id,
        venue_order_id=None,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await exec_client.generate_fill_reports(command)

    # Assert
    exec_client._mock_http_client.request_fill_reports.assert_awaited_once_with(
        instrument_id=instrument.id,
        limit=None,
    )
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_generate_position_status_reports_converts_results(
    exec_client,
    monkeypatch,
):
    # Arrange
    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.bitmex.execution.PositionStatusReport.from_pyo3",
        lambda _obj: expected_report,
    )

    pyo3_report = MagicMock()
    exec_client._mock_http_client.request_position_status_reports.return_value = [pyo3_report]

    command = GeneratePositionStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await exec_client.generate_position_status_reports(command)

    # Assert
    exec_client._mock_http_client.request_position_status_reports.assert_awaited_once_with()
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_generate_position_status_reports_handles_failure(exec_client):
    # Arrange
    exec_client._mock_http_client.request_position_status_reports.side_effect = Exception("boom")

    reports = None
    try:
        command = GeneratePositionStatusReports(
            instrument_id=None,
            start=None,
            end=None,
            command_id=TestIdStubs.uuid(),
            ts_init=0,
        )

        # Act
        reports = await exec_client.generate_position_status_reports(command)
    finally:
        exec_client._mock_http_client.request_position_status_reports.side_effect = None

    # Assert
    assert reports == []


@pytest.mark.asyncio
async def test_submit_limit_order_with_peg_params(exec_client, instrument, strategy):
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PEG-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        params={"peg_price_type": "PrimaryPeg", "peg_offset_value": "0"},
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_called_once()
    call_args = exec_client._mock_http_client.submit_order.call_args[1]
    assert call_args["peg_price_type"] == "PrimaryPeg"
    assert call_args["peg_offset_value"] == 0.0


@pytest.mark.asyncio
async def test_submit_limit_order_with_peg_negative_offset(exec_client, instrument, strategy):
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PEG-002"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(50),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        params={"peg_price_type": "MarketPeg", "peg_offset_value": "-1.5"},
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_called_once()
    call_args = exec_client._mock_http_client.submit_order.call_args[1]
    assert call_args["peg_price_type"] == "MarketPeg"
    assert call_args["peg_offset_value"] == -1.5


@pytest.mark.asyncio
async def test_submit_limit_order_with_peg_type_only(exec_client, instrument, strategy):
    # Arrange - peg_offset_value is optional
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PEG-003"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        params={"peg_price_type": "MidPricePeg"},
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_called_once()
    call_args = exec_client._mock_http_client.submit_order.call_args[1]
    assert call_args["peg_price_type"] == "MidPricePeg"
    assert call_args["peg_offset_value"] is None


@pytest.mark.asyncio
async def test_submit_order_with_invalid_peg_type_denied(
    exec_client,
    instrument,
    strategy,
    mocker,
):
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PEG-004"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        params={"peg_price_type": "InvalidPeg"},
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    denied_spy = mocker.spy(exec_client, "generate_order_denied")

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_not_called()
    denied_spy.assert_called_once()
    denied_kwargs = denied_spy.call_args.kwargs
    assert denied_kwargs["client_order_id"] == order.client_order_id
    assert "peg_price_type" in denied_kwargs["reason"]


@pytest.mark.asyncio
async def test_submit_market_order_with_peg_params_denied(
    exec_client,
    instrument,
    strategy,
    mocker,
):
    # Arrange - pegged orders only supported for LIMIT
    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PEG-005"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        params={"peg_price_type": "PrimaryPeg", "peg_offset_value": "0"},
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    denied_spy = mocker.spy(exec_client, "generate_order_denied")

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_not_called()
    denied_spy.assert_called_once()
    denied_kwargs = denied_spy.call_args.kwargs
    assert denied_kwargs["client_order_id"] == order.client_order_id
    assert "LIMIT" in denied_kwargs["reason"]


@pytest.mark.asyncio
async def test_submit_order_with_invalid_peg_offset_denied(
    exec_client,
    instrument,
    strategy,
    mocker,
):
    # Arrange
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PEG-006"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        params={"peg_price_type": "PrimaryPeg", "peg_offset_value": "not_a_number"},
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    denied_spy = mocker.spy(exec_client, "generate_order_denied")

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_not_called()
    denied_spy.assert_called_once()
    denied_kwargs = denied_spy.call_args.kwargs
    assert denied_kwargs["client_order_id"] == order.client_order_id
    assert "peg_offset_value" in denied_kwargs["reason"]


@pytest.mark.asyncio
async def test_submit_order_without_peg_params_unaffected(exec_client, instrument, strategy):
    # Arrange - no peg params should leave them as None
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PEG-007"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("50000.0"),
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
    assert call_args["peg_price_type"] is None
    assert call_args["peg_offset_value"] is None


@pytest.mark.asyncio
async def test_connect_starts_dms_when_configured(exec_client_with_dms):
    # Act
    await exec_client_with_dms._connect()

    # Assert
    assert exec_client_with_dms._dms_task is not None
    assert not exec_client_with_dms._dms_task.done()


@pytest.mark.asyncio
async def test_connect_does_not_start_dms_when_not_configured(exec_client):
    # Act
    await exec_client._connect()

    # Assert
    assert exec_client._dms_task is None


@pytest.mark.asyncio
async def test_disconnect_does_not_disarm_dms_when_not_configured(exec_client):
    # Arrange
    await exec_client._connect()

    # Act
    await exec_client._disconnect()

    # Assert - cancel_all_after should never be called
    exec_client._mock_http_client.cancel_all_after.assert_not_called()


@pytest.mark.asyncio
async def test_disconnect_disarms_dms(exec_client_with_dms):
    # Arrange
    await exec_client_with_dms._connect()
    assert exec_client_with_dms._dms_task is not None

    # Act
    await exec_client_with_dms._disconnect()

    # Assert - task cancelled and disarm call made with timeout=0
    assert exec_client_with_dms._dms_task is None
    disarm_calls = [
        call
        for call in exec_client_with_dms._mock_http_client.cancel_all_after.call_args_list
        if call.args == (0,) or call.kwargs.get("timeout_ms") == 0
    ]
    assert len(disarm_calls) == 1


@pytest.mark.asyncio
async def test_dms_calls_cancel_all_after_with_correct_timeout(exec_client_with_dms):
    # Act
    await exec_client_with_dms._connect()

    # Allow the DMS loop to make at least one heartbeat call
    import asyncio

    await asyncio.sleep(0.1)

    # Assert - at least one heartbeat call with timeout_ms=60000
    heartbeat_calls = [
        call
        for call in exec_client_with_dms._mock_http_client.cancel_all_after.call_args_list
        if call.args == (60000,) or call.kwargs.get("timeout_ms") == 60000
    ]
    assert len(heartbeat_calls) >= 1

    # Cleanup
    await exec_client_with_dms._disconnect()


@pytest.mark.asyncio
async def test_submit_order_with_peg_offset_only_denied(
    exec_client,
    instrument,
    strategy,
    mocker,
):
    # Arrange - peg_offset_value without peg_price_type should be rejected
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PEG-008"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=strategy.id,
        order=order,
        params={"peg_offset_value": "0"},
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    denied_spy = mocker.spy(exec_client, "generate_order_denied")

    # Act
    await exec_client._submit_order(command)

    # Assert
    exec_client._mock_http_client.submit_order.assert_not_called()
    denied_spy.assert_called_once()
    denied_kwargs = denied_spy.call_args.kwargs
    assert denied_kwargs["client_order_id"] == order.client_order_id
    assert "peg_price_type" in denied_kwargs["reason"]
