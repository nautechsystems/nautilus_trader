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
from ibapi.client import EClient

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client.account import InteractiveBrokersAccountManager
from nautilus_trader.adapters.interactive_brokers.client.connection import InteractiveBrokersConnectionManager
from nautilus_trader.adapters.interactive_brokers.client.contract import InteractiveBrokersContractManager
from nautilus_trader.adapters.interactive_brokers.client.error import InteractiveBrokersErrorHandler
from nautilus_trader.adapters.interactive_brokers.client.market_data import InteractiveBrokersMarketDataManager
from nautilus_trader.adapters.interactive_brokers.client.order import InteractiveBrokersOrderManager


# fmt: on

# pytestmark = pytest.mark.skip(reason="Skip due currently incomplete")


def test_constructor_initializes_properties(ib_client):
    # Assertions to verify initial state of various components
    assert isinstance(ib_client._eclient, EClient)
    assert isinstance(ib_client._internal_msg_queue, asyncio.Queue)
    assert ib_client._connection_attempt_counter == 0
    assert isinstance(
        ib_client.connection_manager,
        InteractiveBrokersConnectionManager,
    )
    assert isinstance(ib_client.account_manager, InteractiveBrokersAccountManager)
    assert isinstance(
        ib_client.market_data_manager,
        InteractiveBrokersMarketDataManager,
    )
    assert isinstance(
        ib_client.contract_manager,
        InteractiveBrokersContractManager,
    )
    assert isinstance(ib_client.order_manager, InteractiveBrokersOrderManager)
    assert isinstance(ib_client._error_handler, InteractiveBrokersErrorHandler)
    assert isinstance(ib_client._watch_dog_task, asyncio.Task)
    assert ib_client.tws_incoming_msg_reader_task is None
    assert ib_client.internal_msg_queue_task is None
    assert not ib_client.is_ready.is_set()
    assert not ib_client.is_ib_ready.is_set()
    assert ib_client.registered_nautilus_clients == set()
    assert ib_client.event_subscriptions == {}

    # Verify initial request ID sequence
    assert ib_client._request_id_seq == 10000


@pytest.mark.asyncio
async def test_create_task(ib_client):
    async def sample_coro():
        return "completed"

    task = ib_client.create_task(sample_coro(), log_msg="sample task")
    assert not task.done()
    await task
    assert task.done()
    assert task.result() == "completed"


def test_subscribe_and_unsubscribe_event(ib_client):
    def sample_handler():
        pass

    ib_client.subscribe_event("test_event", sample_handler)
    assert "test_event" in ib_client.event_subscriptions
    assert ib_client.event_subscriptions["test_event"] == sample_handler

    ib_client.unsubscribe_event("test_event")
    assert "test_event" not in ib_client.event_subscriptions


def test_next_req_id(ib_client):
    first_id = ib_client.next_req_id()
    second_id = ib_client.next_req_id()
    assert first_id + 1 == second_id


def test_start(ib_client):
    ib_client._start()
    assert ib_client.is_ready.is_set()


@pytest.mark.asyncio
async def test_stop(ib_client):
    # Mocking the necessary attributes
    ib_client._watch_dog_task = MagicMock()
    ib_client.tws_incoming_msg_reader_task = MagicMock()
    ib_client.internal_msg_queue_task = MagicMock()
    ib_client._eclient.disconnect = MagicMock()

    ib_client._stop()

    # Verify that the tasks were cancelled
    assert ib_client._watch_dog_task.cancel.called
    assert ib_client.tws_incoming_msg_reader_task.cancel.called
    assert ib_client.internal_msg_queue_task.cancel.called

    # Verify that the client was disconnected
    assert ib_client._eclient.disconnect.called

    # Verify that is_ready is cleared
    assert not ib_client.is_ready.is_set()


@pytest.mark.asyncio
async def test_reset(ib_client):
    # Mocking the necessary methods
    ib_client._stop = MagicMock()
    ib_client._eclient.reset = MagicMock()
    ib_client.create_task = MagicMock()

    ib_client._reset()

    # Verify that stop and reset were called
    assert ib_client._stop.called
    assert ib_client._eclient.reset.called

    # Verify that the watch dog task was created
    assert ib_client.create_task.called


def test_resume(ib_client):
    ib_client.resume()

    # Verify that is_ready is set
    assert ib_client.is_ready.is_set()

    # Verify that the connection attempt counter is reset
    assert ib_client._connection_attempt_counter == 0


@pytest.mark.asyncio
async def test_is_running_async_ready(ib_client):
    # Mock is_ready to simulate the event being set
    with patch.object(ib_client, "is_ready", new=MagicMock()) as mock_is_ready:
        mock_is_ready.is_set.return_value = True
        await ib_client.is_running_async()
        mock_is_ready.wait.assert_not_called()  # Assert wait was not called since is_ready is already set


