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

from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.data import BybitDataClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from tests.integration_tests.adapters.bybit.conftest import _create_ws_mock


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
            "nautilus_trader.adapters.bybit.data.nautilus_pyo3.BybitWebSocketClient.new_public",
            lambda *args, **kwargs: next(ws_iter),
        )

        mock_http_client.reset_mock()
        mock_http_client.request_instruments.return_value = []
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = [
            MagicMock(name="py_instrument"),
        ]

        config = BybitDataClientConfig(
            api_key="test_api_key",
            api_secret="test_api_secret",
            product_types=(nautilus_pyo3.BybitProductType.SPOT,),
            **(config_kwargs or {}),
        )

        client = BybitDataClient(
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
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_orderbook.reset_mock()

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=50,
            instrument_id=InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTCUSDT-SPOT.BYBIT")
        ws_client.subscribe_orderbook.assert_awaited_once_with(expected_id, 50)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_quote_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_orderbook.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE),
        )

        # Act
        await client._subscribe_quote_ticks(command)

        # Assert: SPOT instruments use orderbook depth=1 for quotes
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTCUSDT-SPOT.BYBIT")
        ws_client.subscribe_orderbook.assert_awaited_once_with(expected_id, 1)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_trade_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_trades.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE),
        )

        # Act
        await client._subscribe_trade_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTCUSDT-SPOT.BYBIT")
        ws_client.subscribe_trades.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_bars(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_klines.reset_mock()

        # Mock bar type with 1-minute interval
        bar_type = MagicMock()
        bar_type.instrument_id = InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE)
        bar_type.spec.aggregation = BarAggregation.MINUTE
        bar_type.spec.step = 1

        command = SimpleNamespace(bar_type=bar_type)

        # Act
        await client._subscribe_bars(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTCUSDT-SPOT.BYBIT")
        ws_client.subscribe_klines.assert_awaited_once_with(expected_id, "1")
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_funding_rates(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_ticker.reset_mock()

        # SPOT instruments don't support funding rates
        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE),
        )

        # Act
        await client._subscribe_funding_rates(command)

        # Assert: SPOT should not subscribe (returns early with warning)
        ws_client.subscribe_ticker.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_order_book_deltas(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        # First subscribe to track the depth
        subscribe_command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=50,
            instrument_id=InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE),
        )
        await client._subscribe_order_book_deltas(subscribe_command)

        ws_client.unsubscribe_orderbook.reset_mock()

        # Now unsubscribe
        unsubscribe_command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE),
        )

        # Act
        await client._unsubscribe_order_book_deltas(unsubscribe_command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTCUSDT-SPOT.BYBIT")
        ws_client.unsubscribe_orderbook.assert_awaited_once_with(expected_id, 50)
    finally:
        await client._disconnect()
