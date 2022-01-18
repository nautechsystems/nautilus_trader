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


cdef class TriggerMethodParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 0:
            return "DEFAULT"
        elif value == 1:
            return "LAST"
        elif value == 2:
            return "BID_ASK"
        elif value == 3:
            return "DOUBLE_LAST"
        elif value == 4:
            return "DOUBLE_BID_ASK"
        elif value == 5:
            return "LAST_OR_BID_ASK"
        elif value == 6:
            return "MID_POINT"
        elif value == 7:
            return "MARK"
        elif value == 8:
            return "INDEX"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef TriggerMethod from_str(str value) except *:
        if value == "DEFAULT":
            return TriggerMethod.DEFAULT
        elif value == "LAST":
            return TriggerMethod.LAST
        elif value == "BID_ASK":
            return TriggerMethod.BID_ASK
        elif value == "DOUBLE_LAST":
            return TriggerMethod.DOUBLE_LAST
        elif value == "DOUBLE_BID_ASK":
            return TriggerMethod.DOUBLE_BID_ASK
        elif value == "LAST_OR_BID_ASK":
            return TriggerMethod.LAST_OR_BID_ASK
        elif value == "MID_POINT":
            return TriggerMethod.MID_POINT
        elif value == "MARK":
            return TriggerMethod.MARK
        elif value == "INDEX":
            return TriggerMethod.INDEX
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return TriggerMethodParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return TriggerMethodParser.from_str(value)
