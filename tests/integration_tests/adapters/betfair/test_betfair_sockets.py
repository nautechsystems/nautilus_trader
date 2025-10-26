# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import platform
import sys
from collections.abc import Callable

import msgspec
import pytest
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient
from nautilus_trader.adapters.betfair.sockets import BetfairStreamClient
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.mark.skipif(
    sys.platform == "win32" and platform.python_version().startswith("3.12"),
    reason="Failing on Windows with Python 3.12",
)
class TestBetfairSockets:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.client = BetfairTestStubs.betfair_client(loop=self.loop)

        yield

    def _build_stream_client(
        self,
        host: str,
        port: int,
        handler: Callable[[bytes], None],
    ) -> BetfairStreamClient:
        client = BetfairStreamClient(
            http_client=self.client,
            message_handler=handler,
            host=host,
            port=port,
        )
        client.use_ssl = False
        return client

    def test_unique_id(self):
        clients = [
            BetfairMarketStreamClient(
                http_client=self.client,
                message_handler=len,
            ),
            BetfairOrderStreamClient(
                http_client=self.client,
                message_handler=len,
            ),
            BetfairMarketStreamClient(
                http_client=self.client,
                message_handler=len,
            ),
        ]
        result = [c.unique_id for c in clients]
        assert result == sorted(set(result))

    @pytest.mark.asyncio
    async def test_socket_client_connect(self, socket_server):
        # Arrange
        messages = []
        host, port = socket_server
        client = self._build_stream_client(host=host, port=port, handler=messages.append)

        # Act
        await client.connect()

        # Assert
        assert client.is_active()
        await client.disconnect()

    @pytest.mark.asyncio
    async def test_socket_client_reconnect(self, closing_socket_server):
        # Arrange
        messages = []
        host, port = closing_socket_server
        client = self._build_stream_client(host=host, port=port, handler=messages.append)

        # Act
        await client.connect()
        await client.reconnect()

        # Assert
        assert client.is_active()
        await client.disconnect()

    @pytest.mark.asyncio
    async def test_socket_client_disconnect(self, closing_socket_server):
        # Arrange
        messages = []
        host, port = closing_socket_server
        client = self._build_stream_client(host=host, port=port, handler=messages.append)

        # Act
        await client.connect()

        # Assert
        assert client.is_active()
        await client.disconnect()


@pytest.fixture
def message_collector():
    """
    Provide a message collector for testing.
    """
    messages = []

    def handler(raw_bytes: bytes):
        decoded = stream_decode(raw_bytes)
        messages.append((raw_bytes, decoded))

    return handler, messages


def test_stream_client_message_handler_receives_raw_bytes(message_collector):
    """
    Test that the message handler receives raw bytes correctly.
    """
    # Arrange
    handler, messages = message_collector
    raw_message = b'{"op":"connection","connectionId":"test-123"}\r\n'

    # Act
    handler(raw_message)

    # Assert
    assert len(messages) == 1
    assert messages[0][0] == raw_message
    assert isinstance(messages[0][1], Connection)
    assert messages[0][1].connection_id == "test-123"


def test_message_handler_processes_market_heartbeat(message_collector):
    """
    Test that market heartbeat messages are processed correctly.
    """
    # Arrange
    handler, messages = message_collector
    raw_message = BetfairStreaming.mcm_HEARTBEAT()

    # Act
    handler(raw_message)

    # Assert
    assert len(messages) == 1
    assert isinstance(messages[0][1], MCM)
    assert messages[0][1].ct.value == "HEARTBEAT"


def test_message_handler_processes_status_message(message_collector):
    """
    Test that status messages are processed correctly.
    """
    # Arrange
    handler, messages = message_collector
    raw_message = (
        msgspec.json.encode(
            {
                "op": "status",
                "id": 1,
                "statusCode": "SUCCESS",
                "connectionClosed": False,
            },
        )
        + b"\r\n"
    )

    # Act
    handler(raw_message)

    # Assert
    assert len(messages) == 1
    assert isinstance(messages[0][1], Status)
    assert messages[0][1].status_code == "SUCCESS"
    assert messages[0][1].connection_closed is False


