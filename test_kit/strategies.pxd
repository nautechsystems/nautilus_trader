# -------------------------------------------------------------------------------------------------
# <copyright file="strategies.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.events cimport Event
from nautilus_trader.model.objects cimport Tick, BarType, Bar
from nautilus_trader.trade.strategy cimport TradeStrategy


cdef class EmptyStrategyCython(TradeStrategy):
    """
    A Cython strategy which is empty and does nothing.
    """
    cpdef on_start(self)
    cpdef on_tick(self, Tick tick)
    cpdef on_bar(self, BarType bar_type, Bar bar)
    cpdef on_event(self, Event event)
    cpdef on_stop(self)
    cpdef on_reset(self)
