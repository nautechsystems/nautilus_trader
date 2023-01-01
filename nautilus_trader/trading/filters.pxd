# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.data cimport Data
from nautilus_trader.model.currency cimport Currency


cdef class ForexSessionFilter:
    cdef object _tz_sydney
    cdef object _tz_tokyo
    cdef object _tz_london
    cdef object _tz_new_york

    cpdef datetime local_from_utc(self, session, datetime time_now)
    cpdef datetime next_start(self, session, datetime time_now)
    cpdef datetime prev_start(self, session, datetime time_now)
    cpdef datetime next_end(self, session, datetime time_now)
    cpdef datetime prev_end(self, session, datetime time_now)


cdef class NewsEvent(Data):
    cdef readonly object impact
    """The expected news impact.\n\n:returns: `Enum`"""
    cdef readonly str name
    """The descriptive name of the news event.\n\n:returns: `str`"""
    cdef readonly Currency currency
    """The currency affected by the news event.\n\n:returns: `Currency`"""


cdef class EconomicNewsEventFilter:
    cdef object _news_data

    cdef datetime _unfiltered_data_start
    cdef datetime _unfiltered_data_end
    cdef list _currencies
    cdef list _impacts

    cpdef NewsEvent next_event(self, datetime time_now)
    cpdef NewsEvent prev_event(self, datetime time_now)
