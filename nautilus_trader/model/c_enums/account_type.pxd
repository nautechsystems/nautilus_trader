# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------


cpdef enum AccountType:
    UNDEFINED = 0,  # Invalid value
    SIMULATED = 1,
    DEMO = 2,
    REAL = 3,


cdef inline str account_type_to_string(int value):
    if value == 1:
        return 'SIMULATED'
    elif value == 2:
        return 'DEMO'
    elif value == 3:
        return 'REAL'
    else:
        return 'UNDEFINED'


cdef inline AccountType account_type_from_string(str value):
    if value == 'SIMULATED':
        return AccountType.SIMULATED
    elif value == 'DEMO':
        return AccountType.DEMO
    elif value == 'REAL':
        return AccountType.REAL
    else:
        return AccountType.UNDEFINED
