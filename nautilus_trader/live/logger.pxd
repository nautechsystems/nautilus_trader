# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.logger cimport Logger


cdef class LogMessage:
    """
    Represents a log message.
    """
    cdef readonly datetime timestamp
    cdef readonly int level
    cdef readonly str text
    cdef str as_string(self)


cdef class LiveLogger(Logger):
    """
    Provides a thread safe logger for live concurrent operations.
    """
    cdef object _queue
    cdef object _thread
    cpdef void _process_messages(self)
