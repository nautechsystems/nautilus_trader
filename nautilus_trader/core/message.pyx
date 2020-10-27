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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport MessageType
from nautilus_trader.core.uuid cimport UUID


cdef class Message:
    """
    The base class for all messages.
    """

    def __init__(
            self,
            MessageType message_type,
            UUID identifier not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the Message class.

        Parameters
        ----------
        message_type : MessageType
            The message type.
        identifier : UUID
            The message identifier.
        timestamp : datetime
            The message timestamp.

        Raises
        ------
        ValueError
            If message_type is UNDEFINED.

        """
        Condition.not_equal(message_type, MessageType.UNDEFINED, "message_type", "UNDEFINED")

        self._message_type = message_type
        self._id = identifier
        self._timestamp = timestamp

    def __eq__(self, Message other) -> bool:
        return self._message_type == other.message_type and self._id == other.id

    def __ne__(self, Message other) -> bool:
        return self._message_type != other.message_type or self._id != other.id

    def __hash__(self) -> int:
        return hash(self._id)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self._id}, timestamp={self._timestamp})"

    @property
    def message_type(self):
        """
        The generic message type.

        Returns
        -------
        MessageType

        """
        return self._message_type

    @property
    def id(self):
        """
        The message identifier.

        Returns
        -------
        UUID

        """
        return self._id

    @property
    def timestamp(self):
        """
        The message initialization timestamp.

        Returns
        -------
        datetime

        """
        return self._timestamp


cdef class Command(Message):
    """
    The base class for all commands.
    """

    def __init__(self, UUID identifier not None, datetime timestamp not None):
        """
        Initialize a new instance of the Command class.

        Parameters
        ----------
        identifier : UUID
            The command identifier.
        timestamp : datetime
            The command timestamp.

        """
        super().__init__(MessageType.COMMAND, identifier, timestamp)


cdef class Document(Message):
    """
    The base class for all documents.
    """

    def __init__(
            self,
            UUID identifier not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the Document class.

        Parameters
        ----------
        identifier : UUID
            The document identifier.
        timestamp : datetime
            The document timestamp.

        """
        super().__init__(MessageType.DOCUMENT, identifier, timestamp)


cdef class Event(Message):
    """
    The base class for all events.
    """

    def __init__(
            self,
            UUID identifier not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the Event class.

        Parameters
        ----------
        identifier : UUID
            The event identifier.
        timestamp : datetime
            The event timestamp.

        """
        super().__init__(MessageType.EVENT, identifier, timestamp)


cdef class Request(Message):
    """
    The base class for all requests.
    """

    def __init__(self, UUID identifier not None, datetime timestamp not None):
        """
        Initialize a new instance of the Request class.

        Parameters
        ----------
        identifier : UUID
            The request identifier.
        timestamp : datetime
            The request timestamp.

        """
        super().__init__(MessageType.REQUEST, identifier, timestamp)


cdef class Response(Message):
    """
    The base class for all responses.
    """

    def __init__(
            self,
            UUID correlation_id not None,
            UUID identifier not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the Response class.

        Parameters
        ----------
        correlation_id : UUID
            The correlation identifier.
        identifier : UUID
            The response identifier.
        timestamp : datetime
            The response timestamp.

        """
        super().__init__(MessageType.RESPONSE, identifier, timestamp)

        self._correlation_id = correlation_id

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"correlation_id={self._correlation_id}, "
                f"id={self._id}, "
                f"timestamp={self._timestamp})")

    @property
    def correlation_id(self):
        """
        The message correlation identifier.

        Returns
        -------
        datetime

        """
        return self._correlation_id
