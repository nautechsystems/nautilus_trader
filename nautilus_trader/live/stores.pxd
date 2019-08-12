# -------------------------------------------------------------------------------------------------
# <copyright file="stores.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.live.logger cimport LogMessage


cdef class LogStore:
    """
    Provides a log store.
    """
    cdef object _process
    cdef object _queue
    cdef object _redis
    cdef str _store_key

    cpdef void store(self, LogMessage message)
    cpdef void _process_messages(self)
