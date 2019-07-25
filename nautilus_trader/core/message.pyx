# -------------------------------------------------------------------------------------------------
# <copyright file="message.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.model.identifiers cimport GUID


cdef class Message:
    """
    The base class for all messages.
    """

    def __init__(self, GUID identifier, datetime timestamp):
        """
        Initializes a new instance of the Message abstract class.

        :param identifier: The message identifier.
        :param timestamp: The message timestamp.
        """
        self.id = identifier
        self.timestamp = timestamp

    cdef bint equals(self, Message other):
        """
        Return a value indicating whether the given message is equal to this message.
        
        :param other: The other message to compare
        :return: True if the messages are equal, otherwise False.
        """
        if isinstance(other, self.__class__):
            return self.id == other.id
        else:
            return False

    def __eq__(self, Message other) -> bool:
        """
        Return a value indicating whether the given message is equal to this message.

        :param other: The other message to compare
        :return: True if the messages are equal, otherwise False.
        """
        return self.equals(other)

    def __ne__(self, Message other):
        """
        Return a value indicating whether the given message is not equal to this message.

        :param other: The other message to compare
        :return: True if the messages are not equal, otherwise False.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash for this message.

        :return: int.
        """
        return hash(self.id)

    def __str__(self) -> str:
        """
        Return the str() string representation of this message.

        :return: str.
        """
        return f"{self.__class__.__name__}({self.id.value})"

    def __repr__(self) -> str:
        """
        Return the repr() string representation of this message.

        :return: str.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class Command(Message):
    """
    The base class for all commands.
    """

    def __init__(self, GUID identifier, datetime timestamp):
        """
        Initializes a new instance of the Command abstract class.

        :param identifier: The command identifier.
        :param timestamp: The command timestamp.
        """
        super().__init__(identifier, timestamp)


cdef class Event(Message):
    """
    The base class for all events.
    """

    def __init__(self,
                 GUID identifier,
                 datetime timestamp):
        """
        Initializes a new instance of the Event abstract class.

        :param identifier: The event identifier.
        :param timestamp: The event timestamp.
        """
        super().__init__(identifier, timestamp)


cdef class Request(Message):
    """
    The base class for all requests.
    """

    def __init__(self, GUID identifier, datetime timestamp):
        """
        Initializes a new instance of the Request abstract class.

        :param identifier: The request identifier.
        :param timestamp: The request timestamp.
        """
        super().__init__(identifier, timestamp)


cdef class Response(Message):
    """
    The base class for all responses.
    """

    def __init__(self,
                 GUID correlation_id,
                 GUID identifier,
                 datetime timestamp):
        """
        Initializes a new instance of the Response abstract class.

        :param identifier: The correlation identifier.
        :param identifier: The response identifier.
        :param timestamp: The response timestamp.
        """
        super().__init__(identifier, timestamp)
        self.correlation_id = correlation_id
