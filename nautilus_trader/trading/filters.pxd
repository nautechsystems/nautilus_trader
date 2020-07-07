# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime


cdef class ForexSessionFilter:
    cdef readonly object tz_sydney
    cdef readonly object tz_tokyo
    cdef readonly object tz_london
    cdef readonly object tz_new_york

    cpdef bint is_sydney_session(self, datetime time_now)
    cpdef bint is_tokyo_session(self, datetime time_now)
    cpdef bint is_london_session(self, datetime time_now)
    cpdef bint is_new_york_session(self, datetime time_now)
    cpdef datetime session_start(self, session, datetime datum)
    cpdef datetime session_end(self, session, datetime datum)


cdef class NewsEvent:
    cdef readonly datetime timestamp
    cdef readonly object impact
    cdef readonly str name
    cdef readonly str currency


cdef class EconomicNewsEventFilter:
    cdef object _news_data

    cdef readonly list currencies
    cdef readonly list impacts

    cpdef NewsEvent next_event(self, datetime time_now)
    cpdef NewsEvent prev_event(self, datetime time_now)
