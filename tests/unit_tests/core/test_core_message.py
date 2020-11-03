# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import unittest

from parameterized import parameterized

from nautilus_trader.core.message import Document
from nautilus_trader.core.message import Message
from nautilus_trader.core.message import MessageType
from nautilus_trader.core.message import message_type_from_string
from nautilus_trader.core.message import message_type_to_string
from nautilus_trader.core.message import Response
from nautilus_trader.core.uuid import uuid4
from tests.test_kit.stubs import UNIX_EPOCH


class MessageTests(unittest.TestCase):

    def test_message_equality(self):
        # Arrange
        uuid = uuid4()

        message1 = Message(
            msg_type=MessageType.COMMAND,
            identifier=uuid,
            timestamp=UNIX_EPOCH,
        )

        message2 = Message(
            msg_type=MessageType.COMMAND,
            identifier=uuid,
            timestamp=UNIX_EPOCH,
        )

        message3 = Message(
            msg_type=MessageType.DOCUMENT,  # Different message type
            identifier=uuid,
            timestamp=UNIX_EPOCH,
        )

        message4 = Message(
            msg_type=MessageType.DOCUMENT,
            identifier=uuid4(),  # Different UUID
            timestamp=UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertTrue(message1 == message1)
        self.assertTrue(message1 == message2)
        self.assertTrue(message1 != message3)
        self.assertTrue(message3 != message4)

    def test_message_hash(self):
        # Arrange
        message = Document(
            identifier=uuid4(),
            timestamp=UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertEqual(int, type(hash(message)))

    def test_message_str_and_repr(self):
        # Arrange
        uuid = uuid4()
        message = Document(
            identifier=uuid,
            timestamp=UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertEqual(f"Document(id={uuid}, timestamp=1970-01-01 00:00:00+00:00)", str(message))
        self.assertEqual(f"Document(id={uuid}, timestamp=1970-01-01 00:00:00+00:00)", str(message))

    def test_response_message_str_and_repr(self):
        # Arrange
        uuid_id = uuid4()
        uuid_corr = uuid4()
        message = Response(
            correlation_id=uuid_corr,
            identifier=uuid_id,
            timestamp=UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertEqual(f"Response(correlation_id={uuid_corr}, id={uuid_id}, timestamp=1970-01-01 00:00:00+00:00)", str(message))
        self.assertEqual(f"Response(correlation_id={uuid_corr}, id={uuid_id}, timestamp=1970-01-01 00:00:00+00:00)", str(message))

    @parameterized.expand([
        [MessageType.UNDEFINED, "UNDEFINED"],
        [MessageType.STRING, "STRING"],
        [MessageType.COMMAND, "COMMAND"],
        [MessageType.DOCUMENT, "DOCUMENT"],
        [MessageType.EVENT, "EVENT"],
        [MessageType.REQUEST, "REQUEST"],
        [MessageType.RESPONSE, "RESPONSE"],
    ])
    def test_message_type_to_string(self, msg_type, expected):
        # Arrange
        # Act
        result = message_type_to_string(msg_type)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["UNDEFINED", MessageType.UNDEFINED],
        ["STRING", MessageType.STRING],
        ["COMMAND", MessageType.COMMAND],
        ["DOCUMENT", MessageType.DOCUMENT],
        ["EVENT", MessageType.EVENT],
        ["REQUEST", MessageType.REQUEST],
        ["RESPONSE", MessageType.RESPONSE],
    ])
    def test_message_type_from_string(self, string, expected):
        # Arrange
        # Act
        result = message_type_from_string(string)

        # Assert
        self.assertEqual(expected, result)
