# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------


cpdef enum OrderType:
    UNDEFINED = 0,  # Invalid value
    MARKET = 1,
    LIMIT = 2,
    STOP = 3,
    STOP_LIMIT = 4,
    MIT = 5


cdef inline str order_type_to_string(int value):
    if value == 1:
        return 'MARKET'
    elif value == 2:
        return 'LIMIT'
    elif value == 3:
        return 'STOP'
    elif value == 4:
        return 'STOP_LIMIT'
    elif value == 5:
        return 'MIT'
    else:
        return 'UNDEFINED'


cdef inline OrderType order_type_from_string(str value):
    if value == 'MARKET':
        return OrderType.MARKET
    elif value == 'LIMIT':
        return OrderType.LIMIT
    elif value == 'STOP':
        return OrderType.STOP
    elif value == 'STOP_LIMIT':
        return OrderType.STOP_LIMIT
    elif value == 'MIT':
        return OrderType.MIT
    else:
        return OrderType.UNDEFINED
