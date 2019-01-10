#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="trader.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from cpython.datetime cimport datetime
from inv_trader.common.clock cimport Clock
from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.model.identifiers cimport Label, GUID


cdef class Trader:
    """
    Represents a trader for managing a portfolio of trade strategies.
    """
    cdef Clock _clock
    cdef LoggerAdapter _log

    cdef readonly Label name
    cdef readonly GUID id
    cdef readonly list strategies
    cdef readonly list started_datetimes
    cdef readonly list stopped_datetimes


