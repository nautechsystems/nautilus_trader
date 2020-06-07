# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Request, Response
from nautilus_trader.network.identifiers cimport ClientId, ServerId, SessionId


cdef class Connect(Request):
    cdef readonly ClientId client_id
    cdef readonly str authentication


cdef class Connected(Response):
    cdef readonly str message
    cdef readonly ServerId server_id
    cdef readonly SessionId session_id


cdef class Disconnect(Request):
    cdef readonly ClientId client_id
    cdef readonly SessionId session_id


cdef class Disconnected(Response):
    cdef readonly str message
    cdef readonly ServerId server_id
    cdef readonly SessionId session_id


cdef class MessageReceived(Response):
    cdef readonly str received_type


cdef class MessageRejected(Response):
    cdef readonly str message


cdef class QueryFailure(Response):
    cdef readonly str message


cdef class DataRequest(Request):
    cdef readonly dict query


cdef class DataResponse(Response):
    cdef readonly bytes data
    cdef readonly str data_type
    cdef readonly str data_encoding
