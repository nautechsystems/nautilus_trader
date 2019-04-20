#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="handlers.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.model.objects cimport Tick, BarType, Bar


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


cdef class EventHandler(Handler):
    """
    Provides a handler for event objects.
    """
    pass
