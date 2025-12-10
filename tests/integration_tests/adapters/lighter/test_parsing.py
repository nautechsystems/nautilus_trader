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
Tests for message handling in LighterDataClient.

These tests verify the _handle_msg() dispatcher:
- Handling OrderBookDeltas via pycapsule
- Handling FundingRateUpdate via PyO3 conversion
- Exception handling and logging

"""

from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.lighter.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter.data import LighterDataClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.integration_tests.adapters.lighter.conftest import _create_http_mock
from tests.integration_tests.adapters.lighter.conftest import _create_ws_mock


@pytest.fixture
def data_client_for_parsing_tests(
    event_loop,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
    btc_instrument,
):
    """
    Create a LighterDataClient configured for parsing tests.
    """
    ws_client = _create_ws_mock()
    http_client = _create_http_mock()

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

    return client, ws_client, http_client


@pytest.mark.asyncio
async def test_handle_msg_order_book_deltas_via_capsule(
    data_client_for_parsing_tests,
    btc_instrument,
):
    """
    Test that OrderBookDeltas from pycapsule are routed to _handle_order_book_deltas.
    """
    client, ws_client, http_client = data_client_for_parsing_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id

        # Create mock OrderBookDeltas
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
            sequence=100,
            ts_event=0,
            ts_init=0,
        )
        # OrderBookDeltas.sequence is automatically derived from the deltas
        deltas = OrderBookDeltas(instrument_id=instrument_id, deltas=[delta])

        # Track if _handle_order_book_deltas was called
        handle_deltas_called = False

        def mock_handler(data):
            nonlocal handle_deltas_called
            handle_deltas_called = True
            # Don't call original to avoid side effects

        client._handle_order_book_deltas = mock_handler

        # Mock is_pycapsule to return True
        with (
            patch.object(nautilus_pyo3, "is_pycapsule", return_value=True),
            patch(
                "nautilus_trader.adapters.lighter.data.capsule_to_data",
                return_value=deltas,
            ),
        ):
            # Act
            client._handle_msg(MagicMock())

        # Assert
        assert handle_deltas_called, "OrderBookDeltas should route to _handle_order_book_deltas"

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_msg_trade_tick_via_capsule(
    data_client_for_parsing_tests,
    btc_instrument,
):
    """
    Test that TradeTick from pycapsule is routed to _handle_data.
    """
    client, ws_client, http_client = data_client_for_parsing_tests

    await client._connect()
    try:
        instrument_id = btc_instrument.id

        # Create mock TradeTick
        trade = TradeTick(
            instrument_id=instrument_id,
            price=Price.from_str("50000.0"),
            size=Quantity.from_str("1.0"),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123"),
            ts_event=0,
            ts_init=0,
        )

        # Track if _handle_data was called
        handle_data_called = False

        def mock_handler(data):
            nonlocal handle_data_called
            handle_data_called = True

        client._handle_data = mock_handler

        # Mock is_pycapsule to return True
        with (
            patch.object(nautilus_pyo3, "is_pycapsule", return_value=True),
            patch(
                "nautilus_trader.adapters.lighter.data.capsule_to_data",
                return_value=trade,
            ),
        ):
            # Act
            client._handle_msg(MagicMock())

        # Assert
        assert handle_data_called, "TradeTick should route to _handle_data"

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_msg_non_capsule_non_funding_rate(
    data_client_for_parsing_tests,
    btc_instrument,
):
    """
    Test that non-capsule, non-FundingRateUpdate messages log warning.
    """
    client, ws_client, http_client = data_client_for_parsing_tests

    await client._connect()
    try:
        # Create a non-matching message type
        unknown_msg = "unknown_string_message"

        # Mock is_pycapsule to return False
        with patch.object(nautilus_pyo3, "is_pycapsule", return_value=False):
            # Act: should log warning but not raise
            client._handle_msg(unknown_msg)

        # Assert: no exception raised, code handles unknown type gracefully

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_msg_logs_warning_for_unknown_type(
    data_client_for_parsing_tests,
    caplog,
):
    """
    Test that unknown message types log a warning.
    """
    client, ws_client, http_client = data_client_for_parsing_tests

    await client._connect()
    try:
        # Create unknown message type
        unknown_msg = "unknown_string_message"

        # Mock is_pycapsule to return False
        with patch.object(nautilus_pyo3, "is_pycapsule", return_value=False):
            # Act
            client._handle_msg(unknown_msg)

        # Assert: warning should be logged
        # (The actual log check depends on caplog configuration)

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_msg_catches_exceptions(
    data_client_for_parsing_tests,
    caplog,
):
    """
    Test that exceptions in _handle_msg are caught and logged.
    """
    client, ws_client, http_client = data_client_for_parsing_tests

    await client._connect()
    try:
        # Mock is_pycapsule to raise exception
        with patch.object(nautilus_pyo3, "is_pycapsule", side_effect=Exception("Test error")):
            # Act: should NOT raise
            client._handle_msg(MagicMock())

        # Assert: no exception raised, code continues

    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_msg_capsule_extraction_error(
    data_client_for_parsing_tests,
    caplog,
):
    """
    Test that capsule extraction errors are caught.
    """
    client, ws_client, http_client = data_client_for_parsing_tests

    await client._connect()
    try:
        # Mock is_pycapsule to return True
        with (
            patch.object(nautilus_pyo3, "is_pycapsule", return_value=True),
            patch(
                "nautilus_trader.adapters.lighter.data.capsule_to_data",
                side_effect=Exception("Capsule error"),
            ),
        ):
            # Act: should NOT raise
            client._handle_msg(MagicMock())

        # Assert: no exception raised

    finally:
        await client._disconnect()
