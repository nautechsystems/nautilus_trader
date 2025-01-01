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

from nautilus_trader.serialization.base cimport Serializer


cdef class MsgSpecSerializer(Serializer):
    cdef object _encode
    cdef object _decode

    cdef readonly bint timestamps_as_str
    """If the serializer converts timestamp `int64_t` to integer strings.\n\n:returns: `bool`"""
    cdef readonly bint timestamps_as_iso8601
    """If the serializer converts timestamp `int64_t` to ISO 8601 strings.\n\n:returns: `bool`"""
