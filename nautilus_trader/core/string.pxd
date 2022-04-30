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

from libc.stdint cimport uint8_t
from libc.stdint cimport uintptr_t

from nautilus_trader.core.rust.core cimport Buffer16
from nautilus_trader.core.rust.core cimport Buffer32
from nautilus_trader.core.rust.core cimport Buffer36
from nautilus_trader.core.rust.core cimport Buffer64
from nautilus_trader.core.rust.core cimport Buffer128


cdef inline Buffer16 pystr_to_buffer16(str value) except *:
    assert value is not None, "Invalid string: was `None`"
    cdef Buffer16 buffer
    cdef bytes data = value.encode()
    cdef uintptr_t length = len(data)
    assert length > 0, "Invalid empty string"
    assert length <= 16, f"Invalid string: length exceeded buffer capacity of {16}, was {length}"
    buffer.data = data + (16 - length) * b"\x00"
    buffer.len = length
    return buffer


cdef inline str buffer16_to_pystr(Buffer16 buffer):
    assert buffer is not None, "Buffer was `None`"
    # Copy decoded ASCII bytes from buffer
    cdef str value = buffer.data[:buffer.len].decode()
    return value


cdef inline Buffer32 pystr_to_buffer32(str value) except *:
    assert value is not None, "Invalid string: was `None`"
    cdef Buffer32 buffer
    cdef bytes data = value.encode()
    cdef uintptr_t length = len(data)
    assert length > 0, "Invalid empty string"
    assert length <= 32, f"Invalid string: length exceeded buffer capacity of {32}, was {length}"
    buffer.data = data + (32 - length) * b"\x00"
    buffer.len = length
    return buffer


cdef inline str buffer32_to_pystr(Buffer32 buffer):
    assert buffer is not None, "Buffer was `None`"
    # Copy decoded ASCII bytes from buffer
    cdef str value = buffer.data[:buffer.len].decode()
    return value


cdef inline Buffer36 pystr_to_buffer36(str value) except *:
    assert value is not None, "Invalid string: was `None`"
    cdef Buffer36 buffer
    cdef bytes data = value.encode()
    cdef uintptr_t length = len(data)
    assert length > 0, "Invalid empty string"
    assert length <= 36, f"Invalid string: length exceeded buffer capacity of {36}, was {length}"
    buffer.data = data + (36 - length) * b"\x00"
    buffer.len = length
    return buffer


cdef inline str buffer36_to_pystr(Buffer36 buffer):
    assert buffer is not None, "Buffer was `None`"
    # Copy decoded ASCII bytes from buffer
    cdef str value = buffer.data[:buffer.len].decode()
    return value


cdef inline Buffer64 pystr_to_buffer64(str value) except *:
    assert value is not None, "Invalid string: was `None`"
    cdef Buffer64 buffer
    cdef bytes data = value.encode()
    cdef uintptr_t length = len(data)
    assert length > 0, "Invalid empty string"
    assert length <= 64, f"Invalid string: length exceeded buffer capacity of {64}, was {length}"
    buffer.data = data + (64 - length) * b"\x00"
    buffer.len = length
    return buffer


cdef inline str buffer64_to_pystr(Buffer64 buffer):
    assert buffer is not None, "Buffer was `None`"
    # Copy decoded ASCII bytes from buffer
    cdef str value = buffer.data[:buffer.len].decode()
    return value


cdef inline Buffer128 pystr_to_buffer128(str value) except *:
    assert value is not None, "Invalid string: was `None`"
    cdef Buffer128 buffer
    cdef bytes data = value.encode()
    cdef uintptr_t length = len(data)
    assert length > 0, "Invalid empty string"
    assert length <= 128, f"Invalid string: length exceeded buffer capacity of {128}, was {length}"
    buffer.data = data + (128 - length) * b"\x00"
    buffer.len = length
    return buffer


cdef inline str buffer128_to_pystr(Buffer128 buffer):
    assert buffer is not None, "Buffer was `None`"
    # Copy decoded ASCII bytes from buffer
    cdef str value = buffer.data[:buffer.len].decode()
    return value


cpdef uint8_t precision_from_str(str value) except *
