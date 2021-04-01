# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

cdef class AssetClassParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "FX"
        elif value == 2:
            return "STOCK"
        elif value == 3:
            return "COMMODITY"
        elif value == 4:
            return "BOND"
        elif value == 5:
            return "INDEX"
        elif value == 6:
            return "CRYPTO"
        elif value == 7:
            return "BETTING"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef AssetClass from_str(str value) except *:
        if value == "FX":
            return AssetClass.FX
        elif value == "STOCK":
            return AssetClass.STOCK
        elif value == "COMMODITY":
            return AssetClass.COMMODITY
        elif value == "BOND":
            return AssetClass.BOND
        elif value == "INDEX":
            return AssetClass.INDEX
        elif value == "CRYPTO":
            return AssetClass.CRYPTO
        elif value == "BETTING":
            return AssetClass.BETTING
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return AssetClassParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return AssetClassParser.from_str(value)
