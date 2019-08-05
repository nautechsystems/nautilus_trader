# -------------------------------------------------------------------------------------------------
# <copyright file="concurrency.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython cimport pythread


cdef class FastRLock:
    """
    Provides a fast, re-entrant lock.

    Under un-congested conditions, the lock is never acquired but only
    counted.  Only when a second thread comes in and notices that the
    lock is needed, it acquires the lock and notifies the first thread
    to release it when it's done. This is made possible by the GIL.
    """
    cdef pythread.PyThread_type_lock _real_lock
    cdef long _owner            # ID of thread owning the lock
    cdef int _count             # Re-entry count
    cdef int _pending_requests  # Number of pending requests for real lock
    cdef bint _is_locked        # Whether the real lock is acquired

    cpdef bint acquire(self, bint blocking=*)
    cpdef void release(self)
    cdef bint _is_owned(self)


cdef class ConcurrentDictionary:
    """
    Provides a thread safe wrapper to a standard python dictionary.
    """
    cdef FastRLock _lock
    cdef dict _internal

    cpdef object keys(self)
    cpdef object values(self)
    cpdef object get(self, k, default=*)
    cpdef object setdefault(self, k, default=*)
    cpdef object pop(self, k, d=*)


cdef class ObjectCache:
    """
    Provides a generic object cache with strings as keys.
    """
    cdef ConcurrentDictionary _cache
    cdef object _parser

    cpdef object get(self, str key)
