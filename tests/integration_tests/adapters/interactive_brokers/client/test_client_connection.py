import asyncio
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest


def test_establish_socket_connection(ib_client):
    # Arrange
    ib_client._eclient.startApi = MagicMock()
    ib_client.connection_manager._initialize_connection = AsyncMock()
    ib_client.connection_manager._connect_socket = AsyncMock()
    ib_client.connection_manager._send_version_info = AsyncMock()
    ib_client.connection_manager._receive_server_info = AsyncMock()
    ib_client.connection_manager._setup_client = AsyncMock()
    ib_client.connection_manager._handle_connection_error = AsyncMock()

    # Act
    ib_client.connection_manager._establish_socket_connection()

    # Assert
    ib_client.connection_manager._setup_client.assert_called_once()


@pytest.mark.asyncio
async def test_run_watch_dog(ib_client):
    # Arrange
    connection_manager = ib_client.connection_manager
    connection_manager._resume_client_if_degraded = AsyncMock()
    connection_manager._start_client_if_initialized_but_not_running = AsyncMock()
    connection_manager._handle_ib_is_not_ready = AsyncMock()
    connection_manager._monitor_and_reconnect_socket = AsyncMock()
    ib_client._eclient.isConnected = MagicMock()
    ib_client.is_ib_ready.is_set = MagicMock()
    ib_client._eclient.isConnected.return_value = True
    ib_client.is_ib_ready.is_set.return_value = True

    # Act
    watchdog_task = asyncio.create_task(connection_manager.run_watch_dog())
    await asyncio.sleep(1)
    watchdog_task.cancel()

    # Assert
    connection_manager._resume_client_if_degraded.assert_called_once()
    connection_manager._start_client_if_initialized_but_not_running.assert_called_once()
    connection_manager._handle_ib_is_not_ready.assert_not_called()
    connection_manager._monitor_and_reconnect_socket.assert_not_called()


@pytest.mark.asyncio
async def test_resume_client_if_degraded(ib_client):
    # Arrange

    # Act
    await ib_client.connection_manager.resume_client_if_degraded()

    # Assert
    assert ib_client.is_ready.set()


@pytest.mark.asyncio
async def test_initial_connectivity(ib_client):
    # Arrange
    ib_client._eclient.conn = MagicMock()
    ib_client._eclient.conn.isConnected.return_value = True

    test_messages = [
        b"15\x001\x00DU1234567\x00",
        b"9\x001\x00574\x00",
        b"15\x001\x00DU1234567\x00",
        b"9\x001\x001\x00",
        b"4\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00",
        b"4\x002\x00-1\x002106\x00HMDS data farm connection is OK:ushmds\x00\x00",
        b"4\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00",
    ]
    ib_client._eclient.conn.recvMsg = MagicMock(side_effect=test_messages)

    # Act
    with patch("ibapi.comm.read_msg") as mock_read_msg:
        mock_read_msg.side_effect = [(None, msg, b"") for msg in test_messages]

        task = asyncio.create_task(ib_client.run_tws_incoming_msg_reader())
        await asyncio.sleep(0.1)

        # Assert
        assert ib_client._internal_msg_queue.qsize() == len(test_messages)
        for msg in test_messages:
            assert await ib_client._internal_msg_queue.get() == msg

        # Assert
        assert "DU1234567" in ib_client.accounts()
        assert ib_client.next_order_id() > 0
        assert ib_client.is_ib_ready.is_set()

        # Clean up
        task.cancel()

    # Arrange
    await ib_client.is_running_async(10)
    data = b"\x00\x00\x00\x0f15\x001\x00DU1234567\x00\x00\x00\x00\x089\x001\x00117\x00\x00\x00\x0094\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm.nj\x00\x00\x00\x00\x0084\x002\x00-1\x002104\x00Market data farm connection is OK:usfuture\x00\x00\x00\x00\x0084\x002\x00-1\x002104\x00Market data farm connection is OK:cashfarm\x00\x00\x00\x00\x0054\x002\x00-1\x002104\x00Market data farm connection is OK:usopt\x00\x00\x00\x00\x0064\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00\x00\x00\x0064\x002\x00-1\x002106\x00HMDS data farm connection is OK:cashhmds\x00\x00\x00\x00\x0044\x002\x00-1\x002106\x00HMDS data farm connection is OK:ushmds\x00\x00\x00\x00\x0094\x002\x00-1\x002158\x00Sec-def data farm connection is OK:secdefil\x00\x00"  # noqa
    ib_client._eclient.conn.mock_response.append(data)

    # Act
    await ib_client.is_running_async()
