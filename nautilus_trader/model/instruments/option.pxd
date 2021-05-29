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

from nautilus_trader.model.instruments.base cimport Instrument


cdef class Option(Instrument):
    cdef readonly int contract_id
    cdef readonly str last_trade_date_or_contract_month
    cdef readonly str local_symbol
    cdef readonly str trading_class
    cdef readonly str market_name
    cdef readonly str long_name
    cdef readonly str contract_month
    cdef readonly str time_zone_id
    cdef readonly str trading_hours
    cdef readonly str liquid_hours
    cdef readonly str last_trade_time
