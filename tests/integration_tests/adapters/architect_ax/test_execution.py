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

import asyncio
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.architect_ax.config import AxExecClientConfig
from nautilus_trader.adapters.architect_ax.constants import AX_VENUE
from nautilus_trader.adapters.architect_ax.execution import AxExecutionClient
from nautilus_trader.adapters.architect_ax.factories import AxLiveExecClientFactory
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.network import TransportBackend
from nautilus_trader.test_kit.stubs.events import TestEventStubs
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
        ws_orders.constructor_calls = []

        def create_ws_client(*args, **kwargs):
            ws_orders.constructor_calls.append((args, kwargs))
            return next(ws_iter)

        monkeypatch.setattr(
            "nautilus_trader.adapters.architect_ax.execution.nautilus_pyo3.AxOrdersWebSocketClient",
            create_ws_client,
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


@pytest.fixture
def submitted_order(instrument, cache):
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-ACCEPTED-ONCE"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(2),
        price=Price.from_str("6.4000"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    order.apply(TestEventStubs.order_submitted(order))
    cache.add_order(order, None)
    return order


@pytest.fixture
def accepted_order_status_report(submitted_order):
    return MagicMock(
        client_order_id=submitted_order.client_order_id,
        instrument_id=submitted_order.instrument_id,
        venue_order_id=VenueOrderId("O-AX-ACCEPTED"),
        order_status=OrderStatus.ACCEPTED,
        ts_last=1,
    )


def test_exec_factory_routes_orders_requests_to_separate_base_url(monkeypatch):
    http_client = MagicMock()
    instrument_provider = MagicMock()
    execution_client = MagicMock()
    http_kwargs = {}

    def get_http_client(**kwargs):
        http_kwargs.update(kwargs)
        return http_client

    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.factories.get_cached_ax_http_client",
        get_http_client,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.factories.get_cached_ax_instrument_provider",
        lambda **_: instrument_provider,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.factories.AxExecutionClient",
        lambda **_: execution_client,
    )
    config = AxExecClientConfig(
        base_url_http="https://api.example.com",
        base_url_orders="https://orders.example.com",
    )

    result = AxLiveExecClientFactory.create(
        loop=MagicMock(),
        name="AX",
        config=config,
        msgbus=MagicMock(),
        cache=MagicMock(),
        clock=MagicMock(),
    )

    assert result is execution_client
    assert http_kwargs["base_url"] == "https://api.example.com"
    assert http_kwargs["orders_base_url"] == "https://orders.example.com"


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
        http_client.authenticate_auto.assert_awaited_once_with(3600)
        http_client.request_account_state.assert_awaited_once()
        ws_client.connect.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_connect_uses_execution_websocket_config(exec_client_builder, monkeypatch):
    client, ws_client, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "base_url_ws": "wss://example.com/orders/ws?account=test",
            "heartbeat_interval_secs": 9,
            "cancel_on_disconnect": True,
            "transport_backend": TransportBackend.TUNGSTENITE,
        },
    )

    await client._connect()

    try:
        kwargs = ws_client.constructor_calls[0][1]
        assert kwargs["url"] == (
            "wss://example.com/orders/ws?account=test&cancel_on_disconnect=true"
        )
        assert kwargs["heartbeat"] == 9
        assert kwargs["transport_backend"] == TransportBackend.TUNGSTENITE
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
async def test_auth_refresh_retries_and_updates_orders_websocket_token(
    exec_client_builder,
    monkeypatch,
):
    client, ws_client, http_client, _ = exec_client_builder(monkeypatch)
    refreshed = asyncio.Event()
    block_second_refresh = asyncio.Event()
    refresh_count = 0
    sleep_delays = []
    delays_at_auth_attempt = []
    real_sleep = asyncio.sleep

    async def record_sleep(delay):
        sleep_delays.append(delay)
        await real_sleep(0)

    async def authenticate_auto(expiration_seconds):
        nonlocal refresh_count
        refresh_count += 1
        delays_at_auth_attempt.append(list(sleep_delays))

        if refresh_count == 1:
            raise RuntimeError("temporary authentication failure")
        if refresh_count > 2:
            await block_second_refresh.wait()
        refreshed.set()
        return "refreshed_bearer_token"

    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.execution.asyncio.sleep",
        record_sleep,
    )
    http_client.authenticate_auto = AsyncMock(side_effect=authenticate_auto)
    client._ws_orders_client = ws_client
    refresh_task = asyncio.create_task(client._refresh_auth_token())
    client._auth_refresh_task = refresh_task

    await asyncio.wait_for(refreshed.wait(), timeout=1)
    await real_sleep(0)
    await client._stop_auth_refresh()

    http_client.authenticate_auto.assert_awaited_with(3600)
    assert http_client.authenticate_auto.await_count >= 2
    ws_client.update_auth_token.assert_called_once_with("refreshed_bearer_token")
    assert delays_at_auth_attempt[:2] == [[1800], [1800, 30]]
    assert client._auth_refresh_task is None
    assert refresh_task.cancelled()


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

    synthetic_client_order_id = ClientOrderId("CID-123")
    expected_report = MagicMock(
        client_order_id=synthetic_client_order_id,
        venue_order_id=VenueOrderId("O-EXTERNAL"),
    )
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
    assert expected_report.client_order_id == synthetic_client_order_id


