# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.core.message import Document
from nautilus_trader.core.message import Message
from nautilus_trader.core.message import MessageCategory
from nautilus_trader.core.message import Response
from nautilus_trader.core.message import message_category_from_str
from nautilus_trader.core.message import message_category_to_str
from nautilus_trader.core.uuid import uuid4


class TestMessage:
    def test_message_equality(self):
        # Arrange
        uuid = uuid4()

        message1 = Message(
            category=MessageCategory.COMMAND,
            message_id=uuid,
            timestamp_ns=0,
        )

        message2 = Message(
            category=MessageCategory.COMMAND,
            message_id=uuid,
            timestamp_ns=0,
        )

        message3 = Message(
            category=MessageCategory.DOCUMENT,  # Different message type
            message_id=uuid,
            timestamp_ns=0,
        )

        message4 = Message(
            category=MessageCategory.DOCUMENT,
            message_id=uuid4(),  # Different UUID
            timestamp_ns=0,
        )

        # Act
        # Assert
        assert message1 == message1
        assert message1 == message2
        assert message1 != message3
        assert message3 != message4

    def test_message_hash(self):
        # Arrange
        message = Document(
            document_id=uuid4(),
            timestamp_ns=0,
        )

        # Act
        # Assert
        assert isinstance(hash(message), int)

    def test_message_str_and_repr(self):
        # Arrange
        uuid = uuid4()
        message = Document(
            document_id=uuid,
            timestamp_ns=0,
        )

        # Act
        # Assert
        assert str(message) == f"Document(id={uuid}, timestamp=0)"
        assert str(message) == f"Document(id={uuid}, timestamp=0)"

    def test_response_message_str_and_repr(self):
        # Arrange
        uuid_id = uuid4()
        uuid_corr = uuid4()
        message = Response(
            correlation_id=uuid_corr,
            response_id=uuid_id,
            timestamp_ns=0,
        )

        # Act
        # Assert
        assert str(message) == f"Response(correlation_id={uuid_corr}, id={uuid_id}, timestamp=0)"
        assert str(message) == f"Response(correlation_id={uuid_corr}, id={uuid_id}, timestamp=0)"

    @pytest.mark.parametrize(
        "category, expected",
        [
            [MessageCategory.STRING, "STRING"],
            [MessageCategory.COMMAND, "COMMAND"],
            [MessageCategory.DOCUMENT, "DOCUMENT"],
            [MessageCategory.EVENT, "EVENT"],
            [MessageCategory.REQUEST, "REQUEST"],
            [MessageCategory.RESPONSE, "RESPONSE"],
        ],
    )
    def test_message_category_to_str(self, category, expected):
        # Arrange
        # Act
        result = message_category_to_str(category)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "string, expected",
        [
            ["STRING", MessageCategory.STRING],
            ["COMMAND", MessageCategory.COMMAND],
            ["DOCUMENT", MessageCategory.DOCUMENT],
            ["EVENT", MessageCategory.EVENT],
            ["REQUEST", MessageCategory.REQUEST],
            ["RESPONSE", MessageCategory.RESPONSE],
        ],
    )
    def test_message_category_from_str(self, string, expected):
        # Arrange
        # Act
        result = message_category_from_str(string)

        # Assert
        assert result == expected
