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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport int64_t


cpdef int64_t secs_to_nanos(double seconds) except *
cpdef int64_t millis_to_nanos(double millis) except *
cpdef int64_t micros_to_nanos(double micros) except *
cpdef double nanos_to_secs(double nanos) except *
cpdef int64_t nanos_to_millis(int64_t nanos) except *
cpdef int64_t nanos_to_micros(int64_t nanos) except *
cpdef int64_t dt_to_unix_millis(datetime dt) except *
cpdef int64_t dt_to_unix_micros(datetime dt) except *
cpdef int64_t dt_to_unix_nanos(datetime dt) except *
cpdef int64_t timedelta_to_nanos(timedelta delta) except *
cpdef timedelta nanos_to_timedelta(int64_t nanos)
cpdef datetime nanos_to_unix_dt(double nanos)
cpdef maybe_dt_to_unix_nanos(datetime dt)
cpdef maybe_nanos_to_unix_dt(nanos)
cpdef bint is_datetime_utc(datetime dt) except *
cpdef bint is_tz_aware(time_object) except *
cpdef bint is_tz_naive(time_object) except *
cpdef datetime as_utc_timestamp(datetime dt)
cpdef object as_utc_index(time_object)
cpdef str format_iso8601(datetime dt)
cpdef str format_iso8601_us(datetime dt)
cpdef int64_t iso8601_to_unix_millis(str iso8601) except *
cpdef int64_t iso8601_to_unix_micros(str iso8601) except *
cpdef int64_t iso8601_to_unix_nanos(str iso8601) except *
