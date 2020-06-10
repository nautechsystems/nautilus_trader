# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport GUID
from nautilus_trader.core.message cimport MessageType


cdef class Message:
    """
    The base class for all messages.
    """

    def __init__(self,
                 MessageType message_type,
                 GUID identifier not None,
                 datetime timestamp not None):
        """
        Initializes a new instance of the Message class.

        :param message_type: The message type.
        :param identifier: The message identifier.
        :param timestamp: The message timestamp.
        :raises ValueError: If the message_type is UNDEFINED.
        """
        Condition.not_equal(message_type, MessageType.UNDEFINED, 'message_type', 'UNDEFINED')

        self.message_type = message_type
        self.id = identifier
        self.timestamp = timestamp

    cpdef bint equals(self, Message other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        if self.message_type == other.message_type:
            return self.id == other.id
        else:
            return False

    def __eq__(self, Message other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, Message other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.id)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}({self.id.value})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class Command(Message):
    """
    The base class for all commands.
    """

    def __init__(self, GUID identifier not None, datetime timestamp not None):
        """
        Initializes a new instance of the Command class.

        :param identifier: The command identifier.
        :param timestamp: The command timestamp.
        """
        super().__init__(MessageType.COMMAND, identifier, timestamp)


cdef class Document(Message):
    """
    The base class for all documents.
    """

    def __init__(self,
                 GUID identifier not None,
                 datetime timestamp not None):
        """
        Initializes a new instance of the Document class.

        :param identifier: The event identifier.
        :param timestamp: The event timestamp.
        """
        super().__init__(MessageType.DOCUMENT, identifier, timestamp)


cdef class Event(Message):
    """
    The base class for all events.
    """

    def __init__(self,
                 GUID identifier not None,
                 datetime timestamp not None):
        """
        Initializes a new instance of the Event class.

        :param identifier: The event identifier.
        :param timestamp: The event timestamp.
        """
        super().__init__(MessageType.EVENT, identifier, timestamp)


cdef class Request(Message):
    """
    The base class for all requests.
    """

    def __init__(self, GUID identifier not None, datetime timestamp not None):
        """
        Initializes a new instance of the Request class.

        :param identifier: The request identifier.
        :param timestamp: The request timestamp.
        """
        super().__init__(MessageType.REQUEST, identifier, timestamp)


cdef class Response(Message):
    """
    The base class for all responses.
    """

    def __init__(self,
                 GUID correlation_id not None,
                 GUID identifier not None,
                 datetime timestamp not None):
        """
        Initializes a new instance of the Response class.

        :param identifier: The correlation identifier.
        :param identifier: The response identifier.
        :param timestamp: The response timestamp.
        """
        super().__init__(MessageType.RESPONSE, identifier, timestamp)

        self.correlation_id = correlation_id

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}(id={self.id.value}, correlation_id={self.correlation_id.value})"
