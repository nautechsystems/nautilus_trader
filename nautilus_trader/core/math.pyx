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

from libc.math cimport llround as llround_func
from libc.math cimport lround as lround_func


# Determine correct C lround function
cdef round_func_type _get_round_func() except *:
    if sizeof(long) == 8:
        return <round_func_type>lround_func
    elif sizeof(long long) == 8:
        return <round_func_type>llround_func
    else:
        raise TypeError(f"Can't support 'C' lround function.")

lround = _get_round_func()
