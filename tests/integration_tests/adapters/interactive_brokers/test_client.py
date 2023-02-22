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

from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from tests.integration_tests.adapters.interactive_brokers.base import InteractiveBrokersTestBase


class MockConnection:
    def __init__(self, host, port):
        self.host = host
        self.port = port
        self.socket = None
        self.wrapper = None
        self.mock_response = [b""]

    def connect(self):
        self.socket = MagicMock()
        self.mock_response = [b"\x00\x00\x00\x1a176\x0020230228 17:24:14 EST\x00"]

    def disconnect(self):
        self.socket = None
        if self.wrapper:
            self.wrapper.connectionClosed()

    def isConnected(self):
        return self.socket is not None

    def sendMsg(self, msg):
        return len(msg)

    def recvMsg(self):
        if not self.isConnected():
            return b""
        if self.mock_response:
            return self.mock_response.pop()
        else:
            return b""


@patch("nautilus_trader.adapters.interactive_brokers.client.client.Connection", MockConnection)
class TestInteractiveBrokersClient(InteractiveBrokersTestBase):
    def setup(self):
        super().setup()
        self.instrument = TestInstrumentProvider.aapl_equity()

        self.client: InteractiveBrokersClient = InteractiveBrokersClient(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            host="127.0.0.1",
            port=54321,
            client_id=12345,
        )
        assert isinstance(self.client, InteractiveBrokersClient)
        # self.client._client.conn.mock_response = b'\x00\x00\x00\x1a176\x0020230228 17:24:14 EST\x00'

    @pytest.mark.asyncio
    async def test_initial_connectivity(self):
        # Arrange
        await self.client.is_running_async(10)
        data = b"\x00\x00\x00\x0f15\x001\x00DU1234567\x00\x00\x00\x00\x089\x001\x00117\x00\x00\x00\x0094\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm.nj\x00\x00\x00\x00\x0084\x002\x00-1\x002104\x00Market data farm connection is OK:usfuture\x00\x00\x00\x00\x0084\x002\x00-1\x002104\x00Market data farm connection is OK:cashfarm\x00\x00\x00\x00\x0054\x002\x00-1\x002104\x00Market data farm connection is OK:usopt\x00\x00\x00\x00\x0064\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00\x00\x00\x0064\x002\x00-1\x002106\x00HMDS data farm connection is OK:cashhmds\x00\x00\x00\x00\x0044\x002\x00-1\x002106\x00HMDS data farm connection is OK:ushmds\x00\x00\x00\x00\x0094\x002\x00-1\x002158\x00Sec-def data farm connection is OK:secdefil\x00\x00"  # noqa
        self.client._client.conn.mock_response.append(data)

        # Act
        await self.client.is_running_async()

        # Assert
        assert "DU1234567" in self.client.accounts()
        assert self.client.next_order_id() > 0
        assert self.client.is_ib_ready.is_set()

    def test_ib_is_ready_by_next_valid_id(self):
        # Arrange
        self.client._accounts = ["DU12345"]
        self.client.is_ib_ready.clear()

        # Act
        self.client.nextValidId(1)

        # Assert
        assert self.client.is_ib_ready.is_set()

    def test_ib_is_ready_by_managed_accounts(self):
        # Arrange
        self.client._next_valid_order_id = 1
        self.client.is_ib_ready.clear()

        # Act
        self.client.managedAccounts("DU1234567")

        # Assert
        assert self.client.is_ib_ready.is_set()

    def test_ib_is_ready_by_data_probe(self):
        # Arrange
        self.client.is_ib_ready.clear()

        # Act
        self.client.historicalDataEnd(1, "", "")

        # Assert
        assert self.client.is_ib_ready.is_set()

    def test_ib_is_ready_by_notification_1101(self):
        # Arrange
        self.client.is_ib_ready.clear()

        # Act
        self.client.error(
            -1,
            1101,
            "Connectivity between IB and Trader Workstation has been restored",
        )

        # Assert
        assert self.client.is_ib_ready.is_set()

    def test_ib_is_ready_by_notification_1102(self):
        # Arrange
        self.client.is_ib_ready.clear()

        # Act
        self.client.error(
            -1,
            1102,
            "Connectivity between IB and Trader Workstation has been restored",
        )

        # Assert
        assert self.client.is_ib_ready.is_set()

    def test_ib_is_not_ready_by_error_10182(self):
        # Arrange
        req_id = 6
        self.client.is_ib_ready.set()
        self.client.subscriptions.add(req_id, "EUR.USD", self.client._client.reqHistoricalData, {})

        # Act
        self.client.error(req_id, 10182, "Failed to request live updates (disconnected).")

        # Assert
        assert not self.client.is_ib_ready.is_set()

    # #@pytest.mark.asyncio
    # def test_ib_is_not_ready_by_error_10189(self):
    #     # Arrange
    #     req_id = 6
    #     self.client.is_ib_ready.set()
    #     self.client.subscriptions.add(req_id, 'EUR.USD', self.client.subscribe_ticks, dict(instrument_id=self.instrument, contract=IBContract(conId=1234), tick_type='BidAsk'))  # noqa
    #
    #     # Act
    #     self.client.error(req_id, 10189, 'Failed to request tick-by-tick data.BidAsk tick-by-tick requests are not supported for EUR.USD.')  # noqa
    #
    #     # Assert
    #     assert not self.client.is_ib_ready.is_set()
