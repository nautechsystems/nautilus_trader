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

from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.architect_ax.config import AxExecClientConfig
from nautilus_trader.adapters.architect_ax.constants import AX_VENUE
from nautilus_trader.adapters.architect_ax.execution import AxExecutionClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.architect_ax.conftest import _create_orders_ws_mock


@pytest.fixture
def exec_client_builder(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        ws_orders = _create_orders_ws_mock()
        ws_iter = iter([ws_orders])

        monkeypatch.setattr(
            "nautilus_trader.adapters.architect_ax.execution.nautilus_pyo3.AxOrdersWebSocketClient",
            lambda *args, **kwargs: next(ws_iter),
        )

        monkeypatch.setattr(
            "nautilus_trader.adapters.architect_ax.execution.AxExecutionClient._await_account_registered",
            AsyncMock(),
        )

        mock_http_client.reset_mock()
        mock_http_client.authenticate_auto.return_value = "test_bearer_token"
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = []

        config = AxExecClientConfig(
            api_key="test_api_key",
            api_secret="test_api_secret",
            environment=nautilus_pyo3.AxEnvironment.SANDBOX,
            **(config_kwargs or {}),
        )

        client = AxExecutionClient(
            loop=event_loop,
            client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        return client, ws_orders, mock_http_client, mock_instrument_provider

    return builder


@pytest.mark.asyncio
async def test_connect_success(exec_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    # Act
    await client._connect()

    try:
        # Assert
        instrument_provider.initialize.assert_awaited_once()
        http_client.authenticate_auto.assert_awaited_once()
        http_client.request_account_state.assert_awaited_once()
        ws_client.connect.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_connect_without_credentials(exec_client_builder, monkeypatch):
    """
    Missing credentials should log warning, not raise.
    """
    # Arrange
    client, ws_client, http_client, _ = exec_client_builder(monkeypatch)
    http_client.authenticate_auto.side_effect = ValueError("Missing credentials")

    # Act
    await client._connect()

    try:
        # Assert - should not have created WS client
        assert client._has_credentials is False
        ws_client.connect.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_disconnect_success(exec_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    # Act
    await client._disconnect()

    # Assert
    http_client.cancel_all_requests.assert_called_once()
    ws_client.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_account_id_set_on_initialization(exec_client_builder, monkeypatch):
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)

    # Assert
    assert client.account_id.value == "AX-001"


@pytest.mark.asyncio
async def test_generate_order_status_reports(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.execution.OrderStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    pyo3_report = MagicMock()
    http_client.request_order_status_reports.return_value = [pyo3_report]

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        start=None,
        end=None,
        open_only=True,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_order_status_reports(command)

    # Assert
    http_client.request_order_status_reports.assert_awaited_once()
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_generate_order_status_reports_handles_failure(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_order_status_reports.side_effect = Exception("boom")

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        start=None,
        end=None,
        open_only=False,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_order_status_reports(command)

    # Assert
    assert reports == []


@pytest.mark.asyncio
async def test_generate_fill_reports(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.execution.FillReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_fill_reports.return_value = [MagicMock()]

    command = GenerateFillReports(
        instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        venue_order_id=None,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_fill_reports(command)

    # Assert
    http_client.request_fill_reports.assert_awaited_once()
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_generate_position_status_reports(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.execution.PositionStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_position_reports.return_value = [MagicMock()]

    command = GeneratePositionStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert
    http_client.request_position_reports.assert_awaited_once()
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_submit_limit_order(exec_client_builder, monkeypatch, instrument):
    # Arrange
    client, ws_client, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("100"),
        price=Price.from_str("1.26500"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        # Act
        await client._submit_order(command)

        # Assert
        ws_client.submit_order.assert_awaited_once()
        kwargs = ws_client.submit_order.call_args.kwargs
        assert isinstance(kwargs["order_side"], nautilus_pyo3.OrderSide)
        assert isinstance(kwargs["order_type"], nautilus_pyo3.OrderType)
        assert isinstance(kwargs["time_in_force"], nautilus_pyo3.TimeInForce)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_denied_no_credentials(exec_client_builder, monkeypatch, instrument):
    """
    Order should be denied when no credentials are configured.
    """
    # Arrange
    client, ws_client, http_client, _ = exec_client_builder(monkeypatch)
    http_client.authenticate_auto.side_effect = ValueError("Missing credentials")
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("100"),
        price=Price.from_str("1.26500"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        # Act - should not raise
        await client._submit_order(command)

        # Assert - order should not reach WS
        ws_client.submit_order.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_denied_unsupported_type(exec_client_builder, monkeypatch, instrument):
    """
    Unsupported order types should be denied.
    """
    # Arrange
    client, ws_client, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("100"),
        trigger_price=Price.from_str("1.27000"),
        trigger_type=TriggerType.LAST_PRICE,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        # Act
        await client._submit_order(command)

        # Assert - order should not reach WS
        ws_client.submit_order.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_rejected(exec_client_builder, monkeypatch, instrument, cache):
    """AX does not support order modification - should generate rejection."""
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("100"),
        price=Price.from_str("1.26500"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("AX-12345"),
        quantity=Quantity.from_str("200"),
        price=Price.from_str("1.27000"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act - should not raise, generates rejection event
        await client._modify_order(command)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_order(exec_client_builder, monkeypatch, instrument, cache):
    # Arrange
    client, ws_client, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("100"),
        price=Price.from_str("1.26500"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act
        await client._cancel_order(command)

        # Assert
        ws_client.cancel_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_order_not_in_cache(exec_client_builder, monkeypatch, instrument):
    """
    Canceling an order not in cache should log error, not call WS.
    """
    # Arrange
    client, ws_client, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    command = CancelOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-UNKNOWN"),
        venue_order_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act
        await client._cancel_order(command)

        # Assert - should not reach WS
        ws_client.cancel_order.assert_not_awaited()
    finally:
        await client._disconnect()
