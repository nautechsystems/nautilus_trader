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


cdef class VenueStatusParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "CLOSED"
        elif value == 2:
            return "PRE_OPEN"
        elif value == 3:
            return "OPEN"
        elif value == 4:
            return "PAUSE"
        elif value == 5:
            return "PRE_CLOSE"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef VenueStatus from_str(str value) except *:
        if value == "CLOSED":
            return VenueStatus.CLOSED
        elif value == "PRE_OPEN":
            return VenueStatus.PRE_OPEN
        if value == "OPEN":
            return VenueStatus.OPEN
        if value == "PAUSE":
            return VenueStatus.PAUSE
        if value == "PRE_CLOSE":
            return VenueStatus.PRE_CLOSE
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return VenueStatusParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return VenueStatusParser.from_str(value)
