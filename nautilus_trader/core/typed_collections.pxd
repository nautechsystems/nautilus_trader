# -------------------------------------------------------------------------------------------------
# <copyright file="typed_collections.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.concurrency cimport FastRLock


cdef class TypedList:
    cdef readonly type type_value

    cdef list _internal

    cpdef void append(self, x)
    cpdef void insert(self, int index, x)
    cpdef void remove(self, x)
    cpdef object pop(self)
    cpdef object pop_at(self, int index)
    cpdef void clear(self)
    cpdef int index(self, x, int start, int stop)
    cpdef int count(self, x)
    cpdef void sort(self, key=*, bint reverse=*)
    cpdef void reverse(self)
    cpdef TypedList copy(self)
    cpdef void extend(self)


cdef class TypedDictionary:
    cdef readonly type type_key
    cdef readonly type type_value

    cdef dict _internal

    cpdef object keys(self)
    cpdef object values(self)
    cpdef object items(self)
    cpdef object get(self, k, default=*)
    cpdef object setdefault(self, k, default=*)
    cpdef object pop(self, k, d=*)
    cpdef object popitem(self)
    cpdef dict copy(self)
    cpdef void clear(self)


cdef class ConcurrentList:
    cdef readonly type type_value

    cdef FastRLock _lock
    cdef TypedList _internal

    cpdef void append(self, x)
    cpdef void insert(self, int index, x)
    cpdef void remove(self, x)
    cpdef object pop(self)
    cpdef object pop_at(self, int index)
    cpdef void clear(self)
    cpdef int index(self, x, int start, int stop)
    cpdef int count(self, x)
    cpdef void sort(self, key=*, bint reverse=*)
    cpdef void reverse(self)
    cpdef ConcurrentList copy(self)
    cpdef void extend(self)


cdef class ConcurrentDictionary:
    cdef readonly type type_key
    cdef readonly type type_value

    cdef FastRLock _lock
    cdef TypedDictionary _internal

    cpdef object keys(self)
    cpdef object values(self)
    cpdef object items(self)
    cpdef object get(self, k, default=*)
    cpdef object setdefault(self, k, default=*)
    cpdef object pop(self, k, d=*)
    cpdef object popitem(self)
    cpdef dict copy(self)
    cpdef void clear(self)


cdef class ObjectCache:
    cdef readonly type type_key
    cdef readonly type type_value

    cdef ConcurrentDictionary _cache
    cdef object _parser

    cpdef object get(self, str key)
    cpdef void clear(self)
