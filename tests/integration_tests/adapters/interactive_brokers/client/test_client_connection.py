import asyncio
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import ibapi
import pytest
from ibapi import decoder


@pytest.mark.asyncio
async def test_establish_socket_connection(ib_client):
    # Arrange
    ib_client._eclient.startApi = MagicMock()
    ib_client._initialize_connection = AsyncMock()
    ib_client._connect_socket = AsyncMock()
    ib_client._send_version_info = AsyncMock()
    ib_client._receive_server_info = AsyncMock()
    ib_client._setup_client = AsyncMock()
    ib_client._handle_connection_error = AsyncMock()

    # Act
    await ib_client._establish_socket_connection()

    # Assert
    ib_client._setup_client.assert_called_once()
    ib_client._watch_dog_task.cancel()


@pytest.mark.asyncio
async def test_run_watch_dog(ib_client):
    # Arrange
    ib_client._resume_client_if_degraded = AsyncMock()
    ib_client._start_client_if_initialized_but_not_running = AsyncMock()
    ib_client._handle_ib_is_not_ready = AsyncMock()
    ib_client._monitor_and_reconnect_socket = AsyncMock()
    ib_client._eclient.isConnected = MagicMock()
    ib_client._is_ib_ready.is_set = MagicMock()
    ib_client._eclient.isConnected.return_value = True
    ib_client._is_ib_ready.is_set.return_value = True

    # Act
    watchdog_task = asyncio.create_task(ib_client._run_watch_dog())
    await asyncio.sleep(1)
    watchdog_task.cancel()

    # Assert
    ib_client._resume_client_if_degraded.assert_called_once()
    ib_client._start_client_if_initialized_but_not_running.assert_called_once()
    ib_client._handle_ib_is_not_ready.assert_not_called()
    ib_client._monitor_and_reconnect_socket.assert_not_called()

    ib_client._watch_dog_task.cancel()


@pytest.mark.asyncio
async def test_resume_client_if_degraded(ib_client):
    # Arrange

    # Act
    await ib_client._resume_client_if_degraded()

    # Assert
    assert ib_client._is_ready.is_set()

    ib_client._watch_dog_task.cancel()


@pytest.mark.asyncio
async def test_initial_connectivity(ib_client):
    # Arrange
    ib_client._eclient.conn = MagicMock()
    ib_client._eclient.conn.isConnected.return_value = True
    ib_client._eclient.serverVersion = MagicMock(return_value=179)
    ib_client._eclient.decoder = decoder.Decoder(
        wrapper=ib_client._eclient.wrapper,
        serverVersion=ib_client._eclient.serverVersion(),
    )

    test_messages = [
        b"15\x001\x00DU1234567\x00",
        b"9\x001\x00574\x00",
        b"15\x001\x00DU1234567\x00",
        b"9\x001\x001\x00",
        b"4\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00",
        b"4\x002\x00-1\x002106\x00HMDS data farm connection is OK:ushmds\x00\x00",
        b"4\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00",
    ]
    ibapi.comm.read_msg = MagicMock(side_effect=[(None, msg, b"") for msg in test_messages])

    # Act
    ib_client._tws_incoming_msg_reader_task = asyncio.create_task(
        ib_client._run_tws_incoming_msg_reader(),
    )
    ib_client._internal_msg_queue_task = asyncio.create_task(ib_client._run_internal_msg_queue())
    await asyncio.sleep(0.1)

    # Assert
    assert "DU1234567" in ib_client.accounts()

    # Clean up
    ib_client._tws_incoming_msg_reader_task.cancel()
    ib_client._internal_msg_queue_task.cancel()
    ib_client._watch_dog_task.cancel()
