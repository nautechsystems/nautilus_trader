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
from nautilus_trader.core.message cimport Request
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.network.identifiers cimport ServerId
from nautilus_trader.network.identifiers cimport SessionId


cdef class Connect(Request):
    """
    Represents a request to connect to a session.
    """

    def __init__(self,
                 ClientId client_id not None,
                 str authentication not None,
                 UUID request_id not None,
                 datetime request_timestamp not None):
        """
        Initialize a new instance of the Connect class.

        :param client_id: The client identifier.
        :param authentication: The client authentication.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)

        self.client_id = client_id
        self.authentication = authentication


cdef class Connected(Response):
    """
    Represents a response confirming connection to a session.
    """

    def __init__(self,
                 str message not None,
                 ServerId server_id not None,
                 SessionId session_id not None,
                 UUID correlation_id not None,
                 UUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initialize a new instance of the Connected class.

        :param message: The connected message.
        :param server_id: The service name connected to.
        :param message: The connected session identifier.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(correlation_id, response_id, response_timestamp)

        self.message = message
        self.server_id = server_id
        self.session_id = session_id


cdef class Disconnect(Request):
    """
    Represents a request to disconnect from a session.
    """

    def __init__(self,
                 ClientId client_id not None,
                 SessionId session_id not None,
                 UUID request_id not None,
                 datetime request_timestamp not None):
        """
        Initialize a new instance of the Disconnect class.

        :param client_id: The client identifier.
        :param session_id: The session to disconnect from.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)

        self.client_id = client_id
        self.session_id = session_id


cdef class Disconnected(Response):
    """
    Represents a response confirming disconnection from a session.
    """

    def __init__(self,
                 str message not None,
                 ServerId server_id not None,
                 SessionId session_id not None,
                 UUID correlation_id not None,
                 UUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initialize a new instance of the Disconnected class.

        :param message: The disconnected message.
        :param server_id: The server identifier to disconnected from.
        :param session_id: The session disconnected from.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(correlation_id, response_id, response_timestamp)

        self.message = message
        self.server_id = server_id
        self.session_id = session_id


cdef class MessageReceived(Response):
    """
    Represents a response acknowledging receipt of a message.
    """

    def __init__(self,
                 str received_type,
                 UUID correlation_id not None,
                 UUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initialize a new instance of the MessageReceived class.

        :param received_type: The received type.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        Condition.valid_string(received_type, "received_type")
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
                 UUID correlation_id not None,
                 UUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initialize a new instance of the MessageRejected class.

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
                f"message='{self.message}', "
                f"id={self.id.value}, "
                f"correlation_id={self.id.value})")


cdef class QueryFailure(Response):
    """
    Represents a response indicating a query failure.
    """

    def __init__(self,
                 str failure_message not None,  # Could be an empty string
                 UUID correlation_id not None,
                 UUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initialize a new instance of the QueryFailure class.

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
                 UUID request_id not None,
                 datetime request_timestamp not None):
        """
        Initialize a new instance of the DataRequest class.

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
                 UUID correlation_id not None,
                 UUID response_id not None,
                 datetime response_timestamp not None):
        """
        Initialize a new instance of the DataResponse class.

        :param data: The response data.
        :param data_encoding: The encoding for the data.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        Condition.valid_string(data_type, "data_type")
        Condition.valid_string(data_encoding, "data_encoding")
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
