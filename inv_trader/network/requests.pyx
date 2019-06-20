#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="requests.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.core.message cimport Message
from inv_trader.model.identifiers cimport GUID


cdef class Request(Message):
    """
    The base class for all requests.
    """

    def __init__(self, GUID identifier, datetime timestamp):
        """
        Initializes a new instance of the Request abstract class.

        :param identifier: The request identifier.
        :param timestamp: The request timestamp.
        """
        super().__init__(identifier, timestamp)
