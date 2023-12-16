# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest


def test_start(ib_client):
    # Arrange, Act
    ib_client._start()

    # Assert
    assert ib_client.is_ready.is_set()


@pytest.mark.asyncio
async def test_stop(ib_client):
    # Arrange
    ib_client._watch_dog_task = MagicMock()
    ib_client.tws_incoming_msg_reader_task = MagicMock()
    ib_client.internal_msg_queue_task = MagicMock()
    ib_client._eclient.disconnect = MagicMock()

    # Act
    ib_client._stop()

    # Assert
    assert ib_client._watch_dog_task.cancel.called
    assert ib_client.tws_incoming_msg_reader_task.cancel.called
    assert ib_client.internal_msg_queue_task.cancel.called
    assert ib_client._eclient.disconnect.called
    assert not ib_client.is_ready.is_set()


@pytest.mark.asyncio
async def test_reset(ib_client):
    # Arrange
    ib_client._stop = MagicMock()
    ib_client._eclient.reset = MagicMock()
    ib_client._create_task = MagicMock()

    # Act
    ib_client._reset()

    # Assert
    assert ib_client._stop.called
    assert ib_client._eclient.reset.called
    assert ib_client._create_task.called


def test_resume(ib_client):
    # Arrange, Act
    ib_client._resume()

    # Assert
    assert ib_client.is_ready.is_set()
    assert ib_client._connection_attempt_counter == 0


@pytest.mark.asyncio
async def test_create_task(ib_client):
    # Arrange
    async def sample_coro():
        return "completed"

    # Act
    task = ib_client.create_task(sample_coro(), log_msg="sample task")

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
    assert "test_event" in ib_client.event_subscriptions
    assert ib_client.event_subscriptions["test_event"] == sample_handler


def test_unsubscribe_event(ib_client):
    # Arrange
    ib_client.subscribe_event("test_event", lambda handler: handler)

    # Arrange, Act
    ib_client.unsubscribe_event("test_event")

    # Assert
    assert "test_event" not in ib_client.event_subscriptions.keys()


def test_next_req_id(ib_client):
    # Arrange
    first_id = ib_client.next_req_id()

    # Act
    second_id = ib_client.next_req_id()

    # Assert
    assert first_id + 1 == second_id


@pytest.mark.asyncio
async def test_is_running_async_ready(ib_client):
    # Arrange
    ib_client.is_ready = MagicMock()
    ib_client.is_ready.return_value = True
    ib_client.is_set = MagicMock()
    ib_client.is_set.return_value = True

    # Act
    await ib_client.is_running_async()

    # Assert
    # Assert wait was not called since is_ready is already set
    ib_client.is_ready.wait.assert_not_called()


@pytest.mark.asyncio
async def test_run_tws_incoming_msg_reader(ib_client):
    # Arrange
    ib_client._eclient.conn = MagicMock()
    ib_client._eclient.conn.isConnected.return_value = True

    test_messages = [b"test message 1", b"test message 2"]
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

        # Clean up
        task.cancel()