@patch("nautilus_trader.adapters.interactive_brokers.client._eclient.comm.read_msg")
def test_run_tws_incoming_msg_reader(ib_client, mock_read_msg):
    # Mock the data received from the connection
    mock_data = b"mock_data"
    ib_client.loop.run_in_executor.return_value = mock_data

    # Mock the message and remaining buffer returned by read_msg
    mock_msg = b"mock_msg"
    mock_buf = b""
    mock_read_msg.return_value = (len(mock_msg), mock_msg, mock_buf)

    # Run the method until it has processed one message
    ib_client.loop.run_until_complete(
        ib_client.run_tws_incoming_msg_reader(),
    )

    # Check that the message was added to the internal message queue
    ib_client._internal_msg_queue.put_nowait.assert_called_once_with(mock_msg)


@patch("nautilus_trader.adapters.interactive_brokers.client.client.comm.read_msg")
def test_run_tws_incoming_msg_reader_add_to_queue(ib_client, mock_read_msg):
    # Mock the data received from the connection
    mock_data = b"mock_data"
    ib_client.loop.run_in_executor.return_value = mock_data

    # Mock the message and remaining buffer returned by read_msg
    mock_msg = b"mock_msg"
    # mock_buf = b""
    # read_msg.return_value = (len(mock_msg), mock_msg, mock_buf)

    # Run the method until it has processed one message
    ib_client.loop.run_until_complete(
        ib_client.run_tws_incoming_msg_reader(),
    )

    # Check that the message was added to the internal message queue
    assert ib_client._internal_msg_queue.get_nowait() == mock_msg


@pytest.mark.asyncio
async def test_initial_connectivity(ib_client):
    # Arrange
    await ib_client.is_running_async(10)
    data = b"\x00\x00\x00\x0f15\x001\x00DU1234567\x00\x00\x00\x00\x089\x001\x00117\x00\x00\x00\x0094\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm.nj\x00\x00\x00\x00\x0084\x002\x00-1\x002104\x00Market data farm connection is OK:usfuture\x00\x00\x00\x00\x0084\x002\x00-1\x002104\x00Market data farm connection is OK:cashfarm\x00\x00\x00\x00\x0054\x002\x00-1\x002104\x00Market data farm connection is OK:usopt\x00\x00\x00\x00\x0064\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00\x00\x00\x0064\x002\x00-1\x002106\x00HMDS data farm connection is OK:cashhmds\x00\x00\x00\x00\x0044\x002\x00-1\x002106\x00HMDS data farm connection is OK:ushmds\x00\x00\x00\x00\x0094\x002\x00-1\x002158\x00Sec-def data farm connection is OK:secdefil\x00\x00"  # noqa
    ib_client._eclient.conn.mock_response.append(data)

    # Act
    await ib_client.is_running_async()

    # Assert
    assert "DU1234567" in ib_client.accounts()
    assert ib_client.next_order_id() > 0
    assert ib_client.is_ib_ready.is_set()


def test_ib_is_ready_by_next_valid_id(ib_client):
    # Arrange
    ib_client._accounts = ["DU12345"]
    ib_client.is_ib_ready.clear()

    # Act
    ib_client.nextValidId(1)

    # Assert
    assert ib_client.is_ib_ready.is_set()


def test_ib_is_ready_by_managed_accounts(ib_client):
    # Arrange
    ib_client.next_valid_order_id = 1
    ib_client.is_ib_ready.clear()

    # Act
    ib_client.managedAccounts("DU1234567")

    # Assert
    assert ib_client.is_ib_ready.is_set()


def test_ib_is_ready_by_data_probe(ib_client):
    # Arrange
    ib_client.is_ib_ready.clear()

    # Act
    ib_client.historicalDataEnd(1, "", "")

    # Assert
    assert ib_client.is_ib_ready.is_set()


# class MockConnection:
#     def __init__(self, host, port):
#         self.host = host
#         self.port = port
#         self.socket = None
#         self.wrapper = None
#         self.mock_response = [b""]
#
#     def connect(self):
#         self.socket = MagicMock()
#         self.mock_response = [b"\x00\x00\x00\x1a176\x0020230228 17:24:14 EST\x00"]
#
#     def disconnect(self):
#         self.socket = None
#         if self.wrapper:
#             self.wrapper.connectionClosed()
#
#     def isConnected(self):
#         return self.socket is not None
#
#     def sendMsg(self, msg):
#         return len(msg)
#
#     def recvMsg(self):
#         if not self.isConnected():
#             return b""
#         if self.mock_response:
#             return self.mock_response.pop()
#         else:
#             return b""
#
#
