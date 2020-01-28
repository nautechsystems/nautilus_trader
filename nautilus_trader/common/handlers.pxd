# -------------------------------------------------------------------------------------------------
# <copyright file="handlers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.objects cimport Tick, BarType, Bar, Instrument
from nautilus_trader.core.message cimport Event

# cdef void (*_handler)(Tick tick)

cdef class Handler:
    cdef readonly object handle


cdef class TickHandler(Handler):
    cdef void handle(self, Tick tick) except *


cdef class BarHandler(Handler):
    cdef void handle(self, BarType bar_type, Bar bar)  except *


cdef class InstrumentHandler(Handler):
    cdef void handle(self, Instrument instrument)  except *


cdef class EventHandler(Handler):
    cdef void handle(self, Event event)  except *
