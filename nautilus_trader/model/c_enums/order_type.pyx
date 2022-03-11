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


cdef class OrderTypeParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "MARKET"
        elif value == 2:
            return "LIMIT"
        elif value == 3:
            return "STOP_MARKET"
        elif value == 4:
            return "STOP_LIMIT"
        elif value == 5:
            return "MARKET_TO_LIMIT"
        elif value == 6:
            return "MARKET_IF_TOUCHED"
        elif value == 7:
            return "LIMIT_IF_TOUCHED"
        elif value == 8:
            return "TRAILING_STOP_MARKET"
        elif value == 9:
            return "TRAILING_STOP_LIMIT"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef OrderType from_str(str value) except *:
        if value == "MARKET":
            return OrderType.MARKET
        elif value == "LIMIT":
            return OrderType.LIMIT
        elif value == "STOP_MARKET":
            return OrderType.STOP_MARKET
        elif value == "STOP_LIMIT":
            return OrderType.STOP_LIMIT
        elif value == "MARKET_TO_LIMIT":
            return OrderType.MARKET_TO_LIMIT
        elif value == "MARKET_IF_TOUCHED":
            return OrderType.MARKET_IF_TOUCHED
        elif value == "LIMIT_IF_TOUCHED":
            return OrderType.LIMIT_IF_TOUCHED
        elif value == "TRAILING_STOP_MARKET":
            return OrderType.TRAILING_STOP_MARKET
        elif value == "TRAILING_STOP_LIMIT":
            return OrderType.TRAILING_STOP_LIMIT
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return OrderTypeParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return OrderTypeParser.from_str(value)
