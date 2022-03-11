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


cdef class TrailingOffsetTypeParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 0:
            return "NONE"
        elif value == 1:
            return "DEFAULT"
        elif value == 2:
            return "PRICE"
        elif value == 3:
            return "BASIS_POINTS"
        elif value == 4:
            return "TICKS"
        elif value == 5:
            return "PRICE_TIER"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef TrailingOffsetType from_str(str value) except *:
        if value == "NONE":
            return TrailingOffsetType.NONE
        elif value == "DEFAULT":
            return TrailingOffsetType.DEFAULT
        elif value == "PRICE":
            return TrailingOffsetType.PRICE
        elif value == "BASIS_POINTS":
            return TrailingOffsetType.BASIS_POINTS
        elif value == "TICKS":
            return TrailingOffsetType.TICKS
        elif value == "PRICE_TIER":
            return TrailingOffsetType.PRICE_TIER
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return TrailingOffsetTypeParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return TrailingOffsetTypeParser.from_str(value)
