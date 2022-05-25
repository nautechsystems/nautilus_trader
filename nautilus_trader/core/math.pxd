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

from libc.stdint cimport uint64_t  # noqa (required for round_func_type)


ctypedef uint64_t (* round_func_type)(double x) nogil  # noqa E211 whitespace before '('

cdef round_func_type lround


cdef inline uint64_t min_uint64(uint64_t a, uint64_t b):
    if a < b:
        return a
    else:
        return b


cdef inline uint64_t max_uint64(uint64_t a, uint64_t b):
    if a > b:
        return a
    else:
        return b
