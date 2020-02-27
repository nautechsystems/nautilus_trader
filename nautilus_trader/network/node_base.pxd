# -------------------------------------------------------------------------------------------------
# <copyright file="node_base.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.network.compression cimport Compressor


cdef class NetworkNode:
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef str _network_address
    cdef object _context
    cdef object _socket
    cdef int _expected_frames
    cdef Compressor _compressor

    cdef readonly int sent_count
    cdef readonly int recv_count

    cpdef void dispose(self) except *
    cpdef bint is_disposed(self)
    cdef void _send(self, list frames) except *
