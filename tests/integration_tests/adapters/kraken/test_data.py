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
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.kraken.config import KrakenDataClientConfig
from nautilus_trader.adapters.kraken.constants import KRAKEN_VENUE
from nautilus_trader.adapters.kraken.data import KrakenDataClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import KrakenProductType
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.kraken.conftest import _create_ws_mock


@pytest.fixture
def data_client_builder(
    event_loop,
    mock_http_client,
    live_clock,
    mock_instrument_provider,
):
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import MessageBus

    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        ws_client = _create_ws_mock()

        mock_http_client.reset_mock()
        mock_http_client.request_instruments.return_value = []
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = [
            MagicMock(name="py_instrument"),
        ]

        config = KrakenDataClientConfig(
            product_types=(KrakenProductType.SPOT,),
            **(config_kwargs or {}),
        )

        cache = Cache()
        msgbus = MessageBus(
            trader_id=TestIdStubs.trader_id(),
            clock=live_clock,
        )

        client = KrakenDataClient(
            loop=event_loop,
            http_client_spot=mock_http_client,
            http_client_futures=None,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        # Override the WebSocket clients with our mock
        client._ws_client = ws_client
        client._ws_client_spot = ws_client

        return client, ws_client, mock_http_client, mock_instrument_provider

    return builder


@pytest.mark.asyncio
async def test_connect_and_disconnect_manage_resources(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    # Act
    await client._connect()

    try:
        # Assert
        instrument_provider.initialize.assert_awaited_once()
        http_client.cache_instrument.assert_called_once_with(
            instrument_provider.instruments_pyo3.return_value[0],
        )
        ws_client.connect.assert_awaited_once()
        assert ws_client.wait_until_active.await_count >= 1
    finally:
        await client._disconnect()

    # Assert
    ws_client.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)

    await client._connect()
    try:
        ws_client.subscribe_book.reset_mock()

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=10,
            instrument_id=InstrumentId(Symbol("XBT/USDT"), KRAKEN_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        ws_client.subscribe_book.assert_awaited_once()
        call_args = ws_client.subscribe_book.call_args[0]
        assert str(call_args[0]) == "XBT/USDT.KRAKEN"
        assert call_args[1] == 10
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_order_book_default_depth(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)

    await client._connect()
    try:
        ws_client.subscribe_book.reset_mock()

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=0,
            instrument_id=InstrumentId(Symbol("XBT/USDT"), KRAKEN_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        ws_client.subscribe_book.assert_awaited_once()
        call_args = ws_client.subscribe_book.call_args[0]
        assert call_args[1] == 10
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
            instrument_id=InstrumentId(Symbol("XBT/USDT"), KRAKEN_VENUE),
        )

        # Act
        await client._subscribe_quote_ticks(command)

        # Assert
        ws_client.subscribe_quotes.assert_awaited_once()
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
            instrument_id=InstrumentId(Symbol("XBT/USDT"), KRAKEN_VENUE),
        )

        # Act
        await client._subscribe_trade_ticks(command)

        # Assert
        ws_client.subscribe_trades.assert_awaited_once()
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
            instrument_id=InstrumentId(Symbol("XBT/USDT"), KRAKEN_VENUE),
        )

        # Act
        await client._unsubscribe_quote_ticks(command)

        # Assert
        ws_client.unsubscribe_quotes.assert_awaited_once()
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
            instrument_id=InstrumentId(Symbol("XBT/USDT"), KRAKEN_VENUE),
        )

        # Act
        await client._unsubscribe_trade_ticks(command)

        # Assert
        ws_client.unsubscribe_trades.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_order_book_deltas(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, _, _ = data_client_builder(monkeypatch)

    await client._connect()
    try:
        ws_client.unsubscribe_book.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("XBT/USDT"), KRAKEN_VENUE),
        )

        # Act
        await client._unsubscribe_order_book_deltas(command)

        # Assert
        ws_client.unsubscribe_book.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_request_instrument_receives_single_instrument(
    data_client_builder,
    instrument,
    live_clock,
    monkeypatch,
) -> None:
    client, ws_client, http_client, instrument_provider = data_client_builder(monkeypatch)
    instrument_provider.get_all.return_value = {instrument.id: instrument}

    # Use the client's internal msgbus for subscription
    msgbus = client._msgbus

    # Create DataEngine to process messages and publish to topics
    data_engine = DataEngine(msgbus, client._cache, live_clock)
    data_engine.register_client(client)
    data_engine.start()

    # Track received instruments via msgbus subscription
    received_instruments: list = []
    topic = f"data.instrument.{instrument.id.venue}.{instrument.id.symbol}"
    msgbus.subscribe(topic=topic, handler=received_instruments.append)

    # Mock pyo3 instrument with matching ID
    # Kraken's _request_instrument calls request_instruments() and filters by ID
    mock_pyo3_instrument = MagicMock()
    mock_pyo3_instrument.id = nautilus_pyo3.InstrumentId.from_str(instrument.id.value)

    http_client.request_instruments = AsyncMock(return_value=[mock_pyo3_instrument])

    try:
        # Patch transform_instrument_from_pyo3 to return our fixture instrument
        # Also need to patch KRAKEN_INSTRUMENT_TYPES isinstance check
        with (
            patch(
                "nautilus_trader.adapters.kraken.data.transform_instrument_from_pyo3",
                return_value=instrument,
            ),
            patch(
                "nautilus_trader.adapters.kraken.data.KRAKEN_INSTRUMENT_TYPES",
                (type(mock_pyo3_instrument),),
            ),
        ):
            # Create request
            request = RequestInstrument(
                instrument_id=instrument.id,
                start=None,
                end=None,
                client_id=ClientId(KRAKEN_VENUE.value),
                venue=KRAKEN_VENUE,
                callback=lambda x: None,
                request_id=UUID4(),
                ts_init=live_clock.timestamp_ns(),
                params=None,
            )

            # Act - Request the instrument
            await client._request_instrument(request)

        # Assert - Should receive exactly ONE instrument (not 2!)
        assert len(received_instruments) == 1, (
            f"Expected 1 instrument publication, got {len(received_instruments)}. "
            f"This indicates duplicate publication bug! "
            f"Received: {received_instruments}"
        )
        assert received_instruments[0].id == instrument.id
    finally:
        data_engine.stop()
        await client._disconnect()
