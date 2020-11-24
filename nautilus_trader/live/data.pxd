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

from nautilus_trader.data.engine cimport DataEngine


cdef class LiveDataEngine(DataEngine):
    cdef object _data_thread
    cdef object _data_queue
    cdef object _message_thread
    cdef object _message_queue
    cdef bint _is_running

    cpdef int data_qsize(self) except *
    cpdef int message_qsize(self) except *

    cpdef void _process_data_queue(self) except *
    cpdef void _process_message_queue(self) except *
