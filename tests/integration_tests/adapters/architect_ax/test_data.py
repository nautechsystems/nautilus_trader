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

from types import SimpleNamespace
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.architect_ax.config import AxDataClientConfig
from nautilus_trader.adapters.architect_ax.constants import AX_VENUE
from nautilus_trader.adapters.architect_ax.data import AxDataClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from tests.integration_tests.adapters.architect_ax.conftest import _create_ws_mock


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

        monkeypatch.setattr(
            "nautilus_trader.adapters.architect_ax.data.nautilus_pyo3.AxMdWebSocketClient.without_auth",
            lambda *args, **kwargs: next(ws_iter),
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
        http_client.authenticate_auto.assert_awaited_once()
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
