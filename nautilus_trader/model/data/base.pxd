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

from nautilus_trader.core.data cimport Data


cdef class DataType:
    cdef frozenset _key
    cdef int _hash
    cdef str _metadata_str

    cdef readonly type type
    """The `Data` type of the data.\n\n:returns: `type`"""
    cdef readonly dict metadata
    """The data types metadata.\n\n:returns: `dict[str, object]`"""
    cdef readonly str topic
    """The data types topic string.\n\n:returns: `str`"""


cdef class GenericData(Data):
    cdef readonly DataType data_type
    """The data type.\n\n:returns: `DataType`"""
    cdef readonly Data data
    """The data.\n\n:returns: `Data`"""
