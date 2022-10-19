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


cdef class TriggerTypeParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 0:
            return "NONE"
        elif value == 1:
            return "DEFAULT"
        elif value == 2:
            return "BID_ASK"
        elif value == 3:
            return "LAST"
        elif value == 4:
            return "DOUBLE_LAST"
        elif value == 5:
            return "DOUBLE_BID_ASK"
        elif value == 6:
            return "LAST_OR_BID_ASK"
        elif value == 7:
            return "MID_POINT"
        elif value == 8:
            return "MARK"
        elif value == 9:
            return "INDEX"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef TriggerType from_str(str value) except *:
        if value == "NONE":
            return TriggerType.NONE
        elif value == "DEFAULT":
            return TriggerType.DEFAULT
        elif value == "BID_ASK":
            return TriggerType.BID_ASK
        elif value == "LAST":
            return TriggerType.LAST
        elif value == "DOUBLE_LAST":
            return TriggerType.DOUBLE_LAST
        elif value == "DOUBLE_BID_ASK":
            return TriggerType.DOUBLE_BID_ASK
        elif value == "LAST_OR_BID_ASK":
            return TriggerType.LAST_OR_BID_ASK
        elif value == "MID_POINT":
            return TriggerType.MID_POINT
        elif value == "MARK":
            return TriggerType.MARK
        elif value == "INDEX":
            return TriggerType.INDEX
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return TriggerTypeParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return TriggerTypeParser.from_str(value)
