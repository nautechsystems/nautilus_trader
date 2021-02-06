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

from libc.stdint cimport uint32_t
from libc.stdint cimport uint64_t


cdef extern from "lib/nautilus-core/nautilus_core.h":
    ctypedef struct DateTime:
        uint32_t year
        uint32_t month
        uint32_t day
        uint32_t hour
        uint32_t minute
        uint32_t second
        uint32_t microsecond

    DateTime c_utc_now()
    double c_timestamp()
    uint64_t c_timestamp_ms()
    uint64_t c_timestamp_us()

    char* c_uuid_str_new()
    void c_uuid_str_free(char *s)
