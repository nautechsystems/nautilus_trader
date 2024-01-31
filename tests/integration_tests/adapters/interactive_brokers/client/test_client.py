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


def test_start(ib_client):
    # Arrange, Act
    ib_client._start()

    # Assert
    assert ib_client._is_client_ready.is_set()


def test_start_client_tasks_and_tws_api(ib_client):
    # Arrange
    ib_client._tws_incoming_msg_reader_task = None
    ib_client._internal_msg_queue_task = None
    ib_client._eclient.startApi = Mock()

    # Act
    ib_client._start_client_tasks_and_tws_api()

    # Assert
    assert ib_client._tws_incoming_msg_reader_task
    assert ib_client._internal_msg_queue_task
    assert ib_client._eclient.startApi.called


def test_stop(ib_client):
    # Arrange
    ib_client._start_client_tasks_and_tws_api()
    ib_client._eclient.disconnect = Mock()

    # Act
    ib_client._stop()

    # Assert
    assert ib_client._watch_dog_task.cancel()
    assert ib_client._tws_incoming_msg_reader_task.cancel()
    assert ib_client._internal_msg_queue_task.cancel()
    assert ib_client._eclient.disconnect.called
    assert not ib_client._is_client_ready.is_set()


def test_reset(ib_client):
    # Arrange
    ib_client._stop = Mock()
    ib_client._eclient.reset = Mock()

    # Act
    ib_client._reset()

    # Assert
    assert ib_client._stop.called
    assert ib_client._eclient.reset.called
    assert ib_client._watch_dog_task


def test_resume(ib_client):
    # Arrange
    ib_client._is_client_ready.clear()
    ib_client._connection_attempt_counter = 1

    # Act
    ib_client._resume()

    # Assert
    assert ib_client._is_client_ready.is_set()
    assert ib_client._connection_attempt_counter == 0


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
async def test_wait_until_ready(ib_client):
    # Arrange
    ib_client._is_client_ready = Mock()
    ib_client._is_client_ready.is_set.return_value = True

    # Act
    await ib_client.wait_until_ready()

    # Assert
    # Assert wait was not called since is_client_ready is already set
    ib_client._is_client_ready.wait.assert_not_called()


@pytest.mark.asyncio
async def test_run_watch_dog_reconnect(ib_client):
    # Arrange
    ib_client._eclient = MagicMock()
    ib_client._eclient.isConnected.return_value = False
    ib_client._reconnect = AsyncMock(side_effect=asyncio.CancelledError)

    # Act
    await ib_client._run_watch_dog()

    # Assert
    ib_client._reconnect.assert_called()


@pytest.mark.asyncio
async def test_run_watch_dog_probe(ib_client):
    # Arrange
    ib_client._eclient = MagicMock()
    ib_client._eclient.isConnected.return_value = True
    ib_client._is_ib_ready.clear()
    ib_client._probe_for_connectivity = AsyncMock(side_effect=asyncio.CancelledError)

    # Act
    await ib_client._run_watch_dog()

    # Assert
    ib_client._probe_for_connectivity.assert_called()


@pytest.mark.asyncio
async def test_run_tws_incoming_msg_reader(ib_client):
    # Arrange
    ib_client._eclient.conn = Mock()

    test_messages = [b"test message 1", b"test message 2"]
    ib_client._eclient.conn.recvMsg = MagicMock(side_effect=test_messages)

    with patch("ibapi.comm.read_msg", side_effect=[(None, msg, b"") for msg in test_messages]):
        # Act
        ib_client._tws_incoming_msg_reader_task = ib_client._create_task(
            ib_client._run_tws_incoming_msg_reader(),
        )
        await asyncio.sleep(0.1)

    # Assert
    assert ib_client._internal_msg_queue.qsize() == len(test_messages)
    for msg in test_messages:
        assert await ib_client._internal_msg_queue.get() == msg


@pytest.mark.asyncio
async def test_run_internal_msg_queue(ib_client):
    # Arrange
    test_messages = [b"test message 1", b"test message 2"]
    for msg in test_messages:
        ib_client._internal_msg_queue.put_nowait(msg)
    ib_client._process_message = Mock()

    # Act
    ib_client._internal_msg_queue_task = ib_client._create_task(
        ib_client._run_internal_msg_queue(),
    )
    await asyncio.sleep(0.1)

    # Assert
    assert ib_client._process_message.call_count == len(test_messages)
    assert ib_client._internal_msg_queue.qsize() == 0
