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


cdef datetime UNIX_EPOCH


cpdef long to_posix_ms(datetime timestamp) except *
cpdef datetime from_posix_ms(long posix)
cpdef bint is_datetime_utc(datetime timestamp) except *
cpdef bint is_tz_aware(time_object) except *
cpdef bint is_tz_naive(time_object) except *
cpdef datetime as_utc_timestamp(datetime timestamp)
cpdef object as_utc_index(time_object)
cpdef str format_iso8601(datetime dt)
cpdef str format_iso8601_us(datetime dt)
