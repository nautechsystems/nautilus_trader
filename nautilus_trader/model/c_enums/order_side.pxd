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


cpdef enum OrderSide:
    UNDEFINED = 0,  # Invalid value
    BUY = 1,
    SELL = 2


cdef inline str order_side_to_string(int value):
    if value == 1:
        return 'BUY'
    elif value == 2:
        return 'SELL'
    else:
        return 'UNDEFINED'


cdef inline OrderSide order_side_from_string(str value):
    if value == 'BUY':
        return OrderSide.BUY
    elif value == 'SELL':
        return OrderSide.SELL
    else:
        return OrderSide.UNDEFINED
