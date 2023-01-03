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

from nautilus_trader.core.rust.core cimport cstr_free


cdef extern from "Python.h":
    # Similar to PyUnicode_FromUnicode(), but u points to null-terminated
    # UTF-8 encoded bytes. The size is determined with strlen().
    unicode PyUnicode_FromString(const char *u)  # noqa

    # Return a pointer to the UTF-8 encoding of the Unicode object,
    # and store the size of the encoded representation (in bytes) in size.
    # The size argument can be NULL; in this case no size will be stored.
    # The returned buffer always has an extra null byte appended
    # (not included in size), regardless of whether there are any
    # other null code points.

    # In the case of an error, NULL is returned with an exception set and
    # no size is stored.

    # This caches the UTF-8 representation of the string in the Unicode
    # object, and subsequent calls will return a pointer to the same buffer.
    # The caller is not responsible for deallocating the buffer
    const char* PyUnicode_AsUTF8AndSize(object unicode, Py_ssize_t *size)  # noqa


cdef inline str cstr_to_pystr(const char* ptr):
    cdef str obj = PyUnicode_FromString(ptr)

    # Assumes `ptr` was created from Rust `CString::from_raw`,
    # otherwise will lead to undefined behaviour when passed to `cstr_free`.
    cstr_free(ptr)
    return obj


cdef inline const char* pystr_to_cstr(str value) except *:
    return PyUnicode_AsUTF8AndSize(value, NULL)
