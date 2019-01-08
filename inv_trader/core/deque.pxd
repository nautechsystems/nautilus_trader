#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="deque.pxd" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython cimport list


cdef class Deque(list):
    """
    Represents a double ended queue inheriting a c implemented list.
    """
    cdef readonly int maxlen

    cpdef void appendright(self, x)
    cpdef void appendleft(self, x)
    cpdef bint is_empty(self)


cdef class DequeDouble(list):
    """
    Represents a double ended queue strongly typed to handle double precision
    floating point numbers inheriting a c implemented list.
    """
    cdef readonly int maxlen

    cpdef void appendright(self, double x)
    cpdef void appendleft(self, double x)
    cpdef bint is_empty(self)
