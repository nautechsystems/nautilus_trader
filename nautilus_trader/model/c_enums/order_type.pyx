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

cdef class OrderTypeParser:

    @staticmethod
    cdef str to_string(int value):
        if value == 1:
            return 'MARKET'
        elif value == 2:
            return 'LIMIT'
        elif value == 3:
            return 'STOP_MARKET'
        else:
            return 'UNDEFINED'

    @staticmethod
    cdef OrderType from_string(str value):
        if value == 'MARKET':
            return OrderType.MARKET
        elif value == 'LIMIT':
            return OrderType.LIMIT
        elif value == 'STOP_MARKET':
            return OrderType.STOP_MARKET
        else:
            return OrderType.UNDEFINED

    @staticmethod
    def to_string_py(int value):
        return OrderTypeParser.to_string(value)

    @staticmethod
    def from_string_py(str value):
        return OrderTypeParser.from_string(value)
