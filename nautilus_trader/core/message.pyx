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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport MessageType
from nautilus_trader.core.uuid cimport UUID


cpdef str message_type_to_str(int value):
    """
    Covert a C Enum int to a message type string.

    Parameters
    ----------
    value : int
        The value to convert.

    Returns
    -------
    str

    """
    if value == 1:
        return "STRING"
    elif value == 2:
        return "COMMAND"
    elif value == 3:
        return "DOCUMENT"
    elif value == 4:
        return "EVENT"
    elif value == 5:
        return "REQUEST"
    elif value == 6:
        return "RESPONSE"


cpdef MessageType message_type_from_str(str value):
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
    if value == "STRING":
        return MessageType.STRING
    elif value == "COMMAND":
        return MessageType.COMMAND
    elif value == "DOCUMENT":
        return MessageType.DOCUMENT
    elif value == "EVENT":
        return MessageType.EVENT
    elif value == "REQUEST":
        return MessageType.REQUEST
    elif value == "RESPONSE":
        return MessageType.RESPONSE


cdef class Message:
    """
    The abstract base class for all messages.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        MessageType msg_type,
        UUID identifier not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``Message`` class.

        Parameters
        ----------
        msg_type : MessageType
            The message type.
        identifier : UUID
            The message identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the message initialization.

        """
        self.type = msg_type
        self.id = identifier
        self.timestamp_ns = timestamp_ns

    def __eq__(self, Message other) -> bool:
        return self.type == other.type and self.id == other.id

    def __ne__(self, Message other) -> bool:
        return self.type != other.type or self.id != other.id

    def __hash__(self) -> int:
        return hash((self.type, self.id))

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id}, timestamp={self.timestamp_ns})"


cdef class Command(Message):
    """
    The abstract base class for all commands.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, UUID identifier not None, int64_t timestamp_ns):
        """
        Initialize a new instance of the ``Command`` class.

        Parameters
        ----------
        identifier : UUID
            The command identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the command initialization.

        """
        super().__init__(MessageType.COMMAND, identifier, timestamp_ns)


cdef class Document(Message):
    """
    The abstract base class for all documents.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID identifier not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``Document`` class.

        Parameters
        ----------
        identifier : UUID
            The document identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the document initialization.

        """
        super().__init__(MessageType.DOCUMENT, identifier, timestamp_ns)


cdef class Event(Message):
    """
    The abstract base class for all events.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID identifier not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``Event`` class.

        Parameters
        ----------
        identifier : UUID
            The event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        super().__init__(MessageType.EVENT, identifier, timestamp_ns)


cdef class Request(Message):
    """
    The abstract base class for all requests.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, UUID identifier not None, int64_t timestamp_ns):
        """
        Initialize a new instance of the ``Request`` class.

        Parameters
        ----------
        identifier : UUID
            The request identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the request initialization.

        """
        super().__init__(MessageType.REQUEST, identifier, timestamp_ns)


cdef class Response(Message):
    """
    The abstract base class for all responses.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID correlation_id not None,
        UUID identifier not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``Response`` class.

        Parameters
        ----------
        correlation_id : UUID
            The correlation identifier.
        identifier : UUID
            The response identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the response initialization.

        """
        super().__init__(MessageType.RESPONSE, identifier, timestamp_ns)

        self.correlation_id = correlation_id

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"correlation_id={self.correlation_id}, "
                f"id={self.id}, "
                f"timestamp={self.timestamp_ns})")
