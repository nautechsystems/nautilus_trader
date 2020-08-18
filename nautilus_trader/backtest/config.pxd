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

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.objects cimport Money

cdef class BacktestConfig:
    cdef readonly int tick_capacity
    cdef readonly int bar_capacity
    cdef readonly str exec_db_type
    cdef readonly bint exec_db_flush
    cdef readonly bint frozen_account
    cdef readonly Money starting_capital
    cdef readonly Currency account_currency
    cdef readonly str short_term_interest_csv_path
    cdef readonly double commission_rate_bp
    cdef readonly bint bypass_logging
    cdef readonly int level_console
    cdef readonly int level_file
    cdef readonly int level_store
    cdef readonly bint console_prints
    cdef readonly bint log_thread
    cdef readonly bint log_to_file
    cdef readonly str log_file_path
