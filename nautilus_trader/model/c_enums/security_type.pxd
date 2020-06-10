# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------


cpdef enum SecurityType:
    UNDEFINED = 0,  # Invalid value
    FOREX = 1,
    BOND = 2,
    EQUITY = 3,
    FUTURE = 4,
    CFD = 5,
    OPTION = 6,
    CRYPTO = 7


cdef inline str security_type_to_string(int value):
    if value == 1:
        return 'FOREX'
    elif value == 2:
        return 'BOND'
    elif value == 3:
        return 'EQUITY'
    elif value == 4:
        return 'FUTURE'
    elif value == 5:
        return 'CFD'
    elif value == 6:
        return 'OPTION'
    elif value == 7:
        return 'CRYPTO'
    else:
        return 'UNDEFINED'


cdef inline SecurityType security_type_from_string(str value):
    if value == 'FOREX':
        return SecurityType.FOREX
    elif value == 'BOND':
        return SecurityType.BOND
    elif value == 'EQUITY':
        return SecurityType.EQUITY
    elif value == 'FUTURE':
        return SecurityType.FUTURE
    elif value == 'CFD':
        return SecurityType.CFD
    elif value == 'OPTION':
        return SecurityType.OPTION
    elif value == 'CRYPTO':
        return SecurityType.CRYPTO
    else:
        return SecurityType.UNDEFINED
