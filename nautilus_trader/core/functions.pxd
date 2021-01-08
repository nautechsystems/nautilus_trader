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

import pandas as pd


cpdef bint is_ge_python_version(int major, int minor)
cpdef double fast_mean(list values) except *
cpdef double fast_mean_iterated(
    list values,
    double next_value,
    double current_value,
    int expected_length,
    bint drop_left=*,
) except *
cpdef double fast_std(list values) except *
cpdef double fast_std_with_mean(list values, double mean) except *
cpdef double basis_points_as_percentage(double basis_points) except *
cdef long get_size_of(obj) except *
cpdef str format_bytes(double size)
cpdef str pad_string(str string, int final_length, str pad=*)


cdef inline object slice_dataframe(dataframe, start, end):
    # Slice the dataframe with the given start and end.
    # Method only exists due to cython limitation compiling closures.
    if dataframe is None:
        return pd.DataFrame()

    return dataframe[start:end]
