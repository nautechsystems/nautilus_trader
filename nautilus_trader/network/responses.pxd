#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="responses.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from nautilus_trader.core.message cimport Response
from nautilus_trader.model.objects cimport Symbol, Tick, BarSpecification, Bar, Instrument


cdef class TickDataResponse(Response):
    """
    Represents a response of historical tick data.
    """
    cdef readonly Symbol symbol
    cdef readonly Tick[:] ticks


cdef class BarDataResponse(Response):
    """
    Represents a response of historical bar data.
    """
    cdef readonly Symbol symbol
    cdef readonly BarSpecification bar_spec
    cdef readonly Bar[:] bars


cdef class InstrumentResponse(Response):
    """
    Represents a response of instrument data.
    """
    cdef readonly Instrument[:] instruments
