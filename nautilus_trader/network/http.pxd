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

from nautilus_trader.common.logging cimport LoggerAdapter


cdef class HttpClient:
    cdef readonly object _loop
    cdef readonly LoggerAdapter _log

    cdef list _addresses
    cdef list _nameservers
    cdef int _ttl_dns_cache
    cdef object _ssl
    cdef dict _connector_kwargs
    cdef list _sessions
    cdef int _sessions_idx
    cdef int _sessions_len

    cdef object _get_session(self)
    cpdef str _prepare_params(self, dict params)