@pytest.mark.asyncio
async def test_generate_order_status_reports_uses_cached_client_order_id(
    exec_client_builder,
    monkeypatch,
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    client_order_id = ClientOrderId("O-LOCAL")
    venue_order_id = VenueOrderId("O-AX")
    client._cache.add_venue_order_id(client_order_id, venue_order_id)

    report = MagicMock(
        client_order_id=ClientOrderId("CID-123"),
        venue_order_id=venue_order_id,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.execution.OrderStatusReport.from_pyo3",
        lambda _: report,
    )
    http_client.request_order_status_reports.return_value = [MagicMock()]
    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        start=None,
        end=None,
        open_only=True,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    reports = await client.generate_order_status_reports(command)

    assert reports == [report]
    assert report.client_order_id == client_order_id


@pytest.mark.asyncio
async def test_generate_order_status_reports_passes_open_client_ids_for_cid_resolution(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PERSISTED"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("1"),
        price=Price.from_str("6.4000"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    old_venue_order_id = VenueOrderId("O-OLD")
    new_venue_order_id = VenueOrderId("O-NEW")
    cache.add_order(order, None)
    order.apply(TestEventStubs.order_submitted(order))
    order.apply(
        TestEventStubs.order_accepted(order, venue_order_id=old_venue_order_id),
    )
    cache.update_order(order)

    report = MagicMock(
        client_order_id=order.client_order_id,
        venue_order_id=new_venue_order_id,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.execution.OrderStatusReport.from_pyo3",
        lambda _: report,
    )
    http_client.request_order_status_reports.return_value = [MagicMock()]
    command = GenerateOrderStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        open_only=True,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    reports = await client.generate_order_status_reports(command)

    requested_client_order_ids = http_client.request_order_status_reports.call_args.args[1]
    assert reports == [report]
    assert [value.value for value in requested_client_order_ids] == [order.client_order_id.value]
    assert cache.client_order_id(new_venue_order_id) is None
    assert report.client_order_id == order.client_order_id


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


def test_handle_order_status_report_accepts_submitted_order_once(
    exec_client_builder,
    monkeypatch,
    submitted_order,
    accepted_order_status_report,
    cache,
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.execution.OrderStatusReport.from_pyo3",
        lambda _: accepted_order_status_report,
    )
    client.generate_order_accepted = MagicMock()

    client._handle_order_status_report(MagicMock())
    submitted_order.apply(
        TestEventStubs.order_accepted(
            submitted_order,
            venue_order_id=accepted_order_status_report.venue_order_id,
        ),
    )
    cache.update_order(submitted_order)
    client._handle_order_status_report(MagicMock())

    assert submitted_order.status == OrderStatus.ACCEPTED
    client.generate_order_accepted.assert_called_once_with(
        strategy_id=submitted_order.strategy_id,
        instrument_id=submitted_order.instrument_id,
        client_order_id=submitted_order.client_order_id,
        venue_order_id=accepted_order_status_report.venue_order_id,
        ts_event=1,
    )


@pytest.mark.parametrize(
    "local_status",
    [OrderStatus.PARTIALLY_FILLED, OrderStatus.PENDING_CANCEL],
    ids=["partially_filled", "pending_cancel"],
)
def test_handle_order_status_report_does_not_regress_open_order_state(
    exec_client_builder,
    monkeypatch,
    instrument,
    submitted_order,
    accepted_order_status_report,
    cache,
    local_status,
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    submitted_order.apply(
        TestEventStubs.order_accepted(
            submitted_order,
            venue_order_id=accepted_order_status_report.venue_order_id,
        ),
    )

    if local_status == OrderStatus.PARTIALLY_FILLED:
        submitted_order.apply(
            TestEventStubs.order_filled(
                submitted_order,
                instrument,
                last_qty=Quantity.from_int(1),
            ),
        )
    else:
        submitted_order.apply(TestEventStubs.order_pending_cancel(submitted_order))
    cache.update_order(submitted_order)
    monkeypatch.setattr(
        "nautilus_trader.adapters.architect_ax.execution.OrderStatusReport.from_pyo3",
        lambda _: accepted_order_status_report,
    )
    client.generate_order_accepted = MagicMock()

    client._handle_order_status_report(MagicMock())

    assert submitted_order.status == local_status
    client.generate_order_accepted.assert_not_called()


def test_handle_order_updated_promotes_replacement_venue_id(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-REPLACED"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("1"),
        price=Price.from_str("6.4000"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    old_venue_order_id = VenueOrderId("O-OLD")
    new_venue_order_id = VenueOrderId("O-NEW")
    cache.add_order(order, None)
    order.apply(TestEventStubs.order_submitted(order))
    order.apply(
        TestEventStubs.order_accepted(order, venue_order_id=old_venue_order_id),
    )
    cache.update_order(order)

    expected_event = OrderUpdated(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=new_venue_order_id,
        account_id=order.account_id,
        quantity=order.quantity,
        price=Price.from_str("6.4010"),
        trigger_price=None,
        event_id=TestIdStubs.uuid(),
        ts_event=1,
        ts_init=1,
    )
    pyo3_event = MagicMock()
    pyo3_event.to_dict.return_value = OrderUpdated.to_dict(expected_event)
    captured = []
    client._send_order_event = captured.append

    client._handle_order_updated(pyo3_event)
    order.apply(captured[0])
    cache.update_order(order)

    pyo3_event.to_dict.assert_called_once_with()
    assert len(captured) == 1
    assert order.venue_order_id == new_venue_order_id
    assert cache.venue_order_id(order.client_order_id) == new_venue_order_id
    assert cache.client_order_id(new_venue_order_id) == order.client_order_id


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
        assert isinstance(kwargs["time_in_force"], nautilus_pyo3.TimeInForce)
        assert isinstance(kwargs["price"], nautilus_pyo3.Price)
        assert "order_type" not in kwargs
        assert "trigger_price" not in kwargs
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_market_order(exec_client_builder, monkeypatch, instrument):
    client, ws_client, http_client, _ = exec_client_builder(monkeypatch)
    http_client.preview_aggressive_limit_order.return_value = nautilus_pyo3.Price.from_str("6.5000")
    await client._connect()

    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-MARKET"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("1"),
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
        await client._submit_order(command)

        http_client.preview_aggressive_limit_order.assert_awaited_once()
        ws_client.submit_order.assert_awaited_once()
        kwargs = ws_client.submit_order.call_args.kwargs
        assert kwargs["price"] == nautilus_pyo3.Price.from_str("6.5000")
        assert kwargs["time_in_force"] == nautilus_pyo3.TimeInForce.IOC
        assert kwargs["post_only"] is False
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
@pytest.mark.parametrize("order_kind", ["stop_market", "stop_limit"])
async def test_submit_order_denied_unsupported_type(
    exec_client_builder,
    monkeypatch,
    instrument,
    order_kind,
):
    """
    Unsupported order types should be denied.
    """
    # Arrange
    client, ws_client, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order_kwargs = {
        "trader_id": TestIdStubs.trader_id(),
        "strategy_id": TestIdStubs.strategy_id(),
        "instrument_id": instrument.id,
        "client_order_id": ClientOrderId("O-123456"),
        "order_side": OrderSide.BUY,
        "quantity": Quantity.from_str("100"),
        "trigger_price": Price.from_str("1.27000"),
        "trigger_type": TriggerType.LAST_PRICE,
        "init_id": TestIdStubs.uuid(),
        "ts_init": 0,
    }

    if order_kind == "stop_limit":
        order = StopLimitOrder(price=Price.from_str("1.27100"), **order_kwargs)
    else:
        order = StopMarketOrder(**order_kwargs)

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
@pytest.mark.parametrize("order_kind", ["market", "limit"])
async def test_submit_order_denied_reduce_only(
    exec_client_builder,
    monkeypatch,
    instrument,
    order_kind,
):
    client, ws_client, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order_kwargs = {
        "trader_id": TestIdStubs.trader_id(),
        "strategy_id": TestIdStubs.strategy_id(),
        "instrument_id": instrument.id,
        "client_order_id": ClientOrderId("O-REDUCE-ONLY"),
        "order_side": OrderSide.BUY,
        "quantity": Quantity.from_str("100"),
        "reduce_only": True,
        "init_id": TestIdStubs.uuid(),
        "ts_init": 0,
    }

    if order_kind == "market":
        order = MarketOrder(**order_kwargs)
    else:
        order = LimitOrder(price=Price.from_str("1.26500"), **order_kwargs)

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
        await client._submit_order(command)

        http_client.preview_aggressive_limit_order.assert_not_awaited()
        ws_client.submit_order.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize("order_kind", ["market", "limit"])
async def test_submit_order_denied_quote_quantity(
    exec_client_builder,
    monkeypatch,
    instrument,
    order_kind,
):
    client, ws_client, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order_kwargs = {
        "trader_id": TestIdStubs.trader_id(),
        "strategy_id": TestIdStubs.strategy_id(),
        "instrument_id": instrument.id,
        "client_order_id": ClientOrderId("O-QUOTE-QUANTITY"),
        "order_side": OrderSide.BUY,
        "quantity": Quantity.from_str("100"),
        "quote_quantity": True,
        "init_id": TestIdStubs.uuid(),
        "ts_init": 0,
    }

    if order_kind == "market":
        order = MarketOrder(**order_kwargs)
    else:
        order = LimitOrder(price=Price.from_str("1.26500"), **order_kwargs)

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
        await client._submit_order(command)

        http_client.preview_aggressive_limit_order.assert_not_awaited()
        ws_client.submit_order.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_denied_display_quantity(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    client, ws_client, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-DISPLAY-QUANTITY"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("100"),
        price=Price.from_str("1.26500"),
        display_qty=Quantity.from_str("50"),
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
        await client._submit_order(command)

        http_client.preview_aggressive_limit_order.assert_not_awaited()
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
