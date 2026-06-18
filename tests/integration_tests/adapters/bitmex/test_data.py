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
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pandas as pd
import pytest

from nautilus_trader.adapters.bitmex import data as bitmex_data_module
from nautilus_trader.adapters.bitmex.config import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex.constants import BITMEX_VENUE
from nautilus_trader.adapters.bitmex.data import BitmexDataClient
from nautilus_trader.core.nautilus_pyo3 import BitmexEnvironment
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


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
        environment=BitmexEnvironment.TESTNET,
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
        mock_http_client.cache_instrument.assert_called_once_with(
            mock_instrument_provider.instruments_pyo3.return_value[0],
        )
        mock_ws_client.connect.assert_awaited_once()
        mock_ws_client.wait_until_active.assert_awaited_once_with(timeout_secs=10.0)
    finally:
        await bitmex_data_client._disconnect()

    # Assert
    mock_ws_client.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_request_order_book_snapshot_emits_snapshot_deltas(
    bitmex_data_client,
    instrument,
    mock_http_client,
    monkeypatch,
):
    # Arrange
    bitmex_data_client._cache.add_instrument(instrument)
    mock_http_client.request_book_snapshot = AsyncMock(return_value=Pyo3BookSnapshot())
    handle_data_response = MagicMock()
    monkeypatch.setattr(bitmex_data_client, "_handle_data_response", handle_data_response)

    correlation_id = UUID4()
    request = SimpleNamespace(
        instrument_id=instrument.id,
        limit=25,
        id=correlation_id,
        params={"source": "test"},
    )

    # Act
    await bitmex_data_client._request_order_book_snapshot(request)

    # Assert
    mock_http_client.request_book_snapshot.assert_awaited_once()
    _, kwargs = mock_http_client.request_book_snapshot.await_args
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
async def test_request_funding_rates_converts_and_handles_rates(
    bitmex_data_client,
    instrument,
    mock_http_client,
    monkeypatch,
):
    # Arrange
    pyo3_rates = [MagicMock(name="pyo3_rate")]
    converted_rates = [MagicMock(name="funding_rate")]
    mock_http_client.request_funding_rates = AsyncMock(return_value=pyo3_rates)
    from_pyo3_list = MagicMock(return_value=converted_rates)
    funding_rate_update = SimpleNamespace(from_pyo3_list=from_pyo3_list)
    handle_funding_rates = MagicMock()
    monkeypatch.setattr(bitmex_data_module, "FundingRateUpdate", funding_rate_update)
    monkeypatch.setattr(bitmex_data_client, "_handle_funding_rates", handle_funding_rates)

    start = pd.Timestamp("2025-01-01T00:00:00Z")
    end = pd.Timestamp("2025-01-02T00:00:00Z")
    expected_start = start.to_pydatetime()
    expected_end = end.to_pydatetime()
    correlation_id = UUID4()
    request = SimpleNamespace(
        instrument_id=instrument.id,
        start=start,
        end=end,
        limit=2,
        id=correlation_id,
        params={"source": "test"},
    )

    # Act
    await bitmex_data_client._request_funding_rates(request)

    # Assert
    mock_http_client.request_funding_rates.assert_awaited_once()
    _, kwargs = mock_http_client.request_funding_rates.await_args
    assert kwargs["instrument_id"].value == instrument.id.value
    assert kwargs["start"] == expected_start
    assert kwargs["end"] == expected_end
    assert kwargs["limit"] == 2
    from_pyo3_list.assert_called_once_with(pyo3_rates)
    handle_funding_rates.assert_called_once_with(
        instrument.id,
        converted_rates,
        correlation_id,
        start,
        end,
        {"source": "test"},
    )


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
