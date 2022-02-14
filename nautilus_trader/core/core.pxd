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

from libc.stdint cimport int64_t


cdef extern from "nautilus_core.h":
    struct UUID4:
        pass

    double unix_timestamp()
    int64_t unix_timestamp_ms()
    int64_t unix_timestamp_us()
    int64_t unix_timestamp_ns()

    UUID4 uuid4_new()
    UUID4 uuid4_from_raw(const char *ptr)
    const char *uuid4_to_raw(UUID4 *ptr)
    void uuid4_free_raw(char *ptr)
    void uuid4_free(UUID4 uuid)
