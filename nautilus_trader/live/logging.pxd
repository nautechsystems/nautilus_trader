# -------------------------------------------------------------------------------------------------
# <copyright file="logging.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.logging cimport LogMessage, Logger
from nautilus_trader.serialization.base cimport LogSerializer


cdef class LogStore:
    cdef str _key
    cdef object _queue
    cdef object _process
    cdef object _redis
    cdef LogSerializer _serializer

    cpdef void store(self, LogMessage message)
    cpdef void _consume_messages(self) except *


cdef class LiveLogger(Logger):
    cdef object _queue
    cdef object _thread
    cdef LogStore _store

    cpdef void _consume_messages(self) except *
