# -------------------------------------------------------------------------------------------------
# <copyright file="messages.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport GUID, Identifier
from nautilus_trader.core.message cimport Request
from nautilus_trader.model.identifiers cimport TraderId


cdef class SessionId(Identifier):
    """
    Represents a unique network session identifier
    """

    def __init__(self, str value not None):
        """
        Initializes a new instance of the SessionId class.

        :param value: The session identifier value.
        """
        super().__init__(value)


cdef class Connect(Request):
    """
    Represents a request to connect to a session.
    """

    def __init__(self,
                 TraderId trader_id not None,
                 GUID request_id not None,
                 datetime request_timestamp not None):
        """
        Initializes a new instance of the Connect class.

        :param trader_id: The trader identifier.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)

        self.trader_id = trader_id


cdef class Connected(Response):
    """
    Represents a response confirming connection to a session.
    """

    def __init__(self,
                 str service_name not None,
                 str message not None,
                 SessionId session_id not None,
                 GUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initializes a new instance of the Connected class.

        :param service_name: The service name connected to.
        :param message: The connected message.
        :param message: The connected session identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(response_id, response_timestamp)

        self.service_name = service_name
        self.message = message
        self.session_id = session_id


cdef class Disconnect(Request):
    """
    Represents a request to disconnect from a session.
    """

    def __init__(self,
                 TraderId trader_id not None,
                 GUID request_id not None,
                 datetime request_timestamp not None):
        """
        Initializes a new instance of the Disconnect class.

        :param trader_id: The trader identifier.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)

        self.trader_id = trader_id


cdef class Disconnected(Response):
    """
    Represents a response confirming disconnection from a session.
    """

    def __init__(self,
                 str service_name not None,
                 str message not None,
                 SessionId session_id not None,
                 GUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initializes a new instance of the Disconnected class.

        :param service_name: The service name disconnected from.
        :param message: The disconnected message.
        :param message: The disconnected session identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(response_id, response_timestamp)

        self.service_name = service_name
        self.message = message
        self.session_id = session_id


cdef class MessageReceived(Response):
    """
    Represents a response acknowledging receipt of a message.
    """

    def __init__(self,
                 str received_type,
                 GUID correlation_id not None,
                 GUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initializes a new instance of the MessageReceived class.

        :param received_type: The received type.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        Condition.valid_string(received_type, 'received_type')
        super().__init__(correlation_id, response_id, response_timestamp)

        self.received_type = received_type

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"received_type={self.received_type}, "
                f"id={self.id.value}, "
                f"correlation_id={self.id.value})")


cdef class MessageRejected(Response):
    """
    Represents a response indicating rejection of a message.
    """

    def __init__(self,
                 str rejected_message not None,  # Could be an empty string
                 GUID correlation_id not None,
                 GUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initializes a new instance of the MessageRejected class.

        :param rejected_message: The rejected message.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(correlation_id, response_id, response_timestamp)

        self.message = rejected_message

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"message='{self.message}, '"
                f"id={self.id.value}, "
                f"correlation_id={self.id.value})")


cdef class QueryFailure(Response):
    """
    Represents a response indicating a query failure.
    """

    def __init__(self,
                 str failure_message not None,  # Could be an empty string
                 GUID correlation_id not None,
                 GUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initializes a new instance of the QueryFailure class.

        :param failure_message: The failure message.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(correlation_id, response_id, response_timestamp)

        self.message = failure_message

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"message='{self.message}', "
                f"id={self.id.value}, "
                f"correlation_id={self.id.value})")


cdef class DataRequest(Request):
    """
    Represents a request for data.
    """

    def __init__(self,
                 dict query not None,
                 GUID request_id not None,
                 datetime request_timestamp not None):
        """
        Initializes a new instance of the DataRequest class.

        :param query: The data query.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)

        self.query = query


cdef class DataResponse(Response):
    """
    Represents a data response.
    """

    def __init__(self,
                 bytes data not None,
                 str data_type not None,
                 str data_encoding not None,
                 GUID correlation_id not None,
                 GUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initializes a new instance of the DataResponse class.

        :param data: The response data.
        :param data_encoding: The encoding for the data.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        Condition.valid_string(data_type, 'data_type')
        Condition.valid_string(data_encoding, 'data_encoding')
        super().__init__(correlation_id, response_id, response_timestamp)

        self.data = data
        self.data_type = data_type
        self.data_encoding = data_encoding

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"data_type='{self.data_type}', "
                f"data_encoding='{self.data_encoding}', "
                f"id={self.id.value}, "
                f"correlation_id={self.id.value})")
