# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime

from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.instruments.base cimport Instrument


cdef class BettingInstrument(Instrument):
    cdef readonly int event_type_id
    cdef readonly str event_type_name
    cdef readonly int competition_id
    cdef readonly str competition_name
    cdef readonly int event_id
    cdef readonly str event_name
    cdef readonly str event_country_code
    cdef readonly datetime event_open_date
    cdef readonly str betting_type
    cdef readonly str market_id
    cdef readonly str market_name
    cdef readonly datetime market_start_time
    cdef readonly str market_type
    cdef readonly int selection_id
    cdef readonly str selection_name
    cdef readonly float selection_handicap

    @staticmethod
    cdef BettingInstrument from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(BettingInstrument obj)


cpdef double null_handicap()
cpdef object order_side_to_bet_side(OrderSide order_side)
