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


cpdef enum LiquiditySide:
    NONE = 0,
    MAKER = 1,
    TAKER = 2,


cdef inline str liquidity_side_to_string(int value):
    if value == 1:
        return 'MAKER'
    elif value == 2:
        return 'TAKER'
    else:
        return 'NONE'


cdef inline LiquiditySide liquidity_side_from_string(str value):
    if value == 'MAKER':
        return LiquiditySide.MAKER
    elif value == 'TAKER':
        return LiquiditySide.TAKER
    else:
        return LiquiditySide.NONE
