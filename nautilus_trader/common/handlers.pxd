# -------------------------------------------------------------------------------------------------
# <copyright file="handlers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.objects cimport Tick, BarType, Bar


cdef class Handler:
    """
    The base class for all handlers.
    """
    cdef readonly object handle


cdef class TickHandler(Handler):
    """
    Provides a handler for tick objects.
    """
    #cdef void (*_handler)(Tick tick)
    pass


cdef class BarHandler(Handler):
    """
    Provides a handler for bar type and bar objects.
    """
    pass


cdef class InstrumentHandler(Handler):
    """
    Provides a handler for instrument objects.
    """
    pass


cdef class EventHandler(Handler):
    """
    Provides a handler for event objects.
    """
    pass
