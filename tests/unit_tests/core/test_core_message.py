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

from nautilus_trader.core.message import Document
from nautilus_trader.core.message import Event
from nautilus_trader.core.message import Response
from nautilus_trader.core.uuid import UUID4


class TestMessage:
    def test_event_equality(self):
        # Arrange
        uuid = UUID4()

        event1 = Event(
            event_id=uuid,
            ts_event=0,
            ts_init=0,
        )

        event2 = Event(
            event_id=uuid,
            ts_event=0,
            ts_init=0,
        )

        event3 = Event(
            event_id=UUID4(),  # Different UUID4
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert event1 == event1
        assert event1 == event2
        assert event1 != event3

    def test_message_hash(self):
        # Arrange
        message = Document(
            document_id=UUID4(),
            ts_init=0,
        )

        # Act, Assert
        assert isinstance(hash(message), int)

    def test_message_str_and_repr(self):
        # Arrange
        uuid = UUID4()
        message = Document(
            document_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert str(message) == f"Document(id={uuid}, ts_init=0)"
        assert str(message) == f"Document(id={uuid}, ts_init=0)"

    def test_response_message_str_and_repr(self):
        # Arrange
        uuid_id = UUID4()
        uuid_corr = UUID4()
        response = Response(
            correlation_id=uuid_corr,
            response_id=uuid_id,
            ts_init=0,
        )

        # Act, Assert
        assert str(response) == f"Response(correlation_id={uuid_corr}, id={uuid_id}, ts_init=0)"
        assert str(response) == f"Response(correlation_id={uuid_corr}, id={uuid_id}, ts_init=0)"
