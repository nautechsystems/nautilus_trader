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

from nautilus_trader.core.message cimport MessageCategory
from nautilus_trader.core.uuid cimport UUID


cpdef str message_category_to_str(int value):
    """
    Convert a C Enum int to a message category string.

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
    else:
        raise ValueError(f"value was invalid, was {value}")


cpdef MessageCategory message_category_from_str(str value):
    """
    Parse a string to a message category.

    Parameters
    ----------
    value : str
        The value to parse.

    Returns
    -------
    str

    """
    if value == "STRING":
        return MessageCategory.STRING
    elif value == "COMMAND":
        return MessageCategory.COMMAND
    elif value == "DOCUMENT":
        return MessageCategory.DOCUMENT
    elif value == "EVENT":
        return MessageCategory.EVENT
    elif value == "REQUEST":
        return MessageCategory.REQUEST
    elif value == "RESPONSE":
        return MessageCategory.RESPONSE
    else:
        raise ValueError(f"value was invalid, was {value}")


cdef class Message:
    """
    The abstract base class for all messages.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        MessageCategory category,
        UUID message_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``Message`` class.

        Parameters
        ----------
        category : MessageCategory
            The message category.
        message_id : UUID
            The message ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the message initialization.

        """
        self.category = category
        self.id = message_id
        self.timestamp_ns = timestamp_ns

    def __eq__(self, Message other) -> bool:
        return self.category == other.category and self.id == other.id

    def __hash__(self) -> int:
        return hash((self.category, self.id))

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id}, timestamp={self.timestamp_ns})"


cdef class Command(Message):
    """
    The abstract base class for all commands.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, UUID command_id not None, int64_t timestamp_ns):
        """
        Initialize a new instance of the ``Command`` class.

        Parameters
        ----------
        command_id : UUID
            The command ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the command initialization.

        """
        super().__init__(MessageCategory.COMMAND, command_id, timestamp_ns)


cdef class Document(Message):
    """
    The abstract base class for all documents.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID document_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``Document`` class.

        Parameters
        ----------
        document_id : UUID
            The document ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the document initialization.

        """
        super().__init__(MessageCategory.DOCUMENT, document_id, timestamp_ns)


cdef class Event(Message):
    """
    The abstract base class for all events.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID event_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``Event`` class.

        Parameters
        ----------
        event_id : UUID
            The event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the event initialization.

        """
        super().__init__(MessageCategory.EVENT, event_id, timestamp_ns)


cdef class Request(Message):
    """
    The abstract base class for all requests.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, UUID request_id not None, int64_t timestamp_ns):
        """
        Initialize a new instance of the ``Request`` class.

        Parameters
        ----------
        request_id : UUID
            The request ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the request initialization.

        """
        super().__init__(MessageCategory.REQUEST, request_id, timestamp_ns)


cdef class Response(Message):
    """
    The abstract base class for all responses.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID correlation_id not None,
        UUID response_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``Response`` class.

        Parameters
        ----------
        correlation_id : UUID
            The correlation ID.
        response_id : UUID
            The response ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the response initialization.

        """
        super().__init__(MessageCategory.RESPONSE, response_id, timestamp_ns)

        self.correlation_id = correlation_id

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"correlation_id={self.correlation_id}, "
                f"id={self.id}, "
                f"timestamp={self.timestamp_ns})")
