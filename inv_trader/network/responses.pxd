#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="responses.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.core.message cimport Message
from inv_trader.model.objects cimport Symbol, BarSpecification
from inv_trader.model.identifiers cimport GUID


cdef class Response(Message):
    """
    The base class for all responses.
    """
    cdef readonly GUID correlation_id


cdef class TickDataResponse(Response):
    """
    Represents a response of historical tick data.
    """
    cdef readonly Symbol symbol
    cdef readonly bytes[:] ticks


cdef class BarDataResponse(Response):
    """
    Represents a response of historical bar data.
    """
    cdef readonly Symbol symbol
    cdef readonly BarSpecification bar_spec
    cdef readonly bytes[:] bars


cdef class InstrumentResponse(Response):
    """
    Represents a response of instrument data.
    """
    cdef readonly bytes[:] instruments
