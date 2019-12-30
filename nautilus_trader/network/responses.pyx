# -------------------------------------------------------------------------------------------------
# <copyright file="responses.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport GUID
from nautilus_trader.core.message cimport Response


cdef class MessageReceived(Response):
    """
    Represents a response acknowledging receipt of a message.
    """

    def __init__(self,
                 str received_type,
                 GUID correlation_id,
                 GUID response_id,
                 datetime response_timestamp):
        """
        Initializes a new instance of the MessageReceived class.

        :param received_type: The received type.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(correlation_id, response_id, response_timestamp)
        self.received_type = received_type

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}(id={self.id.value}, correlation_id={self.id.value}, received_type={self.received_type})"


cdef class MessageRejected(Response):
    """
    Represents a response indicating rejection of a message.
    """

    def __init__(self,
                 str rejected_message,
                 GUID correlation_id,
                 GUID response_id,
                 datetime response_timestamp):
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
        Return a string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}(id={self.id.value}, correlation_id={self.id.value}, message='{self.message}')"


cdef class QueryFailure(Response):
    """
    Represents a response indicating a query failure.
    """

    def __init__(self,
                 str failure_message,
                 GUID correlation_id,
                 GUID response_id,
                 datetime response_timestamp):
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
        Return a string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}(id={self.id.value}, correlation_id={self.id.value}, message='{self.message}')"


cdef class DataResponse(Response):
    """
    Represents a data response.
    """

    def __init__(self,
                 bytes data,
                 str data_encoding,
                 GUID correlation_id,
                 GUID response_id,
                 datetime response_timestamp):
        """
        Initializes a new instance of the DataResponse class.

        :param data: The response data.
        :param data_encoding: The encoding for the data.
        :param correlation_id: The correlation identifier.
        :param response_id: The response identifier.
        :param response_timestamp: The response timestamp.
        """
        super().__init__(correlation_id, response_id, response_timestamp)
        self.data = data
        self.data_encoding = data_encoding
