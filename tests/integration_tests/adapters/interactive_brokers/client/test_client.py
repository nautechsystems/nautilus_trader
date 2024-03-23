# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import Mock
from unittest.mock import patch

import pytest

from nautilus_trader.test_kit.functions import ensure_all_tasks_completed
from nautilus_trader.test_kit.functions import eventually


def test_start(ib_client):
    # Arrange
    ib_client._is_ib_connected.set()
    ib_client._connect = AsyncMock()
    ib_client._eclient = MagicMock()

    # Act
    ib_client.start()

    # Assert
    assert ib_client._is_client_ready.is_set()


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


def test_stop(ib_client):
    # Arrange
    ib_client._is_ib_connected.set()
    ib_client._connect = AsyncMock()
    ib_client._eclient = MagicMock()
    ib_client.start()

    # Act
    ib_client.stop()
    ensure_all_tasks_completed()

    # Assert
    assert ib_client.is_stopped
    assert ib_client._connection_watchdog_task.done()
    assert ib_client._tws_incoming_msg_reader_task.done()
    assert ib_client._internal_msg_queue_processor_task.done()
    assert not ib_client._is_client_ready.is_set()
    assert len(ib_client.registered_nautilus_clients) == 0


def test_reset(ib_client):
    # Arrange
    ib_client._stop = Mock()
    ib_client._start = Mock()

    # Act
    ib_client.reset()

    # Assert
    assert ib_client._stop.called
    assert ib_client._start.called


def test_resume(ib_client_running):
    # Arrange, Act, Assert
    ib_client_running._degrade()

    # Act
    ib_client_running._resume()

    # Assert
    assert ib_client_running._is_client_ready.is_set()


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
    assert "test_event" not in ib_client._event_subscriptions.keys()


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
