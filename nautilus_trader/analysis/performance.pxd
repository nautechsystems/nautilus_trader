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

from cpython.datetime cimport datetime

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.common.account cimport Account


cdef class PerformanceAnalyzer:
    cdef Money _account_starting_capital
    cdef Money _account_capital
    cdef Currency _account_currency
    cdef object _returns
    cdef object _positions
    cdef object _transactions

    cpdef void calculate_statistics(self, Account account, dict positions) except *
    cpdef void add_transaction(self, AccountStateEvent event) except *
    cpdef void add_return(self, datetime time, double value) except *
    cpdef void add_positions(self, datetime time, list positions, Money cash_balance) except *
    cpdef void reset(self) except *
    cpdef object get_returns(self)
    cpdef object get_positions(self)
    cpdef object get_transactions(self)
    cpdef object get_equity_curve(self)
    cpdef double total_pnl(self)
    cpdef double total_pnl_percentage(self)
    cpdef double max_winner(self)
    cpdef double max_loser(self)
    cpdef double min_winner(self)
    cpdef double min_loser(self)
    cpdef double avg_winner(self)
    cpdef double avg_loser(self)
    cpdef double win_rate(self)
    cpdef double expectancy(self)
    cpdef double annual_return(self)
    cpdef double cum_return(self)
    cpdef double max_drawdown_return(self)
    cpdef double annual_volatility(self)
    cpdef double sharpe_ratio(self)
    cpdef double calmar_ratio(self)
    cpdef double sortino_ratio(self)
    cpdef double omega_ratio(self)
    cpdef double stability_of_timeseries(self)
    cpdef double returns_mean(self)
    cpdef double returns_variance(self)
    cpdef double returns_skew(self)
    cpdef double returns_kurtosis(self)
    cpdef double returns_tail_ratio(self)
    cpdef double alpha(self)
    cpdef double beta(self)
    cpdef dict get_performance_stats(self)

    cdef list get_performance_stats_formatted(self, Currency account_currency)
