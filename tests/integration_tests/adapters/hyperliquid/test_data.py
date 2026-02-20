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

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.data import HyperliquidDataClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from tests.integration_tests.adapters.hyperliquid.conftest import _create_ws_mock


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
            "nautilus_trader.adapters.hyperliquid.data.nautilus_pyo3.HyperliquidWebSocketClient",
            lambda *args, **kwargs: next(ws_iter),
        )

        mock_http_client.reset_mock()
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = [
            MagicMock(name="py_instrument"),
        ]

        config = HyperliquidDataClientConfig(
            testnet=False,
            **(config_kwargs or {}),
        )

        client = HyperliquidDataClient(
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
        ws_client.subscribe_book.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_book.assert_awaited_once_with(expected_id)
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
        ws_client.subscribe_quotes.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_quote_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_quotes.assert_awaited_once_with(expected_id)
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
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_trade_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_trades.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_mark_prices(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_mark_prices.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_mark_prices(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_mark_prices.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_index_prices(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_index_prices.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_index_prices(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_index_prices.assert_awaited_once_with(expected_id)
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
        ws_client.subscribe_bars.reset_mock()

        bar_type = BarType.from_str("BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL")
        command = SimpleNamespace(bar_type=bar_type)

        # Act
        await client._subscribe_bars(command)

        # Assert
        expected_bar_type = nautilus_pyo3.BarType.from_str(
            "BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL",
        )
        ws_client.subscribe_bars.assert_awaited_once_with(expected_bar_type)
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
        ws_client.subscribe_funding_rates.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_funding_rates(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_funding_rates.assert_awaited_once_with(expected_id)
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
        ws_client.unsubscribe_book.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_order_book_deltas(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_book.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_quote_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_quotes.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_quote_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_quotes.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_trade_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_trades.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_trade_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_trades.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_mark_prices(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_mark_prices.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_mark_prices(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_mark_prices.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_index_prices(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_index_prices.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_index_prices(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_index_prices.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_bars(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_bars.reset_mock()

        bar_type = BarType.from_str("BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL")
        command = SimpleNamespace(bar_type=bar_type)

        # Act
        await client._unsubscribe_bars(command)

        # Assert
        expected_bar_type = nautilus_pyo3.BarType.from_str(
            "BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL",
        )
        ws_client.unsubscribe_bars.assert_awaited_once_with(expected_bar_type)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_funding_rates(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_funding_rates.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_funding_rates(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_funding_rates.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()
