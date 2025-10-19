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

from types import SimpleNamespace
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.execution import OKXExecutionClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.okx.conftest import _create_ws_mock


@pytest.fixture()
def exec_client_builder(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        private_ws = _create_ws_mock()
        business_ws = _create_ws_mock()
        ws_iter = iter([private_ws, business_ws])

        monkeypatch.setattr(
            "nautilus_trader.adapters.okx.execution.nautilus_pyo3.OKXWebSocketClient.with_credentials",
            lambda *args, **kwargs: next(ws_iter),
        )

        mock_http_client.reset_mock()
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = [MagicMock(name="py_instrument")]

        config_kwargs = config_kwargs or {}
        instrument_types = config_kwargs.pop(
            "instrument_types",
            (nautilus_pyo3.OKXInstrumentType.SPOT,),
        )

        # Set the mock provider's instrument_types to match config
        mock_instrument_provider.instrument_types = instrument_types

        config = OKXExecClientConfig(
            api_key="test_api_key",
            api_secret="test_api_secret",
            api_passphrase="test_passphrase",
            instrument_types=instrument_types,
            **config_kwargs,
        )

        client = OKXExecutionClient(
            loop=event_loop,
            client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        return client, private_ws, business_ws, mock_http_client, mock_instrument_provider

    return builder


@pytest.mark.asyncio
async def test_connect_success(exec_client_builder, monkeypatch):
    # Arrange
    client, private_ws, business_ws, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    # Act
    await client._connect()

    try:
        # Assert
        instrument_provider.initialize.assert_awaited_once()
        http_client.add_instrument.assert_called_once_with(
            instrument_provider.instruments_pyo3.return_value[0],
        )
        http_client.request_account_state.assert_awaited_once()
        private_ws.connect.assert_awaited_once()
        private_ws.wait_until_active.assert_awaited_once_with(timeout_secs=30.0)
        business_ws.connect.assert_awaited_once()
        business_ws.wait_until_active.assert_awaited_once_with(timeout_secs=30.0)
        private_ws.subscribe_orders.assert_awaited_once_with(nautilus_pyo3.OKXInstrumentType.SPOT)
        business_ws.subscribe_orders_algo.assert_awaited_once_with(
            nautilus_pyo3.OKXInstrumentType.SPOT,
        )
        private_ws.subscribe_fills.assert_not_called()
        private_ws.subscribe_account.assert_awaited_once()
    finally:
        await client._disconnect()

    # Assert
    http_client.cancel_all_requests.assert_called_once()
    private_ws.close.assert_awaited_once()
    business_ws.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_generate_order_status_reports_converts_results(exec_client_builder, monkeypatch):
    # Arrange
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.OrderStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    pyo3_report = MagicMock()
    http_client.request_order_status_reports.return_value = [pyo3_report]

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
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
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_order_status_reports.side_effect = Exception("boom")

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
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
async def test_generate_fill_reports_converts_results(exec_client_builder, monkeypatch):
    # Arrange
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.FillReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_fill_reports.return_value = [MagicMock()]

    command = GenerateFillReports(
        instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
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
async def test_generate_position_status_reports_converts_results(exec_client_builder, monkeypatch):
    # Arrange
    # Use SWAP (derivatives) so positions are actually queried
    client, _, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"instrument_types": (nautilus_pyo3.OKXInstrumentType.SWAP,)},
    )

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.PositionStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_position_status_reports.return_value = [MagicMock()]

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
    http_client.request_position_status_reports.assert_awaited_once()
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_handle_fill_report_updates_venue_id_before_fill(exec_client_builder, monkeypatch):
    # Arrange
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)

    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
    client._cache.add_instrument(instrument)

    order_list = TestExecStubs.limit_with_stop_market(instrument=instrument)
    stop_order = next(order for order in order_list.orders if isinstance(order, StopMarketOrder))

    submitted = TestEventStubs.order_submitted(order=stop_order)
    stop_order.apply(submitted)
    accepted = TestEventStubs.order_accepted(
        order=stop_order,
        venue_order_id=VenueOrderId("algo-venue-id"),
    )
    stop_order.apply(accepted)

    client._cache.add_order(stop_order, None, None)

    canonical_id = stop_order.client_order_id
    client._algo_order_ids[canonical_id] = "algo-venue-id"
    client._algo_order_instruments[canonical_id] = stop_order.instrument_id

    emitted_events: list = []

    def _capture(event):
        emitted_events.append(event)

    monkeypatch.setattr(client, "_send_order_event", _capture)

    new_venue_id = VenueOrderId("child-venue-id")
    fill_report = SimpleNamespace(
        client_order_id=stop_order.client_order_id,
        venue_order_id=new_venue_id,
        venue_position_id=None,
        trade_id=TestIdStubs.trade_id(),
        last_qty=stop_order.quantity,
        last_px=instrument.make_price(4018.5),
        commission=Money(0, instrument.quote_currency),
        liquidity_side=LiquiditySide.TAKER,
        ts_event=123456789,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.FillReport.from_pyo3",
        lambda _obj: fill_report,
    )

    # Act
    client._handle_fill_report_pyo3(MagicMock())

    # Assert
    assert any(
        isinstance(event, OrderUpdated) and event.venue_order_id == new_venue_id
        for event in emitted_events
    )
    assert any(isinstance(event, OrderFilled) for event in emitted_events)
    assert client._cache.venue_order_id(stop_order.client_order_id) == new_venue_id
    assert canonical_id not in client._algo_order_ids

    http_client.request_fill_reports.assert_not_called()


@pytest.mark.asyncio
async def test_generate_position_status_reports_handles_failure(exec_client_builder, monkeypatch):
    # Arrange
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_position_status_reports.side_effect = Exception("boom")

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
    assert reports == []
