#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="requests.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.core.message cimport Message


cdef class Request(Message):
    """
    The base class for all requests.
    """
    pass
