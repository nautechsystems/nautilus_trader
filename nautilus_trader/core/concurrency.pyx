# -------------------------------------------------------------------------------------------------
# <copyright file="concurrency.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython cimport pythread
from cpython.exc cimport PyErr_NoMemory


cdef class FastRLock:
    """
    Provides a fast, re-entrant lock.

    Under un-congested conditions, the lock is never acquired but only
    counted.  Only when a second thread comes in and notices that the
    lock is needed, it acquires the lock and notifies the first thread
    to release it when it's done. This is made possible by the GIL.
    """

    def __cinit__(self):
        self._owner = -1
        self._count = 0
        self._is_locked = False
        self._pending_requests = 0
        self._real_lock = pythread.PyThread_allocate_lock()
        if self._real_lock is NULL:
            PyErr_NoMemory()

    def __dealloc__(self):
        if self._real_lock is not NULL:
            pythread.PyThread_free_lock(self._real_lock)
            self._real_lock = NULL

    cpdef bint acquire(self, bint blocking=True):
        return lock_lock(self, pythread.PyThread_get_thread_ident(), blocking)

    cpdef void release(self):
        if self._owner != pythread.PyThread_get_thread_ident():
            raise RuntimeError("cannot release un-acquired lock")
        unlock_lock(self)

    def __enter__(self):
        return lock_lock(self, pythread.PyThread_get_thread_ident(), True)

    def __exit__(self, t, v, tb):
        if self._owner != pythread.PyThread_get_thread_ident():
            raise RuntimeError("cannot release un-acquired lock")
        unlock_lock(self)

    cdef bint _is_owned(self):
        return self._owner == pythread.PyThread_get_thread_ident()


cdef inline bint lock_lock(FastRLock lock, long current_thread, bint blocking) nogil:
    # Note that this function *must* hold the GIL when being called.
    # We just use 'nogil' in the signature to make sure that no Python
    # code execution slips in that might free the GIL

    if lock._count:
        # locked! - by myself?
        if current_thread == lock._owner:
            lock._count += 1
            return 1
    elif not lock._pending_requests:
        # not locked, not requested - go!
        lock._owner = current_thread
        lock._count = 1
        return 1
    # need to get the real lock
    return _acquire_lock(
        lock, current_thread,
        pythread.WAIT_LOCK if blocking else pythread.NOWAIT_LOCK)

cdef bint _acquire_lock(FastRLock lock, long current_thread, int wait) nogil:
    # Note that this function *must* hold the GIL when being called.
    # We just use 'nogil' in the signature to make sure that no Python
    # code execution slips in that might free the GIL

    if not lock._is_locked and not lock._pending_requests:
        # someone owns it but didn't acquire the real lock - do that
        # now and tell the owner to release it when done. Note that we
        # do not release the GIL here as we must absolutely be the one
        # who acquires the lock now.
        if not pythread.PyThread_acquire_lock(lock._real_lock, wait):
            return 0
        #assert not lock._is_locked
        lock._is_locked = True
    lock._pending_requests += 1
    with nogil:
        # wait for the lock owning thread to release it
        locked = pythread.PyThread_acquire_lock(lock._real_lock, wait)
    lock._pending_requests -= 1
    #assert not lock._is_locked
    #assert lock._count == 0
    if not locked:
        return 0
    lock._is_locked = True
    lock._owner = current_thread
    lock._count = 1
    return 1

cdef inline void unlock_lock(FastRLock lock) nogil:
    # Note that this function *must* hold the GIL when being called.
    # We just use 'nogil' in the signature to make sure that no Python
    # code execution slips in that might free the GIL

    #assert lock._owner == pythread.PyThread_get_thread_ident()
    #assert lock._count > 0
    lock._count -= 1
    if lock._count == 0:
        lock._owner = -1
        if lock._is_locked:
            pythread.PyThread_release_lock(lock._real_lock)
            lock._is_locked = False
