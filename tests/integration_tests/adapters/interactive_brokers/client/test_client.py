import asyncio
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import Mock
from unittest.mock import patch

import pytest

from nautilus_trader.test_kit.functions import eventually


@pytest.mark.asyncio
async def test_start(event_loop, ib_client):
    # Arrange
    ib_client.connect = AsyncMock()
    ib_client._eclient = MagicMock()
    ib_client._eclient.startApi = MagicMock(side_effect=ib_client._is_ib_connected.set)

    # Act
    await ib_client._start_async()

    # Assert
    assert ib_client._is_client_ready.is_set()


@pytest.mark.asyncio
async def test_start_waits_for_completed_handshake_before_start_api(ib_client):
    # Arrange
    ib_client._connect = AsyncMock()
    ib_client._eclient = MagicMock()
    ib_client._eclient.serverVersion.return_value = None
    ib_client._eclient.startApi = MagicMock()
    ib_client._stop = MagicMock()
    ib_client._max_connection_attempts = 1
    ib_client._indefinite_reconnect = False
    ib_client._reconnect_delay = 0

    # Act
    await ib_client._start_async()

    # Assert
    ib_client._eclient.startApi.assert_not_called()
    assert not ib_client._is_client_ready.is_set()


def test_start_tasks(ib_client):
    # Arrange
    ib_client._eclient = MagicMock()
    ib_client._tws_incoming_msg_reader_task = None
    ib_client._internal_msg_queue_task = None
    ib_client._connection_watchdog_task = None

    # Act
    ib_client._start_tws_incoming_msg_reader()
    ib_client._start_internal_msg_queue_processor()
    ib_client._start_connection_watchdog()

    # Assert
    # Tasks should be running if there's a (simulated) connection
    assert not ib_client._tws_incoming_msg_reader_task.done()
    assert not ib_client._internal_msg_queue_processor_task.done()
    assert not ib_client._connection_watchdog_task.done()


@pytest.mark.asyncio
async def test_stop(ib_client_running):
    # Arrange

    # Act
    ib_client_running.stop()
    await asyncio.sleep(0.1)

    # Assert
    assert ib_client_running.is_stopped
    assert ib_client_running._connection_watchdog_task.done()
    assert ib_client_running._tws_incoming_msg_reader_task.done()
    assert ib_client_running._internal_msg_queue_processor_task.done()
    assert not ib_client_running._is_client_ready.is_set()
    assert len(ib_client_running.registered_nautilus_clients) == 0


@pytest.mark.asyncio
async def test_reset(ib_client_running):
    # Arrange
    ib_client_running._start_async = AsyncMock()
    ib_client_running._stop_async = AsyncMock()

    # Act
    ib_client_running._reset()
    await asyncio.sleep(0.1)

    # Assert
    ib_client_running._start_async.assert_awaited_once()
    ib_client_running._stop_async.assert_awaited_once()


@pytest.mark.asyncio
async def test_resume(ib_client_running):
    # Arrange, Act, Assert
    ib_client_running._resubscribe_all = MagicMock()

    # Act
    ib_client_running._resume()
    await asyncio.sleep(0.1)

    # Assert
    ib_client_running._resubscribe_all.assert_called_once()


def test_degrade(ib_client_running):
    # Arrange

    # Act
    ib_client_running._degrade()

    # Assert
    assert not ib_client_running._is_client_ready.is_set()
    assert len(ib_client_running._account_ids) == 0


@pytest.mark.asyncio
async def test_create_task(ib_client):
    # Arrange
    async def sample_coro():
        return "completed"

    # Act
    task = ib_client._create_task(sample_coro(), log_msg="sample task")

    # Assert
    assert not task.done()
    await task
    assert task.done()
    assert task.result() == "completed"


def test_subscribe_event(ib_client):
    # Arrange
    def sample_handler():
        pass

    # Act
    ib_client.subscribe_event("test_event", sample_handler)

    # Assert
    assert "test_event" in ib_client._event_subscriptions
    assert ib_client._event_subscriptions["test_event"] == sample_handler


def test_unsubscribe_event(ib_client):
    # Arrange
    ib_client.subscribe_event("test_event", lambda handler: handler)

    # Act
    ib_client.unsubscribe_event("test_event")

    # Assert
    assert "test_event" not in ib_client._event_subscriptions


def test_next_req_id(ib_client):
    # Arrange
    first_id = ib_client._next_req_id()

    # Act
    second_id = ib_client._next_req_id()

    # Assert
    assert first_id + 1 == second_id


@pytest.mark.asyncio
async def test_wait_until_ready(ib_client_running):
    # Arrange

    # Act
    await ib_client_running.wait_until_ready()

    # Assert
    assert True


@pytest.mark.asyncio
async def test_run_connection_watchdog_reconnect(ib_client):
    # Arrange
    ib_client._is_ib_connected.clear()
    ib_client._eclient = MagicMock()
    ib_client._eclient.isConnected.return_value = False
    ib_client._handle_disconnection = AsyncMock(side_effect=asyncio.CancelledError)

    # Act
    await ib_client._run_connection_watchdog()

    # Assert
    ib_client._handle_disconnection.assert_called()


@pytest.mark.asyncio
async def test_run_tws_incoming_msg_reader(ib_client):
    # Arrange
    ib_client._eclient.conn = Mock()

    test_messages = [b"test message 1", b"test message 2"]
    ib_client._eclient.conn.recvMsg = MagicMock(side_effect=test_messages)

    with patch("ibapi.comm.read_msg", side_effect=[(None, msg, b"") for msg in test_messages]):
        # Act
        ib_client._start_tws_incoming_msg_reader()
        await eventually(lambda: ib_client._internal_msg_queue.qsize() == len(test_messages))

    # Assert
    for msg in test_messages:
        assert await ib_client._internal_msg_queue.get() == msg


@pytest.mark.asyncio
async def test_run_internal_msg_queue(ib_client_running):
    # Arrange
    test_messages = [b"test message 1", b"test message 2"]
    for msg in test_messages:
        ib_client_running._internal_msg_queue.put_nowait(msg)
    ib_client_running._process_message = AsyncMock()

    # Act

    # Assert
    await eventually(lambda: ib_client_running._process_message.call_count == len(test_messages))
    assert ib_client_running._internal_msg_queue.qsize() == 0
