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


cpdef enum AssetType:
    UNDEFINED = 0,  # Invalid value
    SPOT = 1,
    SWAP = 2,
    FUTURE = 3,
    FORWARD = 4,
    CFD = 5,
    OPTION = 6,


cdef inline str asset_type_to_string(int value):
    if value == 1:
        return 'SPOT'
    elif value == 2:
        return 'SWAP'
    elif value == 3:
        return 'FUTURE'
    elif value == 4:
        return 'FORWARD'
    elif value == 5:
        return 'CFD'
    elif value == 6:
        return 'OPTION'
    else:
        return 'UNDEFINED'


cdef inline AssetType asset_type_from_string(str value):
    if value == 'SPOT':
        return AssetType.SPOT
    elif value == 'SWAP':
        return AssetType.SWAP
    elif value == 'FUTURE':
        return AssetType.FUTURE
    elif value == 'FORWARD':
        return AssetType.FORWARD
    elif value == 'CFD':
        return AssetType.CFD
    elif value == 'OPTION':
        return AssetType.OPTION
    else:
        return AssetType.UNDEFINED
