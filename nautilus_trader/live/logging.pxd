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

from nautilus_trader.common.logging cimport LogMessage, Logger
from nautilus_trader.serialization.base cimport LogSerializer


cdef class LogStore:
    cdef str _key
    cdef object _queue
    cdef object _process
    cdef object _redis
    cdef LogSerializer _serializer

    cpdef void store(self, LogMessage message)
    cpdef void _consume_messages(self) except *


cdef class LiveLogger(Logger):
    cdef object _queue
    cdef object _thread
    cdef LogStore _store

    cpdef void _consume_messages(self) except *
