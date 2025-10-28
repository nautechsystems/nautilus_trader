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

import pytest

from nautilus_trader.adapters.bitmex.config import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex.constants import BITMEX_VENUE
from nautilus_trader.adapters.bitmex.data import BitmexDataClient
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


@pytest.fixture()
def bitmex_data_client(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
    mock_ws_client,
    monkeypatch,
) -> BitmexDataClient:
    monkeypatch.setattr(
        "nautilus_trader.adapters.bitmex.data.nautilus_pyo3.BitmexWebSocketClient",
        lambda *args, **kwargs: mock_ws_client,
    )

    config = BitmexDataClientConfig(
        api_key="test_api_key",
        api_secret="test_api_secret",
        testnet=True,
        update_instruments_interval_mins=1,
    )

    client = BitmexDataClient(
        loop=event_loop,
        client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        clock=live_clock,
        instrument_provider=mock_instrument_provider,
        config=config,
        name=None,
    )

    return client


@pytest.mark.asyncio
async def test_connect_and_disconnect_manage_resources(
    bitmex_data_client,
    mock_instrument_provider,
    mock_http_client,
    mock_ws_client,
):
    # Arrange
    mock_http_client.request_instruments.return_value = []

    # Act
    await bitmex_data_client._connect()

    try:
        # Assert
        mock_instrument_provider.initialize.assert_awaited_once()
        mock_http_client.add_instrument.assert_called_once_with(
            mock_instrument_provider.instruments_pyo3.return_value[0],
        )
        mock_ws_client.connect.assert_awaited_once()
        mock_ws_client.wait_until_active.assert_awaited_once_with(timeout_secs=10.0)
        assert bitmex_data_client._update_instruments_task is not None
    finally:
        await bitmex_data_client._disconnect()

    # Assert
    mock_ws_client.close.assert_awaited_once()
    assert bitmex_data_client._update_instruments_task is None


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_rejects_unsupported_book_type(
    bitmex_data_client,
    mock_ws_client,
):
    # Arrange
    await bitmex_data_client._connect()
    try:
        mock_ws_client.connect.reset_mock()
        mock_ws_client.subscribe_book.reset_mock()
        mock_ws_client.subscribe_book_25.reset_mock()

        instrument_id = InstrumentId(Symbol("XBTUSD"), BITMEX_VENUE)
        command = SimpleNamespace(
            book_type=BookType.L1_MBP,
            depth=0,
            instrument_id=instrument_id,
        )

        # Act
        await bitmex_data_client._subscribe_order_book_deltas(command)

        # Assert
        mock_ws_client.subscribe_book.assert_not_awaited()
        mock_ws_client.subscribe_book_25.assert_not_awaited()
    finally:
        await bitmex_data_client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_depth_25_uses_compact_channel(
    bitmex_data_client,
    mock_ws_client,
):
    # Arrange
    await bitmex_data_client._connect()
    try:
        mock_ws_client.subscribe_book.reset_mock()
        mock_ws_client.subscribe_book_25.reset_mock()

        instrument_id = InstrumentId(Symbol("XBTUSD"), BITMEX_VENUE)
        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=25,
            instrument_id=instrument_id,
        )

        # Act
        await bitmex_data_client._subscribe_order_book_deltas(command)

        # Assert
        mock_ws_client.subscribe_book.assert_not_awaited()
        mock_ws_client.subscribe_book_25.assert_awaited_once()
        args, _kwargs = mock_ws_client.subscribe_book_25.await_args
        assert args[0].value == instrument_id.value
    finally:
        await bitmex_data_client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_order_book_snapshots_invalid_depth_is_ignored(
    bitmex_data_client,
    mock_ws_client,
):
    # Arrange
    await bitmex_data_client._connect()
    try:
        mock_ws_client.subscribe_book_depth10.reset_mock()

        instrument_id = InstrumentId(Symbol("XBTUSD"), BITMEX_VENUE)
        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=5,
            instrument_id=instrument_id,
        )

        # Act
        await bitmex_data_client._subscribe_order_book_snapshots(command)

        # Assert
        mock_ws_client.subscribe_book_depth10.assert_not_awaited()
    finally:
        await bitmex_data_client._disconnect()
