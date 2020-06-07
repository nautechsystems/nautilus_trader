# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport Identifier


cdef class ClientId(Identifier):
    pass


cdef class ServerId(Identifier):
    pass


cdef class SessionId(Identifier):
    @staticmethod
    cdef SessionId create(ClientId client_id, datetime now, str secret)
