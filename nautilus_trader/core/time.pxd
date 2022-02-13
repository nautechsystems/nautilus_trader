# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

"""
Cython implementation of (parts of) the standard library time module.
"""

from libc.stdint cimport int64_t


cdef extern from "pytime.h":
    ctypedef int64_t _PyTime_t
    ctypedef int _PyTime_round_t
    _PyTime_t _PyTime_GetSystemClock() nogil
    double _PyTime_AsSecondsDouble(_PyTime_t t) nogil
    _PyTime_t _PyTime_AsMilliseconds(_PyTime_t t, _PyTime_round_t round) nogil
    _PyTime_t _PyTime_AsMicroseconds(_PyTime_t t, _PyTime_round_t round) nogil


cdef inline double unix_timestamp() nogil:
    cdef _PyTime_t tic = _PyTime_GetSystemClock()
    return _PyTime_AsSecondsDouble(tic)


cdef inline int64_t unix_timestamp_ms() nogil:
    cdef _PyTime_t tic = _PyTime_GetSystemClock()
    # _PyTime_ROUND_UP=3 should be used for timeouts
    return _PyTime_AsMilliseconds(tic, 3)


cdef inline int64_t unix_timestamp_us() nogil:
    cdef _PyTime_t tic = _PyTime_GetSystemClock()
    # _PyTime_ROUND_UP=3 should be used for timeouts
    return _PyTime_AsMicroseconds(tic, 3)


cdef inline int64_t unix_timestamp_ns() nogil:
    return _PyTime_GetSystemClock()
