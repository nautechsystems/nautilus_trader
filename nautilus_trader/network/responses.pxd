# -------------------------------------------------------------------------------------------------
# <copyright file="responses.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.types cimport GUID
from nautilus_trader.core.message cimport Response


cdef class MessageReceived(Response):
    """
    Represents a response acknowledging receipt of a message.
    """
    cdef readonly str received_type


cdef class MessageRejected(Response):
    """
    Represents a response indicating rejection of a message.
    """
    cdef readonly str message


cdef class QueryFailure(Response):
    """
    Represents a response indicating query failure.
    """
    cdef readonly str message


cdef class DataResponse(Response):
    """
    Represents a response of data.
    """
    cdef readonly bytes data
    cdef readonly str data_encoding
