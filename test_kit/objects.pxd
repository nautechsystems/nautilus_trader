#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False


cdef class ObjectStorer:
    cdef list _store
    cdef readonly int count

    cpdef list get_store(self)
    cpdef void store(self, object obj, bint print_storage=*)
    cpdef void store_2(self, object obj1, object obj2, bint print_storage=*)
