#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="requests.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.core.message cimport Request
from inv_trader.c_enums.venue cimport Venue
from inv_trader.model.objects cimport Symbol, BarSpecification


cdef class TickDataRequest(Request):
    """
    Represents a request for historical tick data.
    """
    cdef readonly Symbol symbol
    cdef readonly datetime from_datetime
    cdef readonly datetime to_datetime


cdef class BarDataRequest(Request):
    """
    Represents a request for historical bar data.
    """
    cdef readonly Symbol symbol
    cdef readonly BarSpecification bar_spec
    cdef readonly datetime from_datetime
    cdef readonly datetime to_datetime


cdef class InstrumentRequest(Request):
    """
    Represents a request for an instrument.
    """
    cdef readonly Symbol symbol


cdef class InstrumentsRequest(Request):
    """
    Represents a request for all instruments for a venue.
    """
    cdef readonly Venue venue
