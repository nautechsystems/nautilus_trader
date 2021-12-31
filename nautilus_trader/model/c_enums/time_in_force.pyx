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


cdef class TimeInForceParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "DAY"
        elif value == 2:
            return "GTC"
        elif value == 3:
            return "IOC"
        elif value == 4:
            return "FOK"
        elif value == 5:
            return "FAK"
        elif value == 6:
            return "GTD"
        elif value == 7:
            return "OC"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef TimeInForce from_str(str value) except *:
        if value == "DAY":
            return TimeInForce.DAY
        elif value == "GTC":
            return TimeInForce.GTC
        elif value == "IOC":
            return TimeInForce.IOC
        elif value == "FOK":
            return TimeInForce.FOK
        elif value == "FAK":
            return TimeInForce.FAK
        elif value == "GTD":
            return TimeInForce.GTD
        elif value == "OC":
            return TimeInForce.OC
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return TimeInForceParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return TimeInForceParser.from_str(value)
