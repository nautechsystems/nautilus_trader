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


cdef extern from *:
    ctypedef unsigned long long int128 "__int128_t"


cdef enum SafeUUID:
    UNKNOWN = -1
    UNDEFINED = 0
    SAFE = 1
    UNSAFE = 2


cdef inline str safe_uuid_to_string(int value):
    if value == -1:
        return 'UNKNOWN'
    elif value == 1:
        return 'SAFE'
    elif value == 2:
        return 'UNSAFE'
    else:
        return 'UNDEFINED'


cdef inline SafeUUID safe_uuid_from_string(str value):
    if value == 'UNKNOWN':
        return SafeUUID.UNKNOWN
    elif value == 'SAFE':
        return SafeUUID.SAFE
    elif value == 'UNSAFE':
        return SafeUUID.UNSAFE
    else:
        return SafeUUID.UNDEFINED


cdef class UUID:
    cdef readonly object int_value
    cdef readonly str value
    cdef readonly SafeUUID is_safe

    cdef str _get_hex_string(self)


cpdef UUID uuid1(node=*, clock_seq=*)
cpdef UUID uuid3(UUID namespace_uuid, str name)
cpdef UUID uuid4()
cpdef UUID uuid5(UUID namespace_uuid, str name)
