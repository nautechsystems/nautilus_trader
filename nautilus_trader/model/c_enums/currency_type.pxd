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


cpdef enum CurrencyType:
    UNDEFINED = 0,
    CRYPTO = 1,
    FIAT = 2,


cdef inline str currency_type_to_string(int value):
    if value == 1:
        return 'CRYPTO'
    elif value == 2:
        return 'FIAT'
    else:
        return 'UNDEFINED'


cdef inline CurrencyType currency_type_from_string(str value):
    if value == 'CRYPTO':
        return CurrencyType.CRYPTO
    elif value == 'FIAT':
        return CurrencyType.FIAT
    else:
        return CurrencyType.UNDEFINED
