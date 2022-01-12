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


cdef class AssetTypeParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "SPOT"
        elif value == 2:
            return "SWAP"
        elif value == 3:
            return "FUTURE"
        elif value == 4:
            return "FORWARD"
        elif value == 5:
            return "CFD"
        elif value == 6:
            return "OPTION"
        elif value == 7:
            return "WARRANT"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef AssetType from_str(str value) except *:
        if value == "SPOT":
            return AssetType.SPOT
        elif value == "SWAP":
            return AssetType.SWAP
        elif value == "FUTURE":
            return AssetType.FUTURE
        elif value == "FORWARD":
            return AssetType.FORWARD
        elif value == "CFD":
            return AssetType.CFD
        elif value == "OPTION":
            return AssetType.OPTION
        elif value == "WARRANT":
            return AssetType.WARRANT
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return AssetTypeParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return AssetTypeParser.from_str(value)
