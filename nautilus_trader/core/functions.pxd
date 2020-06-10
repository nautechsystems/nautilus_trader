# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

cpdef double fast_round(double value, int precision)
cpdef double fast_mean(list values)
cpdef double fast_mean_iterated(
    list values,
    double next_value,
    double current_value,
    int expected_length,
    bint drop_left=*)
cpdef double basis_points_as_percentage(double basis_points)
cdef long get_size_of(obj)
cpdef str format_bytes(double size)
cpdef str pad_string(str string, int length, str pad=*)
