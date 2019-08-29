# -------------------------------------------------------------------------------------------------
# <copyright file="handlers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cdef class Handler:
    cdef readonly object handle


cdef class TickHandler(Handler):
    #cdef void (*_handler)(Tick tick)
    pass


cdef class BarHandler(Handler):
    pass


cdef class InstrumentHandler(Handler):
    pass


cdef class EventHandler(Handler):
    pass
