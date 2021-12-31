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


cdef class VenueTypeParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "EXCHANGE"
        elif value == 2:
            return "ECN"
        elif value == 3:
            return "BROKERAGE"
        elif value == 4:
            return "BROKERAGE_MULTI_VENUE"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef VenueType from_str(str value) except *:
        if value == "EXCHANGE":
            return VenueType.EXCHANGE
        elif value == "ECN":
            return VenueType.ECN
        elif value == "BROKERAGE":
            return VenueType.BROKERAGE
        elif value == "BROKERAGE_MULTI_VENUE":
            return VenueType.BROKERAGE_MULTI_VENUE
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return VenueTypeParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return VenueTypeParser.from_str(value)
