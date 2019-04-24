#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategies.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from inv_trader.model.events cimport Event
from inv_trader.model.objects cimport Tick, BarType, Bar
from inv_trader.strategy cimport TradeStrategy


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
