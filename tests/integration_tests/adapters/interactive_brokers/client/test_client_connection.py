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

from unittest.mock import AsyncMock
from unittest.mock import Mock
from unittest.mock import patch

import pytest
from ibapi.client import EClient


@pytest.mark.asyncio
async def testconnect(ib_client):
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
    await ib_client.connect()

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

        # Assert
        mock_connection_instance.connect.assert_called_once()
