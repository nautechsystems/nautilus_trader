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

"""
Tests for order book gap detection and resync logic in LighterDataClient.

These tests verify the critical path in _handle_order_book_deltas():
- Tracking last sequence number per instrument
- Detecting gaps when sequence != last + 1
- Triggering HTTP snapshot resync on gap detection
- Clearing offset tracking after resync
"""

import asyncio
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.lighter.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter.constants import LIGHTER_VENUE
from nautilus_trader.adapters.lighter.data import LighterDataClient
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.integration_tests.adapters.lighter.conftest import _create_http_mock
from tests.integration_tests.adapters.lighter.conftest import _create_ws_mock


def create_mock_deltas(instrument_id: InstrumentId, sequence: int) -> OrderBookDeltas:
    """Create mock OrderBookDeltas with a specific sequence number."""
    order = BookOrder(
        side=OrderSide.BUY,
        price=Price.from_str("50000.0"),
        size=Quantity.from_str("1.0"),
        order_id=0,
    )
    delta = OrderBookDelta(
        instrument_id=instrument_id,
        action=BookAction.ADD,
        order=order,
        flags=0,
        sequence=sequence,
        ts_event=0,
        ts_init=0,
    )
    # OrderBookDeltas.sequence is automatically derived from the deltas
    return OrderBookDeltas(instrument_id=instrument_id, deltas=[delta])


@pytest.fixture
def data_client_for_sync_tests(
    event_loop,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
    btc_instrument,
):
    """Create a LighterDataClient configured for sync tests."""
    ws_client = _create_ws_mock()
    http_client = _create_http_mock()

    # Set up provider
    mock_pyo3_instrument = MagicMock()
    mock_pyo3_instrument.id.return_value = MagicMock(value=str(btc_instrument.id))
    mock_instrument_provider.instruments_pyo3.return_value = [mock_pyo3_instrument]

    config = LighterDataClientConfig(testnet=True)

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