def test_message_handler_processes_order_change_message(message_collector):
    """
    Test that order change messages are processed correctly.
    """
    # Arrange
    handler, messages = message_collector
    raw_message = (
        msgspec.json.encode(
            {
                "op": "ocm",
                "id": 2,
                "clk": "AAAAAAAA",
                "pt": 1234567890,
                "oc": [],
            },
        )
        + b"\r\n"
    )

    # Act
    handler(raw_message)

    # Assert
    assert len(messages) == 1
    assert isinstance(messages[0][1], OCM)
    assert messages[0][1].id == 2
    assert messages[0][1].clk == "AAAAAAAA"


def test_betfair_messages_not_mistaken_for_fix():
    """
    Test that Betfair JSON messages are not FIX protocol messages.
    """
    # Arrange
    betfair_messages = [
        b'{"op":"connection","connectionId":"123"}\r\n',
        BetfairStreaming.mcm_HEARTBEAT(),
    ]

    # Act & Assert
    for msg in betfair_messages:
        # Verify message doesn't start with FIX header
        assert not msg.startswith(b"8=FIX")

        # Verify it has JSON structure (starts with '{')
        assert msg.strip().startswith(b"{")

        # Verify it can be decoded as Betfair message
        decoded = stream_decode(msg)
        assert decoded is not None


@pytest.mark.parametrize(
    "raw_message,expected_type",
    [
        (b'{"op":"connection","connectionId":"123"}\r\n', Connection),
        (
            msgspec.json.encode(
                {
                    "op": "status",
                    "id": 1,
                    "statusCode": "SUCCESS",
                    "connectionClosed": False,
                    "errorCode": None,
                    "errorMessage": None,
                },
            )
            + b"\r\n",
            Status,
        ),
        (msgspec.json.encode({"op": "mcm", "id": 1, "clk": "ABC", "pt": 123}) + b"\r\n", MCM),
        (
            msgspec.json.encode({"op": "ocm", "id": 1, "clk": "XYZ", "pt": 999, "oc": []})
            + b"\r\n",
            OCM,
        ),
    ],
)
def test_various_message_types_decoded_correctly(raw_message, expected_type):
    """
    Parametrized test for various Betfair message types.
    """
    # Act
    decoded = stream_decode(raw_message)

    # Assert
    assert isinstance(decoded, expected_type)


def test_multiple_messages_processed_in_sequence(message_collector):
    """
    Test that multiple messages are processed correctly in sequence.
    """
    # Arrange
    handler, messages = message_collector
    raw_messages = [
        b'{"op":"connection","connectionId":"conn-1"}\r\n',
        msgspec.json.encode(
            {
                "op": "status",
                "id": 1,
                "statusCode": "SUCCESS",
                "connectionClosed": False,
                "errorCode": None,
                "errorMessage": None,
            },
        )
        + b"\r\n",
        BetfairStreaming.mcm_HEARTBEAT(),
        msgspec.json.encode({"op": "ocm", "id": 2, "clk": "XYZ", "pt": 999, "oc": []}) + b"\r\n",
    ]

    # Act
    for raw in raw_messages:
        handler(raw)

    # Assert
    assert len(messages) == 4
    assert isinstance(messages[0][1], Connection)
    assert isinstance(messages[1][1], Status)
    assert isinstance(messages[2][1], MCM)
    assert isinstance(messages[3][1], OCM)


def test_malformed_message_handling(message_collector):
    """
    Test handling of malformed messages.
    """
    # Arrange
    handler, messages = message_collector
    malformed_messages = [
        b'{"op":"unknown_op"}\r\n',  # Unknown operation
        b'{"missing":"op_field"}\r\n',  # Missing op field
    ]

    # Act & Assert
    for raw in malformed_messages:
        with pytest.raises(Exception):  # stream_decode should raise on invalid messages
            handler(raw)
