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

from cpython.object cimport PyObject
from cpython.ref cimport Py_XDECREF
from libc.stdint cimport uint8_t


cdef inline str pyobj_to_str(PyObject* ptr):
    cdef PyObject* str_obj = ptr
    cdef str str_value = <str>str_obj
    Py_XDECREF(str_obj)
    Py_XDECREF(ptr)
    return str_value


cpdef uint8_t precision_from_str(str value) except *
