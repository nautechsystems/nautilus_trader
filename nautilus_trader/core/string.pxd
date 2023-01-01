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

from cpython.unicode cimport PyUnicode_AsUTF8String
from cpython.unicode cimport PyUnicode_FromString
from libc.stdint cimport uint8_t

from nautilus_trader.core.rust.core cimport cstr_free


cpdef uint8_t precision_from_str(str value) except *


cdef inline str cstr_to_pystr(const char* ptr):
    # Assumes `ptr` was created from Rust `CString::from_raw`,
    # otherwise will lead to undefined behaviour when passed to `cstr_free`.
    cdef str obj = PyUnicode_FromString(ptr)
    cstr_free(ptr)
    return obj


cdef inline const char* pystr_to_cstr(str value):
    cdef bytes utf8_bytes = PyUnicode_AsUTF8String(value)
    cdef char* cstr = utf8_bytes
    return cstr
