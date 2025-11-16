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
Tests for Betfair session error handling and automatic reconnection.

These tests verify that when session tokens expire (NO_SESSION or
INVALID_SESSION_INFORMATION errors), the adapter properly triggers reconnection logic
and re-raises exceptions for proper error handling by the execution engine.

"""

from unittest.mock import AsyncMock

import pytest
from betfair_parser.exceptions import BetfairError

from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import TraderId


@pytest.mark.asyncio
async def test_generate_order_status_reports_no_session_error_triggers_reconnect(exec_client):
    """
    Test that NO_SESSION error in generate_order_status_reports triggers reconnection
    and retries successfully with fresh session.
    """
    # Arrange
    command = GenerateOrderStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        open_only=False,
        command_id=UUID4(),
        ts_init=0,
    )

    # Mock list_current_orders: fail first time, succeed on retry
    error_msg = "NO_SESSION: A session token is required for this operation"
    call_count = 0

    async def mock_list_orders(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 1:
            raise BetfairError(error_msg)
        # Second call (after reconnect) succeeds
        return []

    exec_client._client.list_current_orders = mock_list_orders
    exec_client._reconnect = AsyncMock()

    # Act
    result = await exec_client.generate_order_status_reports(command)

    # Assert
    assert result == []  # Got results from retry
    assert call_count == 2  # Called twice: initial + retry
    exec_client._reconnect.assert_called_once()


@pytest.mark.asyncio
async def test_generate_order_status_reports_invalid_session_error_triggers_reconnect(
    exec_client,
):
    """
    Test that INVALID_SESSION_INFORMATION error triggers reconnection and retries.
    """
    # Arrange
    command = GenerateOrderStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        open_only=False,
        command_id=UUID4(),
        ts_init=0,
    )

    error_msg = "INVALID_SESSION_INFORMATION: The session token has expired"
    call_count = 0

    async def mock_list_orders(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 1:
            raise BetfairError(error_msg)
        return []

    exec_client._client.list_current_orders = mock_list_orders
    exec_client._reconnect = AsyncMock()

    # Act
    result = await exec_client.generate_order_status_reports(command)

    # Assert
    assert result == []
    assert call_count == 2
    exec_client._reconnect.assert_called_once()


@pytest.mark.asyncio
async def test_generate_fill_reports_no_session_error_triggers_reconnect(exec_client):
    """
    Test that NO_SESSION error in generate_fill_reports triggers reconnection and
    retries.
    """
    # Arrange
    command = GenerateFillReports(
        instrument_id=None,
        venue_order_id=None,
        start=None,
        end=None,
        command_id=UUID4(),
        ts_init=0,
    )

    error_msg = "NO_SESSION: A session token is required for this operation"
    call_count = 0

    async def mock_list_orders(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 1:
            raise BetfairError(error_msg)
        return []

    exec_client._client.list_current_orders = mock_list_orders
    exec_client._reconnect = AsyncMock()

    # Act
    result = await exec_client.generate_fill_reports(command)

    # Assert
    assert result == []
    assert call_count == 2
    exec_client._reconnect.assert_called_once()


@pytest.mark.asyncio
async def test_generate_order_status_report_no_session_error_triggers_reconnect(exec_client):
    """
    Test that NO_SESSION error in generate_order_status_report (singular) triggers
    reconnection and retries.
    """
    # Arrange
    command = GenerateOrderStatusReport(
        instrument_id=None,
        client_order_id=ClientOrderId("O-123"),
        venue_order_id=None,
        command_id=UUID4(),
        ts_init=0,
    )

    error_msg = "NO_SESSION: A session token is required for this operation"
    call_count = 0

    async def mock_list_orders(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 1:
            raise BetfairError(error_msg)
        return []  # Second call succeeds with empty list (order not found)

    exec_client._client.list_current_orders = mock_list_orders
    exec_client._reconnect = AsyncMock()

    # Act
    result = await exec_client.generate_order_status_report(command)

    # Assert
    assert result is None  # Returns None when order not found
    assert call_count == 2  # Called twice: initial + retry
    exec_client._reconnect.assert_called_once()


@pytest.mark.asyncio
async def test_query_account_no_session_error_triggers_reconnect(exec_client):
    """
    Test that NO_SESSION error in _query_account triggers reconnection.

    Unlike report generators, this doesn't retry - user can manually retry.

    """
    # Arrange
    command = QueryAccount(
        trader_id=TraderId("TRADER-001"),
        account_id=AccountId("BETFAIR-001"),
        command_id=UUID4(),
        ts_init=0,
    )

    error_msg = "NO_SESSION: A session token is required for this operation"
    exec_client._client.get_account_details = AsyncMock(
        side_effect=BetfairError(error_msg),
    )
    exec_client._reconnect = AsyncMock()

    # Act - should complete without raising
    await exec_client._query_account(command)

    # Assert - reconnect triggered but no retry (user command)
    exec_client._reconnect.assert_called_once()
    exec_client._client.get_account_details.assert_called_once()  # Only called once, no retry


@pytest.mark.asyncio
async def test_non_session_betfair_error_raises_without_reconnect(exec_client):
    """
    Test that non-session BetfairErrors (e.g., PERMISSION_DENIED) are raised.

    They should not trigger reconnection and should propagate the exception.

    """
    # Arrange
    command = GenerateOrderStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        open_only=False,
        command_id=UUID4(),
        ts_init=0,
    )

    error_msg = "PERMISSION_DENIED: Business rules do not allow order to be placed"
    exec_client._client.list_current_orders = AsyncMock(
        side_effect=BetfairError(error_msg),
    )
    exec_client._reconnect = AsyncMock()

    # Act & Assert
    with pytest.raises(BetfairError, match="PERMISSION_DENIED"):
        await exec_client.generate_order_status_reports(command)

    exec_client._reconnect.assert_not_called()  # No reconnection for non-session errors


@pytest.mark.asyncio
async def test_non_betfair_error_does_not_trigger_reconnect(exec_client):
    """
    Test that non-BetfairError exceptions don't trigger reconnection but still
    propagate.
    """
    # Arrange
    command = GenerateOrderStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        open_only=False,
        command_id=UUID4(),
        ts_init=0,
    )

    # Mock with generic exception (not BetfairError)
    exec_client._client.list_current_orders = AsyncMock(
        side_effect=RuntimeError("Network error"),
    )
    exec_client._reconnect = AsyncMock()

    # Act & Assert
    with pytest.raises(RuntimeError, match="Network error"):
        await exec_client.generate_order_status_reports(command)

    # Verify reconnection was NOT triggered for non-Betfair errors
    exec_client._reconnect.assert_not_called()


@pytest.mark.asyncio
async def test_session_error_retries_only_once(exec_client):
    """
    Test that retry happens only once - if second attempt also fails, raise exception.
    """
    # Arrange
    command = GenerateOrderStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        open_only=False,
        command_id=UUID4(),
        ts_init=0,
    )

    # Mock to fail both times (initial + retry)
    error_msg = "NO_SESSION: A session token is required for this operation"
    exec_client._client.list_current_orders = AsyncMock(
        side_effect=BetfairError(error_msg),
    )

    reconnect_count = 0

    async def mock_reconnect():
        nonlocal reconnect_count
        reconnect_count += 1

    exec_client._reconnect = mock_reconnect

    # Act & Assert
    with pytest.raises(BetfairError, match="NO_SESSION"):
        await exec_client.generate_order_status_reports(command)

    # Verify retry behavior
    assert reconnect_count == 2  # Reconnects on both attempts
    assert exec_client._client.list_current_orders.call_count == 2  # Only retries once


@pytest.mark.asyncio
async def test_reconnect_is_blocking(exec_client):
    """
    Test that reconnection blocks until complete before retrying.

    This ensures that the session is refreshed before retry happens, so the retry has a
    valid session.

    """
    # Arrange
    command = GenerateOrderStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        open_only=False,
        command_id=UUID4(),
        ts_init=0,
    )

    error_msg = "NO_SESSION: A session token is required for this operation"
    call_count = 0

    async def mock_list_orders(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        events.append(f"api_call_{call_count}")
        if call_count == 1:
            raise BetfairError(error_msg)
        return []

    exec_client._client.list_current_orders = mock_list_orders

    # Track execution order
    events = []

    async def mock_reconnect():
        events.append("reconnect_start")
        # Simulate reconnection taking time
        import asyncio

        await asyncio.sleep(0.01)
        events.append("reconnect_complete")

    exec_client._reconnect = mock_reconnect

    # Act
    result = await exec_client.generate_order_status_reports(command)
    events.append("method_returned")

    # Assert
    assert result == []
    # Proves: first call → reconnect (blocking) → retry call → return
    assert events == [
        "api_call_1",
        "reconnect_start",
        "reconnect_complete",
        "api_call_2",
        "method_returned",
    ]
