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


cdef class DataResponse(Response):
    """
    Represents a response of data.
    """
    cdef readonly str data_type
    cdef readonly str data_encoding
    cdef readonly bytes data
