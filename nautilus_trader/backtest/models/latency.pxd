# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint64_t


cdef class LatencyModel:
    cdef readonly uint64_t base_latency_nanos
    """The default latency to the exchange.\n\n:returns: `int`"""
    cdef readonly uint64_t insert_latency_nanos
    """The latency (nanoseconds) for order insert messages to reach the exchange.\n\n:returns: `int`"""
    cdef readonly uint64_t update_latency_nanos
    """The latency (nanoseconds) for order update messages to reach the exchange.\n\n:returns: `int`"""
    cdef readonly uint64_t cancel_latency_nanos
    """The latency (nanoseconds) for order cancel messages to reach the exchange.\n\n:returns: `int`"""
