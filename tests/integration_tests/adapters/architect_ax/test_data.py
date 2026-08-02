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
from types import SimpleNamespace
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.architect_ax.config import AxDataClientConfig
from nautilus_trader.adapters.architect_ax.constants import AX_VENUE
from nautilus_trader.adapters.architect_ax.data import AxDataClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.network import TransportBackend
from tests.integration_tests.adapters.architect_ax.conftest import _create_ws_mock


class Pyo3BookPrice:
    def __init__(self, value: float) -> None:
        self._value = value

    def as_double(self) -> float:
        return self._value


class Pyo3BookLevel:
    def __init__(self, price: float, size: int) -> None:
        self.price = Pyo3BookPrice(price)
        self._size = size

    def size(self) -> int:
        return self._size


class Pyo3BookSnapshot:
    def __init__(self) -> None:
        self.ts_last = 1_234_567_890
        self.sequence = 42
        self._bids = [Pyo3BookLevel(50_000.0, 3)]
        self._asks = [Pyo3BookLevel(50_001.0, 5)]

    def bids(self) -> list[Pyo3BookLevel]:
        return self._bids

    def asks(self) -> list[Pyo3BookLevel]:
        return self._asks


@pytest.fixture
def data_client_builder(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        ws_client = _create_ws_mock()
        ws_iter = iter([ws_client])
        ws_client.constructor_calls = []

        def create_ws_client(*args, **kwargs):
            ws_client.constructor_calls.append((args, kwargs))
            return next(ws_iter)

        monkeypatch.setattr(
            "nautilus_trader.adapters.architect_ax.data.nautilus_pyo3.AxMdWebSocketClient.without_auth",
            create_ws_client,
        )

        mock_http_client.reset_mock()
        mock_http_client.authenticate_auto.return_value = "test_bearer_token"
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = [
            MagicMock(name="py_instrument"),
        ]

        config = AxDataClientConfig(
            environment=nautilus_pyo3.AxEnvironment.SANDBOX,
            update_instruments_interval_mins=None,
            **(config_kwargs or {}),
        )

        client = AxDataClient(
            loop=event_loop,
            client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        return client, ws_client, mock_http_client, mock_instrument_provider

    return builder


@pytest.mark.asyncio
async def test_connect_and_disconnect(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    # Act
    await client._connect()

    try:
        # Assert
        instrument_provider.initialize.assert_awaited_once()
        http_client.authenticate_auto.assert_awaited_once_with(3600)
        ws_client.set_auth_token.assert_called_once_with("test_bearer_token")
        http_client.cache_instrument.assert_called_once_with(
            instrument_provider.instruments_pyo3.return_value[0],
        )
        ws_client.cache_instrument.assert_called_once_with(
            instrument_provider.instruments_pyo3.return_value[0],
        )
        ws_client.connect.assert_awaited_once()
    finally:
        await client._disconnect()

    http_client.cancel_all_requests.assert_called_once()
    ws_client.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_connect_uses_configured_heartbeat(data_client_builder, monkeypatch):
    client, ws_client, _, _ = data_client_builder(
        monkeypatch,
        config_kwargs={
            "heartbeat_interval_secs": 12,
            "transport_backend": TransportBackend.TUNGSTENITE,
        },
    )

    await client._connect()

    try:
        kwargs = ws_client.constructor_calls[0][1]
        assert kwargs["heartbeat"] == 12
        assert kwargs["transport_backend"] == TransportBackend.TUNGSTENITE
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_request_instruments_returns_loaded_venue_instruments(
    data_client_builder,
    instrument,
    monkeypatch,
):
    client, _, _, instrument_provider = data_client_builder(monkeypatch)
    handle_instruments = MagicMock()
    monkeypatch.setattr(client, "_handle_instruments", handle_instruments)
    correlation_id = UUID4()
    request = SimpleNamespace(
        venue=AX_VENUE,
        id=correlation_id,
        start=None,
        end=None,
        params={"source": "test"},
    )

    await client._request_instruments(request)

    instrument_provider.get_all.assert_called_once()
    handle_instruments.assert_called_once_with(
        AX_VENUE,
        [instrument],
        correlation_id,
        None,
        None,
        {"source": "test"},
    )


@pytest.mark.asyncio
async def test_request_order_book_snapshot_emits_snapshot_deltas(
    data_client_builder,
    instrument,
    monkeypatch,
):
    client, _, http_client, _ = data_client_builder(monkeypatch)
    client._cache.add_instrument(instrument)
    http_client.request_book_snapshot = AsyncMock(return_value=Pyo3BookSnapshot())
    handle_data_response = MagicMock()
    monkeypatch.setattr(client, "_handle_data_response", handle_data_response)
    correlation_id = UUID4()
    request = SimpleNamespace(
        instrument_id=instrument.id,
        limit=25,
        id=correlation_id,
        params={"source": "test"},
    )

    await client._request_order_book_snapshot(request)

    http_client.request_book_snapshot.assert_awaited_once()
    _, kwargs = http_client.request_book_snapshot.await_args
    assert kwargs["instrument_id"].value == instrument.id.value
    assert kwargs["depth"] == 25

    handle_data_response.assert_called_once()
    response = handle_data_response.call_args.kwargs
    assert response["data_type"].type is OrderBookDeltas
    assert response["data_type"].metadata == {"instrument_id": instrument.id}
    assert response["correlation_id"] == correlation_id
    assert response["start"] is None
    assert response["end"] is None
    assert response["params"] == {"source": "test"}

    data = response["data"]
    assert len(data) == 1
    deltas = data[0]
    assert deltas.is_snapshot
    assert len(deltas.deltas) == 3
    assert deltas.deltas[0].action == BookAction.CLEAR
    assert deltas.deltas[0].order.side == OrderSide.NO_ORDER_SIDE
    assert deltas.deltas[1].order.side == OrderSide.BUY
    assert deltas.deltas[1].order.price == instrument.make_price(50_000.0)
    assert deltas.deltas[1].order.size == instrument.make_qty(3)
    assert deltas.deltas[2].order.side == OrderSide.SELL
    assert deltas.deltas[2].order.price == instrument.make_price(50_001.0)
    assert deltas.deltas[2].order.size == instrument.make_qty(5)
    assert deltas.deltas[2].flags == RecordFlag.F_SNAPSHOT | RecordFlag.F_LAST


@pytest.mark.asyncio
async def test_auth_refresh_retries_and_updates_websocket_token(data_client_builder, monkeypatch):
    client, ws_client, http_client, _ = data_client_builder(monkeypatch)
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
        "nautilus_trader.adapters.architect_ax.data.asyncio.sleep",
        record_sleep,
    )
    http_client.authenticate_auto = AsyncMock(side_effect=authenticate_auto)
    client._ws_client = ws_client
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
async def test_subscribe_order_book_deltas_l2(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        ws_client.subscribe_book_deltas.reset_mock()

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("GBPUSD-PERP.AX")
        ws_client.subscribe_book_deltas.assert_awaited_once_with(
            expected_id,
            nautilus_pyo3.AxMarketDataLevel.LEVEL2,
        )
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_l3(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        ws_client.subscribe_book_deltas.reset_mock()

        command = SimpleNamespace(
            book_type=BookType.L3_MBO,
            instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("GBPUSD-PERP.AX")
        ws_client.subscribe_book_deltas.assert_awaited_once_with(
            expected_id,
            nautilus_pyo3.AxMarketDataLevel.LEVEL3,
        )
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_quote_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        ws_client.subscribe_quotes.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        )

        # Act
        await client._subscribe_quote_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("GBPUSD-PERP.AX")
        ws_client.subscribe_quotes.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_trade_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        ws_client.subscribe_trades.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        )

        # Act
        await client._subscribe_trade_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("GBPUSD-PERP.AX")
        ws_client.subscribe_trades.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_bars(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        ws_client.subscribe_bars.reset_mock()

        bar_type = BarType.from_str("GBPUSD-PERP.AX-1-MINUTE-LAST-EXTERNAL")
        command = SimpleNamespace(bar_type=bar_type)

        # Act
        await client._subscribe_bars(command)

        # Assert
        expected_bar_type = nautilus_pyo3.BarType.from_str(
            "GBPUSD-PERP.AX-1-MINUTE-LAST-EXTERNAL",
        )
        ws_client.subscribe_bars.assert_awaited_once_with(expected_bar_type)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_order_book_deltas(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        ws_client.unsubscribe_book_deltas.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        )

        # Act
        await client._unsubscribe_order_book_deltas(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("GBPUSD-PERP.AX")
        ws_client.unsubscribe_book_deltas.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_quote_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        ws_client.unsubscribe_quotes.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        )

        # Act
        await client._unsubscribe_quote_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("GBPUSD-PERP.AX")
        ws_client.unsubscribe_quotes.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_trade_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        ws_client.unsubscribe_trades.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
        )

        # Act
        await client._unsubscribe_trade_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("GBPUSD-PERP.AX")
        ws_client.unsubscribe_trades.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_bars(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        ws_client.unsubscribe_bars.reset_mock()

        bar_type = BarType.from_str("GBPUSD-PERP.AX-1-MINUTE-LAST-EXTERNAL")
        command = SimpleNamespace(bar_type=bar_type)

        # Act
        await client._unsubscribe_bars(command)

        # Assert
        expected_bar_type = nautilus_pyo3.BarType.from_str(
            "GBPUSD-PERP.AX-1-MINUTE-LAST-EXTERNAL",
        )
        ws_client.unsubscribe_bars.assert_awaited_once_with(expected_bar_type)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_funding_rates_creates_task(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        instrument_id = InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE)
        command = SimpleNamespace(instrument_id=instrument_id)

        # Act
        await client._subscribe_funding_rates(command)

        # Assert
        assert instrument_id in client._funding_rate_tasks
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_funding_rates_cancels_task(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        instrument_id = InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE)
        subscribe_cmd = SimpleNamespace(instrument_id=instrument_id)
        await client._subscribe_funding_rates(subscribe_cmd)
        assert instrument_id in client._funding_rate_tasks

        unsubscribe_cmd = SimpleNamespace(instrument_id=instrument_id)

        # Act
        await client._unsubscribe_funding_rates(unsubscribe_cmd)

        # Assert
        assert instrument_id not in client._funding_rate_tasks
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_order_book_before_ws_connected(data_client_builder, monkeypatch):
    """
    Subscribing before WS connection should log warning and return.
    """
    # Arrange
    client, _, _, _ = data_client_builder(monkeypatch)

    command = SimpleNamespace(
        book_type=BookType.L2_MBP,
        instrument_id=InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE),
    )

    # Act - should not raise
    await client._subscribe_order_book_deltas(command)


@pytest.mark.asyncio
async def test_subscribe_duplicate_funding_rates_is_noop(data_client_builder, monkeypatch):
    """
    Subscribing to the same instrument twice should not create a second task.
    """
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)
    await client._connect()

    try:
        instrument_id = InstrumentId(Symbol("GBPUSD-PERP"), AX_VENUE)
        command = SimpleNamespace(instrument_id=instrument_id)

        await client._subscribe_funding_rates(command)
        first_task = client._funding_rate_tasks[instrument_id]

        # Act
        await client._subscribe_funding_rates(command)

        # Assert - same task, not replaced
        assert client._funding_rate_tasks[instrument_id] is first_task
    finally:
        await client._disconnect()
