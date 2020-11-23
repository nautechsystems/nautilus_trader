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


cpdef str message_type_to_string(int value):
    """
    Covert a C enum int to a message type string.

    Parameters
    ----------
    value : int
        The value to convert.

    Returns
    -------
    str

    """
    if value == 1:
        return 'STRING'
    elif value == 2:
        return 'COMMAND'
    elif value == 3:
        return 'DOCUMENT'
    elif value == 4:
        return 'EVENT'
    elif value == 5:
        return 'REQUEST'
    elif value == 6:
        return 'RESPONSE'
    else:
        return 'UNDEFINED'


cpdef MessageType message_type_from_string(str value):
    """
    Parse a string to a message type.

    Parameters
    ----------
    value : str
        The value to parse.

    Returns
    -------
    str

    """
    if value == 'STRING':
        return MessageType.STRING
    elif value == 'COMMAND':
        return MessageType.COMMAND
    elif value == 'DOCUMENT':
        return MessageType.DOCUMENT
    elif value == 'EVENT':
        return MessageType.EVENT
    elif value == 'REQUEST':
        return MessageType.REQUEST
    elif value == 'RESPONSE':
        return MessageType.RESPONSE
    else:
        return MessageType.UNDEFINED


cdef class Message:
    """
    The abstract base class for all messages.

    It should not be used directly, but through its concrete subclasses.
    """

    def __init__(
            self,
            MessageType msg_type,
            UUID identifier not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the `Message` class.

        Parameters
        ----------
        msg_type : MessageType
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
        Condition.not_equal(msg_type, MessageType.UNDEFINED, "msg_type", "UNDEFINED")

        self.type = msg_type
        self.id = identifier
        self.timestamp = timestamp

    def __eq__(self, Message other) -> bool:
        return self.type == other.type and self.id == other.id

    def __ne__(self, Message other) -> bool:
        return self.type != other.type or self.id != other.id

    def __hash__(self) -> int:
        return hash((self.type, self.id))

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id}, timestamp={self.timestamp})"


cdef class Command(Message):
    """
    The abstract base class for all commands.

    It should not be used directly, but through its concrete subclasses.
    """

    def __init__(self, UUID identifier not None, datetime timestamp not None):
        """
        Initialize a new instance of the `Command` class.

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
    The abstract base class for all documents.

    It should not be used directly, but through its concrete subclasses.
    """

    def __init__(
            self,
            UUID identifier not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the `Document` class.

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
    The abstract base class for all events.

    It should not be used directly, but through its concrete subclasses.
    """

    def __init__(
            self,
            UUID identifier not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the `Event` class.

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
    The abstract base class for all requests.

    It should not be used directly, but through its concrete subclasses.
    """

    def __init__(self, UUID identifier not None, datetime timestamp not None):
        """
        Initialize a new instance of the `Request` class.

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
    The abstract base class for all responses.

    It should not be used directly, but through its concrete subclasses.
    """

    def __init__(
            self,
            UUID correlation_id not None,
            UUID identifier not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the `Response` class.

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

        self.correlation_id = correlation_id

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"correlation_id={self.correlation_id}, "
                f"id={self.id}, "
                f"timestamp={self.timestamp})")
