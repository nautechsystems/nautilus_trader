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


cdef class Data:
    cdef readonly datetime timestamp
    """The data timestamp (UTC).\n\n:returns: `datetime`"""
    cdef readonly double unix_timestamp
    """The data Unix timestamp (seconds).\n\n:returns: `double`"""


cdef class DataType:
    cdef frozenset _key
    cdef int _hash

    cdef readonly type type
    """The PyObject type of the data.\n\n:returns: `type`"""
    cdef readonly dict metadata
    """The data types metadata.\n\n:returns: `set[str, object]`"""


cdef class GenericData(Data):
    cdef readonly DataType data_type
    """The data type for the data.\n\n:returns: `DataType`"""
    cdef readonly object data
    """The data.\n\n:returns: `object`"""
