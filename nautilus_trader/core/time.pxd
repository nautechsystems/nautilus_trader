"""
Cython implementation of (parts of) the standard library time module.
"""

from libc.stdint cimport int64_t

cdef extern from "pytime.h":
    ctypedef int64_t _PyTime_t
    _PyTime_t _PyTime_GetSystemClock() nogil
    double _PyTime_AsSecondsDouble(_PyTime_t t) nogil


cdef inline double unix_time() nogil:
    cdef:
        _PyTime_t tic

    tic = _PyTime_GetSystemClock()
    return _PyTime_AsSecondsDouble(tic)


cdef inline double unix_time_ms() nogil:
    cdef:
        _PyTime_t tic

    tic = _PyTime_GetSystemClock()
    return _PyTime_AsSecondsDouble(tic) * 1000
