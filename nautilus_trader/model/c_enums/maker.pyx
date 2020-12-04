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

cdef class MakerParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "BUYER"
        elif value == 2:
            return "SELLER"
        else:
            return "UNDEFINED"

    @staticmethod
    cdef Maker from_str(str value):
        if value == "BUYER":
            return Maker.BUYER
        elif value == "SELLER":
            return Maker.SELLER
        else:
            return Maker.UNDEFINED

    @staticmethod
    def to_str_py(int value):
        return MakerParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return MakerParser.from_str(value)
