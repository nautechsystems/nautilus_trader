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

from nautilus_trader.adapters.okx.config import OKXDataClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.data import OKXDataClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from tests.integration_tests.adapters.okx.conftest import _create_ws_mock


@pytest.fixture()
def data_client_builder(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        public_ws = _create_ws_mock()
        business_ws = _create_ws_mock()
        ws_iter = iter([public_ws, business_ws])

        monkeypatch.setattr(
            "nautilus_trader.adapters.okx.data.nautilus_pyo3.OKXWebSocketClient",
            lambda *args, **kwargs: next(ws_iter),
        )

        mock_http_client.reset_mock()
        mock_http_client.request_instruments.return_value = []
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = [MagicMock(name="py_instrument")]

        config = OKXDataClientConfig(
            api_key="test_api_key",
            api_secret="test_api_secret",
            api_passphrase="test_passphrase",
            instrument_types=(nautilus_pyo3.OKXInstrumentType.SPOT,),
            update_instruments_interval_mins=1,
            **(config_kwargs or {}),
        )

        client = OKXDataClient(
            loop=event_loop,
            client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        return client, public_ws, business_ws, mock_http_client, mock_instrument_provider

    return builder


@pytest.mark.asyncio
async def test_connect_and_disconnect_manage_resources(data_client_builder, monkeypatch):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
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
        public_ws.connect.assert_awaited_once()
        assert public_ws.wait_until_active.await_count >= 1
        business_ws.connect.assert_awaited_once()
        public_ws.subscribe_instruments.assert_awaited_once_with(
            nautilus_pyo3.OKXInstrumentType.SPOT,
        )
    finally:
        await client._disconnect()

    # Assert
    http_client.cancel_all_requests.assert_called_once()
    public_ws.close.assert_awaited_once()
    business_ws.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_depth_default_uses_standard_channel(
    data_client_builder,
    monkeypatch,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        public_ws.subscribe_book_with_depth.reset_mock()
        public_ws.vip_level = nautilus_pyo3.OKXVipLevel.VIP0

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=0,
            instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        public_ws.subscribe_book_with_depth.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_depth_50_requires_vip(data_client_builder, monkeypatch):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        public_ws.subscribe_book_with_depth.reset_mock()
        public_ws.vip_level = nautilus_pyo3.OKXVipLevel.VIP3

        # Configure mock to raise for insufficient VIP level
        async def mock_subscribe_with_vip_check(instrument_id, depth):
            if depth == 50 and public_ws.vip_level.value < 4:
                raise ValueError(
                    f"VIP level {public_ws.vip_level} insufficient for 50 depth subscription (requires VIP4)",
                )

        public_ws.subscribe_book_with_depth.side_effect = mock_subscribe_with_vip_check

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=50,
            instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
        )

        # Act & Assert
        with pytest.raises(ValueError, match="insufficient for 50 depth"):
            await client._subscribe_order_book_deltas(command)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_depth_50_with_vip_calls_compact(
    data_client_builder,
    monkeypatch,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        public_ws.subscribe_book_with_depth.reset_mock()
        public_ws.vip_level = nautilus_pyo3.OKXVipLevel.VIP4

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=50,
            instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        public_ws.subscribe_book_with_depth.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_bars_uses_business_websocket(data_client_builder, monkeypatch):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        captured_args = {}

        def fake_from_str(value):
            captured_args["value"] = value
            return MagicMock(name="bar_type")

        monkeypatch.setattr(
            "nautilus_trader.adapters.okx.data.nautilus_pyo3.BarType.from_str",
            fake_from_str,
        )

        bar_command = SimpleNamespace(bar_type="BAR-TYPE")

        # Act
        await client._subscribe_bars(bar_command)

        # Assert
        business_ws.subscribe_bars.assert_awaited_once()
        assert captured_args["value"] == str(bar_command.bar_type)
    finally:
        await client._disconnect()
