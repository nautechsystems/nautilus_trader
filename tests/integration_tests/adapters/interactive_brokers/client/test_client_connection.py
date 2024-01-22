import asyncio
from unittest.mock import AsyncMock
from unittest.mock import Mock
from unittest.mock import patch

import pytest
from ibapi.client import EClient


@pytest.mark.asyncio
async def test_establish_socket_connection(ib_client):
    # Arrange
    ib_client._eclient.connState = EClient.DISCONNECTED
    ib_client._tws_incoming_msg_reader_task = None
    ib_client._internal_msg_queue_task = None
    ib_client._initialize_connection_params = Mock()
    ib_client._connect_socket = AsyncMock()
    ib_client._send_version_info = AsyncMock()
    ib_client._receive_server_info = AsyncMock()
    ib_client._eclient.serverVersion = Mock()
    ib_client._eclient.wrapper = Mock()
    ib_client._eclient.startApi = Mock()
    ib_client._eclient.conn = Mock()
    ib_client._eclient.conn.isConnected = Mock(return_value=True)

    # Act
    await ib_client._establish_socket_connection()

    # Assert
    assert ib_client._eclient.isConnected()
    assert ib_client._tws_incoming_msg_reader_task
    assert ib_client._internal_msg_queue_task
    ib_client._eclient.startApi.assert_called_once()


@pytest.mark.asyncio
async def test_connect_socket(ib_client):
    # Arrange
    with patch(
        "nautilus_trader.adapters.interactive_brokers.client.connection.Connection",
    ) as MockConnection:
        mock_connection_instance = MockConnection.return_value
        mock_connection_instance.connect = Mock()

        # Act
        await ib_client._connect_socket()
        asyncio.sleep(0.1)

        # Assert
        mock_connection_instance.connect.assert_called_once()
