# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.logger cimport Logger
from nautilus_trader.live.stores cimport LogStore


cdef class LiveLogger(Logger):
    """
    Provides a thread safe logger for live concurrent operations.
    """
    cdef object _queue
    cdef object _thread
    cdef LogStore _store
    cpdef void _process_queue(self)
