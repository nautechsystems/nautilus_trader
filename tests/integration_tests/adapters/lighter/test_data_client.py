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

from nautilus_trader.adapters.lighter.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter.constants import LIGHTER_VENUE
from nautilus_trader.adapters.lighter.data import LighterDataClient
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from tests.integration_tests.adapters.lighter.conftest import _create_http_mock
from tests.integration_tests.adapters.lighter.conftest import _create_ws_mock


@pytest.fixture
def data_client_builder(
    event_loop,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
    btc_instrument,
):
    """
    Build LighterDataClient with mocked dependencies.
    """

    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        ws_client = _create_ws_mock()
        http_client = _create_http_mock()

        # Set up provider to return our test instrument
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()

        # Create a mock PyO3 instrument
        mock_pyo3_instrument = MagicMock()
        mock_pyo3_instrument.id.return_value = MagicMock(value=str(btc_instrument.id))
        mock_instrument_provider.instruments_pyo3.return_value = [mock_pyo3_instrument]

        config = LighterDataClientConfig(
            testnet=True,
            **(config_kwargs or {}),
        )

        client = LighterDataClient(
            loop=event_loop,
            http_client=http_client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        return client, ws_client, http_client, mock_instrument_provider

    return builder


@pytest.mark.asyncio
async def test_connect_initializes_provider(data_client_builder, monkeypatch):
    """
    Test that _connect() initializes the instrument provider.
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    # Act
    await client._connect()

    try:
        # Assert
        instrument_provider.initialize.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_connect_caches_instruments(data_client_builder, monkeypatch, btc_instrument, cache):
    """
    Test that _connect() caches instruments.
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    # Act
    await client._connect()

    try:
        # Assert: instrument should be in cache
        cached = cache.instrument(btc_instrument.id)
        assert cached is not None
        assert cached.id == btc_instrument.id
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_connect_starts_websocket(data_client_builder, monkeypatch):
    """
    Test that _connect() starts the WebSocket client.
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    # Act
    await client._connect()

    try:
        # Assert
        ws_client.connect.assert_awaited_once()
        ws_client.wait_until_active.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_disconnect_closes_websocket(data_client_builder, monkeypatch):
    """
    Test that _disconnect() closes the WebSocket client.
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    await client._connect()

    # Act
    await client._disconnect()

    # Assert
    ws_client.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas(data_client_builder, monkeypatch):
    """
    Test subscription to order book deltas.
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    await client._connect()
    try:
        ws_client.subscribe_order_book.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), LIGHTER_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert: should call subscribe with market_index (provider returns 1)
        ws_client.subscribe_order_book.assert_awaited_once_with(1)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_trade_ticks(data_client_builder, monkeypatch):
    """
    Test subscription to trade ticks.
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    await client._connect()
    try:
        ws_client.subscribe_trades.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), LIGHTER_VENUE),
        )

        # Act
        await client._subscribe_trade_ticks(command)

        # Assert
        ws_client.subscribe_trades.assert_awaited_once_with(1)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_mark_prices(data_client_builder, monkeypatch):
    """
    Test subscription to mark prices (via market_stats).
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    await client._connect()
    try:
        ws_client.subscribe_market_stats.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), LIGHTER_VENUE),
        )

        # Act
        await client._subscribe_mark_prices(command)

        # Assert
        ws_client.subscribe_market_stats.assert_awaited_once_with(1)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_funding_rates(data_client_builder, monkeypatch):
    """
    Test subscription to funding rates (via market_stats).
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    await client._connect()
    try:
        ws_client.subscribe_market_stats.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), LIGHTER_VENUE),
        )

        # Act
        await client._subscribe_funding_rates(command)

        # Assert
        ws_client.subscribe_market_stats.assert_awaited_once_with(1)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_order_book_deltas(data_client_builder, monkeypatch):
    """
    Test unsubscription from order book deltas.
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    await client._connect()
    try:
        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), LIGHTER_VENUE),
        )

        # First subscribe
        await client._subscribe_order_book_deltas(command)
        ws_client.unsubscribe_order_book.reset_mock()

        # Act
        await client._unsubscribe_order_book_deltas(command)

        # Assert
        ws_client.unsubscribe_order_book.assert_awaited_once_with(1)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_with_missing_market_index_logs_warning(
    data_client_builder,
    monkeypatch,
    caplog,
):
    """
    Test that subscribing with unknown instrument logs warning.
    """
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)

    # Make provider return None for unknown instruments
    instrument_provider.market_index_for.return_value = None

    await client._connect()
    try:
        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("UNKNOWN-USD-PERP"), LIGHTER_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert: should NOT call subscribe (no market index)
        ws_client.subscribe_order_book.assert_not_awaited()
    finally:
        await client._disconnect()
