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


cdef class AggressorSideParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 0:
            return "NONE"
        elif value == 1:
            return "BUY"
        elif value == 2:
            return "SELL"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef AggressorSide from_str(str value) except *:
        if value == "NONE":
            return AggressorSide.NONE
        elif value == "BUY":
            return AggressorSide.BUY
        elif value == "SELL":
            return AggressorSide.SELL
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return AggressorSideParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return AggressorSideParser.from_str(value)
