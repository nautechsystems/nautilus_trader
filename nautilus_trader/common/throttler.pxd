# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport timedelta

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.common.timer cimport TimeEvent


cdef class Throttler:
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef Queue _queue
    cdef int _limit
    cdef int _vouchers
    cdef str _token
    cdef timedelta _interval
    cdef object _output

    cdef readonly str name
    """The name of the throttler.\n\n:returns: `str`"""
    cdef readonly bint is_active
    """If the throttler is actively timing.\n\n:returns: `bool`"""
    cdef readonly bint is_throttling
    """If the throttler is currently throttling items.\n\n:returns: `bool`"""

    cpdef void send(self, item) except *
    cpdef void _process_queue(self) except *
    cpdef void _refresh_vouchers(self, TimeEvent event) except *
    cdef void _run_timer(self) except *