@pytest.mark.asyncio
async def test_handle_deltas_updates_offset(data_client_for_sync_tests, btc_instrument):
    """Test that _handle_order_book_deltas updates _last_book_offsets correctly."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id
        key = str(instrument_id)

        # Create deltas with sequence 100
        deltas = create_mock_deltas(instrument_id, sequence=100)

        # Act
        client._handle_order_book_deltas(deltas)

        # Assert: offset should be updated
        assert key in client._last_book_offsets
        assert client._last_book_offsets[key] == 100
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_deltas_sequential_updates(data_client_for_sync_tests, btc_instrument):
    """Test that sequential updates (100, 101, 102) are handled correctly."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id
        key = str(instrument_id)

        # Send sequential deltas
        for seq in [100, 101, 102]:
            deltas = create_mock_deltas(instrument_id, sequence=seq)
            client._handle_order_book_deltas(deltas)

        # Assert: offset should be at 102
        assert client._last_book_offsets[key] == 102
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_deltas_detects_gap(data_client_for_sync_tests, btc_instrument, caplog):
    """Test that gap detection logs warning when sequence != last + 1."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id
        key = str(instrument_id)

        # First delta establishes baseline
        deltas1 = create_mock_deltas(instrument_id, sequence=100)
        client._handle_order_book_deltas(deltas1)

        # Gap: 100 -> 105 (missing 101-104)
        deltas2 = create_mock_deltas(instrument_id, sequence=105)

        # Act: should detect gap
        with patch.object(client, "_resync_order_book", new_callable=AsyncMock) as mock_resync:
            client._handle_order_book_deltas(deltas2)

            # Wait for async task to be created
            await asyncio.sleep(0.01)

            # Assert: gap detection should NOT update offset (returns early)
            assert client._last_book_offsets[key] == 100

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_deltas_triggers_resync_on_gap(data_client_for_sync_tests, btc_instrument):
    """Test that gap detection triggers _resync_order_book()."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id

        # First delta establishes baseline
        deltas1 = create_mock_deltas(instrument_id, sequence=100)
        client._handle_order_book_deltas(deltas1)

        # Track if resync was called
        resync_called = False
        original_resync = client._resync_order_book

        async def mock_resync(inst_id):
            nonlocal resync_called
            resync_called = True

        client._resync_order_book = mock_resync

        # Gap: 100 -> 105 (missing 101-104)
        deltas2 = create_mock_deltas(instrument_id, sequence=105)
        client._handle_order_book_deltas(deltas2)

        # Wait for the async task
        await asyncio.sleep(0.05)

        # Assert
        assert resync_called, "Expected _resync_order_book to be called on gap"

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_resync_fetches_http_snapshot(data_client_for_sync_tests, btc_instrument):
    """Test that _resync_order_book() calls HTTP client for snapshot."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id
        key = str(instrument_id)

        # Set up the PyO3 instrument in client's cache
        mock_pyo3_inst = MagicMock()
        client._pyo3_instruments[key] = mock_pyo3_inst

        # Configure HTTP client to return mock snapshot
        mock_snapshot = create_mock_deltas(instrument_id, sequence=200)
        http_client.get_order_book_snapshot = AsyncMock(return_value=mock_snapshot)

        # Act
        await client._resync_order_book(instrument_id)

        # Assert
        http_client.get_order_book_snapshot.assert_awaited_once_with(mock_pyo3_inst)

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_resync_clears_offset_tracking(data_client_for_sync_tests, btc_instrument):
    """Test that _resync_order_book() clears the offset after successful resync."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id
        key = str(instrument_id)

        # Set initial offset
        client._last_book_offsets[key] = 100

        # Set up the PyO3 instrument in client's cache
        mock_pyo3_inst = MagicMock()
        client._pyo3_instruments[key] = mock_pyo3_inst

        # Configure HTTP client to return mock snapshot
        mock_snapshot = create_mock_deltas(instrument_id, sequence=200)
        http_client.get_order_book_snapshot = AsyncMock(return_value=mock_snapshot)

        # Act
        await client._resync_order_book(instrument_id)

        # Assert: offset should be cleared
        assert key not in client._last_book_offsets

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_resync_handles_missing_instrument(data_client_for_sync_tests, btc_instrument, caplog):
    """Test that _resync_order_book() handles missing PyO3 instrument gracefully."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id

        # Clear the PyO3 instruments cache to simulate missing instrument
        client._pyo3_instruments.clear()

        # Act
        await client._resync_order_book(instrument_id)

        # Assert: should NOT call HTTP client (missing instrument)
        http_client.get_order_book_snapshot.assert_not_awaited()

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_resync_handles_http_exception(data_client_for_sync_tests, btc_instrument, caplog):
    """Test that _resync_order_book() handles HTTP exceptions gracefully."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id
        key = str(instrument_id)

        # Set up the PyO3 instrument in client's cache
        mock_pyo3_inst = MagicMock()
        client._pyo3_instruments[key] = mock_pyo3_inst

        # Configure HTTP client to raise exception
        http_client.get_order_book_snapshot = AsyncMock(
            side_effect=Exception("HTTP error"),
        )

        # Set initial offset
        client._last_book_offsets[key] = 100

        # Act: should NOT raise
        await client._resync_order_book(instrument_id)

        # Assert: offset should NOT be cleared on error
        assert client._last_book_offsets.get(key) == 100

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_first_delta_without_prior_offset(data_client_for_sync_tests, btc_instrument):
    """Test that first delta (no prior offset) doesn't trigger resync."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id
        key = str(instrument_id)

        # Ensure no prior offset
        assert key not in client._last_book_offsets

        resync_called = False

        async def mock_resync(inst_id):
            nonlocal resync_called
            resync_called = True

        client._resync_order_book = mock_resync

        # First delta - should NOT trigger resync
        deltas = create_mock_deltas(instrument_id, sequence=100)
        client._handle_order_book_deltas(deltas)

        await asyncio.sleep(0.01)

        # Assert
        assert not resync_called, "First delta should NOT trigger resync"
        assert client._last_book_offsets[key] == 100

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_deltas_without_sequence_are_passed_through(
    data_client_for_sync_tests,
    btc_instrument,
):
    """Test that deltas without sequence attribute are passed through without gap check."""
    client, ws_client, http_client, provider = data_client_for_sync_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id

        # Create deltas without sequence attribute
        order = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("50000.0"),
            size=Quantity.from_str("1.0"),
            order_id=0,
        )
        delta = OrderBookDelta(
            instrument_id=instrument_id,
            action=BookAction.ADD,
            order=order,
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        deltas = OrderBookDeltas(instrument_id=instrument_id, deltas=[delta])
        # No .sequence attribute set

        data_handled = False
        original_handle_data = client._handle_data

        def mock_handle_data(data):
            nonlocal data_handled
            data_handled = True
            original_handle_data(data)

        client._handle_data = mock_handle_data

        # Act
        client._handle_order_book_deltas(deltas)

        # Assert: data should be passed through
        assert data_handled

    finally:
        await client._disconnect()
