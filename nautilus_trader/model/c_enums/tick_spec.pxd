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


cpdef enum TickSpecification:
    UNDEFINED = 0,  # Invalid value
    QUOTE = 1,
    TRADE = 2,
    OPEN_INTEREST = 3,


cdef inline str tick_spec_to_string(int value):
    if value == 1:
        return 'QUOTE'
    elif value == 2:
        return 'TRADE'
    elif value == 3:
        return 'OPEN_INTEREST'
    else:
        return 'UNDEFINED'


cdef inline TickSpecification tick_spec_from_string(str value):
    if value == 'QUOTE':
        return TickSpecification.QUOTE
    elif value == 'TRADE':
        return TickSpecification.TRADE
    elif value == 'OPEN_INTEREST':
        return TickSpecification.OPEN_INTEREST
    else:
        return TickSpecification.UNDEFINED
