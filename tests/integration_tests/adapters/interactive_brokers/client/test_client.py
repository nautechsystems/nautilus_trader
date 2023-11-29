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
from nautilus_trader.adapters.interactive_brokers.client.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.client.connection import InteractiveBrokersConnectionManager
from nautilus_trader.adapters.interactive_brokers.client.error import InteractiveBrokersErrorHandler
from nautilus_trader.adapters.interactive_brokers.client.market_data import InteractiveBrokersMarketDataManager
from nautilus_trader.adapters.interactive_brokers.client.order import InteractiveBrokersOrderManager
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.msgbus import MessageBus
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


# fmt: on

# pytestmark = pytest.mark.skip(reason="Skip due currently incomplete")


class TestInteractiveBrokersClient:
    def setup(self):
        self.loop = asyncio.get_event_loop()
        asyncio.set_event_loop(self.loop)
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.logger = Logger(self.clock, bypass=True)

        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()

        self.client = InteractiveBrokersClient(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            host="127.0.0.1",
            port=7497,
            client_id=1,
        )

    def teardown(self):
        self.client._stop()
        pending = asyncio.all_tasks(self.loop)
        for task in pending:
            task.cancel()

        if pending:
            self.loop.run_until_complete(asyncio.gather(*pending, return_exceptions=True))

    def test_constructor_initializes_properties(self):
        # Assertions to verify initial state of various components
        assert isinstance(self.client._eclient, EClient)
        assert isinstance(self.client._internal_msg_queue, asyncio.Queue)
        assert self.client._connection_attempt_counter == 0
        assert isinstance(self.client.connection_manager, InteractiveBrokersConnectionManager)
        assert isinstance(self.client.account_manager, InteractiveBrokersAccountManager)
        assert isinstance(self.client.market_data_manager, InteractiveBrokersMarketDataManager)
        assert isinstance(self.client.order_manager, InteractiveBrokersOrderManager)
        assert isinstance(self.client._error_handler, InteractiveBrokersErrorHandler)
        assert isinstance(self.client._watch_dog_task, asyncio.Task)
        assert self.client.tws_incoming_msg_reader_task is None
        assert self.client.internal_msg_queue_task is None
        assert not self.client.is_ready.is_set()
        assert not self.client.is_ib_ready.is_set()
        assert self.client.registered_nautilus_clients == set()
        assert self.client.event_subscriptions == {}

        # Verify initial request ID sequence
        assert self.client._request_id_seq == 10000

    @pytest.mark.asyncio
    async def test_create_task(self):
        async def sample_coro():
            return "completed"

        task = self.client.create_task(sample_coro(), log_msg="sample task")
        assert not task.done()
        await task
        assert task.done()
        assert task.result() == "completed"

    def test_subscribe_and_unsubscribe_event(self):
        def sample_handler():
            pass

        self.client.subscribe_event("test_event", sample_handler)
        assert "test_event" in self.client.event_subscriptions
        assert self.client.event_subscriptions["test_event"] == sample_handler

        self.client.unsubscribe_event("test_event")
        assert "test_event" not in self.client.event_subscriptions

    def test_next_req_id(self):
        first_id = self.client._next_req_id()
        second_id = self.client._next_req_id()
        assert first_id + 1 == second_id

    def test_start(self):
        self.client._start()
        assert self.client.is_ready.is_set()

    @pytest.mark.asyncio
    async def test_stop(self):
        # Mocking the necessary attributes
        self.client._watch_dog_task = MagicMock()
        self.client.tws_incoming_msg_reader_task = MagicMock()
        self.client.internal_msg_queue_task = MagicMock()
        self.client._eclient.disconnect = MagicMock()

        self.client._stop()

        # Verify that the tasks were cancelled
        assert self.client._watch_dog_task.cancel.called
        assert self.client.tws_incoming_msg_reader_task.cancel.called
        assert self.client.internal_msg_queue_task.cancel.called

        # Verify that the client was disconnected
        assert self.client._eclient.disconnect.called

        # Verify that is_ready is cleared
        assert not self.client.is_ready.is_set()

    @pytest.mark.asyncio
    async def test_reset(self):
        # Mocking the necessary methods
        self.client._stop = MagicMock()
        self.client._eclient.reset = MagicMock()
        self.client.create_task = MagicMock()

        self.client._reset()

        # Verify that stop and reset were called
        assert self.client._stop.called
        assert self.client._eclient.reset.called

        # Verify that the watch dog task was created
        assert self.client.create_task.called

    def test_resume(self):
        self.client._resume()

        # Verify that is_ready is set
        assert self.client.is_ready.is_set()

        # Verify that the connection attempt counter is reset
        assert self.client._connection_attempt_counter == 0

    @pytest.mark.asyncio
    async def test_is_running_async_ready(self):
        # Mock is_ready to simulate the event being set
        with patch.object(self.client, "is_ready", new=MagicMock()) as mock_is_ready:
            mock_is_ready.is_set.return_value = True
            await self.client.is_running_async()
            mock_is_ready.wait.assert_not_called()  # Assert wait was not called since is_ready is already set

    @patch("nautilus_trader.adapters.interactive_brokers.client.client.comm.read_msg")
    def test_run_tws_incoming_msg_reader(self, mock_read_msg):
        # Mock the data received from the connection
        mock_data = b"mock_data"
        self.mock_client.loop.run_in_executor.return_value = mock_data

        # Mock the message and remaining buffer returned by read_msg
        mock_msg = b"mock_msg"
        mock_buf = b""
        mock_read_msg.return_value = (len(mock_msg), mock_msg, mock_buf)

        # Run the method until it has processed one message
        self.loop.run_until_complete(self.mock_client.run_tws_incoming_msg_reader())

        # Check that the message was added to the internal message queue
        self.mock_client._internal_msg_queue.put_nowait.assert_called_once_with(mock_msg)

    @patch("nautilus_trader.adapters.interactive_brokers.client.client.comm.read_msg")
    def test_run_tws_incoming_msg_reader_add_to_queue(self, mock_read_msg):
        # Mock the data received from the connection
        mock_data = b"mock_data"
        self.mock_client.loop.run_in_executor.return_value = mock_data

        # Mock the message and remaining buffer returned by read_msg
        mock_msg = b"mock_msg"
        mock_buf = b""
        mock_read_msg.return_value = (len(mock_msg), mock_msg, mock_buf)

        # Run the method until it has processed one message
        self.loop.run_until_complete(self.mock_client.run_tws_incoming_msg_reader())

        # Check that the message was added to the internal message queue
        assert self.mock_client._internal_msg_queue.get_nowait() == mock_msg


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
# @patch("nautilus_trader.adapters.interactive_brokers.client.client.Connection", MockConnection)
# class TestInteractiveBrokersClient(InteractiveBrokersTestBase):
#     def setup(self):
#         super().setup()
#         self.instrument = TestInstrumentProvider.aapl_equity()
#
#         self.client: InteractiveBrokersClient = InteractiveBrokersClient(
#             loop=self.loop,
#             msgbus=self.msgbus,
#             cache=self.cache,
#             clock=self.clock,
#             logger=self.logger,
#             host="127.0.0.1",
#             port=54321,
#             client_id=12345,
#         )
#         assert isinstance(self.client, InteractiveBrokersClient)
#         # self.client._client.conn.mock_response = b'\x00\x00\x00\x1a176\x0020230228 17:24:14 EST\x00'
#
#     @pytest.mark.asyncio
#     async def test_initial_connectivity(self):
#         # Arrange
#         await self.client.is_running_async(10)
#         data = b"\x00\x00\x00\x0f15\x001\x00DU1234567\x00\x00\x00\x00\x089\x001\x00117\x00\x00\x00\x0094\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm.nj\x00\x00\x00\x00\x0084\x002\x00-1\x002104\x00Market data farm connection is OK:usfuture\x00\x00\x00\x00\x0084\x002\x00-1\x002104\x00Market data farm connection is OK:cashfarm\x00\x00\x00\x00\x0054\x002\x00-1\x002104\x00Market data farm connection is OK:usopt\x00\x00\x00\x00\x0064\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00\x00\x00\x0064\x002\x00-1\x002106\x00HMDS data farm connection is OK:cashhmds\x00\x00\x00\x00\x0044\x002\x00-1\x002106\x00HMDS data farm connection is OK:ushmds\x00\x00\x00\x00\x0094\x002\x00-1\x002158\x00Sec-def data farm connection is OK:secdefil\x00\x00"  # noqa
#         self.client._client.conn.mock_response.append(data)
#
#         # Act
#         await self.client.is_running_async()
#
#         # Assert
#         assert "DU1234567" in self.client.accounts()
#         assert self.client.next_order_id() > 0
#         assert self.client.is_ib_ready.is_set()
#
#     def test_ib_is_ready_by_next_valid_id(self):
#         # Arrange
#         self.client._accounts = ["DU12345"]
#         self.client.is_ib_ready.clear()
#
#         # Act
#         self.client.nextValidId(1)
#
#         # Assert
#         assert self.client.is_ib_ready.is_set()
#
#     def test_ib_is_ready_by_managed_accounts(self):
#         # Arrange
#         self.client.next_valid_order_id = 1
#         self.client.is_ib_ready.clear()
#
#         # Act
#         self.client.managedAccounts("DU1234567")
#
#         # Assert
#         assert self.client.is_ib_ready.is_set()
#
#     def test_ib_is_ready_by_data_probe(self):
#         # Arrange
#         self.client.is_ib_ready.clear()
#
#         # Act
#         self.client.historicalDataEnd(1, "", "")
#
#         # Assert
#         assert self.client.is_ib_ready.is_set()
#
#     def test_ib_is_ready_by_notification_1101(self):
#         # Arrange
#         self.client.is_ib_ready.clear()
#
#         # Act
#         self.client.error(
#             -1,
#             1101,
#             "Connectivity between IB and Trader Workstation has been restored",
#         )
#
#         # Assert
#         assert self.client.is_ib_ready.is_set()
#
#     def test_ib_is_ready_by_notification_1102(self):
#         # Arrange
#         self.client.is_ib_ready.clear()
#
#         # Act
#         self.client.error(
#             -1,
#             1102,
#             "Connectivity between IB and Trader Workstation has been restored",
#         )
#
#         # Assert
#         assert self.client.is_ib_ready.is_set()
#
#     def test_ib_is_not_ready_by_error_10182(self):
#         # Arrange
#         req_id = 6
#         self.client.is_ib_ready.set()
#         self.client.subscriptions.add(req_id, "EUR.USD", self.client._client.reqHistoricalData, {})
#
#         # Act
#         self.client.error(req_id, 10182, "Failed to request live updates (disconnected).")
#
#         # Assert
#         assert not self.client.is_ib_ready.is_set()
#
#     # #@pytest.mark.asyncio
#     # def test_ib_is_not_ready_by_error_10189(self):
#     #     # Arrange
#     #     req_id = 6
#     #     self.client.is_ib_ready.set()
#     #     self.client.subscriptions.add(req_id, 'EUR.USD', self.client.subscribe_ticks, dict(instrument_id=self.instrument, contract=IBContract(conId=1234), tick_type='BidAsk'))  # noqa
#     #
#     #     # Act
#     #     self.client.error(req_id, 10189, 'Failed to request tick-by-tick data.BidAsk tick-by-tick requests are not supported for EUR.USD.')
#     #
#     #     # Assert
#     #     assert not self.client.is_ib_ready.is_set()
