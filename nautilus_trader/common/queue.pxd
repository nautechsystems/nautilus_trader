# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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


cdef class Queue:
    cdef object _queue

    cdef readonly int maxsize
    """The maximum capacity of the queue before blocking.\n\n:returns: `int`"""
    cdef readonly int count
    """The current count of items on the queue.\n\n:returns: `int`"""

    cpdef int qsize(self) except *
    cpdef bint empty(self) except *
    cpdef bint full(self) except *
    cpdef void put_nowait(self, item) except *
    cpdef object get_nowait(self)
    cpdef object peek_back(self)
    cpdef object peek_front(self)
    cpdef object peek_index(self, int index)
    cpdef list to_list(self)

    cdef int _qsize(self) except *
    cdef bint _empty(self) except *
    cdef bint _full(self) except *
    cdef void _put_nowait(self, item) except *
    cdef object _get_nowait(self)
