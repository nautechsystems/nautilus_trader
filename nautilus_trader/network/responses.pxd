# -------------------------------------------------------------------------------------------------
# <copyright file="responses.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Response


cdef class MessageReceived(Response):
    cdef readonly str received_type


cdef class MessageRejected(Response):
    cdef readonly str message


cdef class QueryFailure(Response):
    cdef readonly str message


cdef class DataResponse(Response):
    cdef readonly bytes data
    cdef readonly str data_type
    cdef readonly str data_encoding
