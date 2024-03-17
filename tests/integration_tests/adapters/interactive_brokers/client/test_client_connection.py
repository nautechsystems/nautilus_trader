import asyncio
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest
from ibapi.common import NO_VALID_ID
from ibapi.errors import CONNECT_FAIL


@pytest.mark.asyncio
async def test_connect_success(ib_client):
    ib_client._initialize_connection_params = MagicMock()
    ib_client._connect_socket = AsyncMock()
    ib_client._send_version_info = AsyncMock()
    ib_client._receive_server_info = AsyncMock()
    ib_client._eclient.connTime = MagicMock()
    ib_client._eclient.setConnState = MagicMock()

    await ib_client._connect()

    ib_client._initialize_connection_params.assert_called_once()
    ib_client._connect_socket.assert_awaited_once()
    ib_client._send_version_info.assert_awaited_once()
    ib_client._receive_server_info.assert_awaited_once()
    ib_client._eclient.setConnState.assert_called_with(ib_client._eclient.CONNECTED)


@pytest.mark.asyncio
async def test_connect_cancelled(ib_client):
    ib_client._initialize_connection_params = MagicMock()
    ib_client._connect_socket = AsyncMock(side_effect=asyncio.CancelledError())
    ib_client._disconnect = AsyncMock()

    await ib_client._connect()

    ib_client._disconnect.assert_awaited_once()


@pytest.mark.asyncio
async def test_connect_fail(ib_client):
    ib_client._initialize_connection_params = MagicMock()
    ib_client._connect_socket = AsyncMock(side_effect=Exception("Connection failed"))
    ib_client._disconnect = AsyncMock()
    ib_client._handle_reconnect = AsyncMock()
    ib_client._eclient.wrapper.error = MagicMock()

    await ib_client._connect()

    ib_client._eclient.wrapper.error.assert_called_with(
        NO_VALID_ID,
        CONNECT_FAIL.code(),
        CONNECT_FAIL.msg(),
    )
    ib_client._handle_reconnect.assert_awaited_once()
