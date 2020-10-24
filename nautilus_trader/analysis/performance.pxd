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

from cpython.datetime cimport datetime

from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.trading.account cimport Account


cdef class PerformanceAnalyzer:
    cdef Money _account_starting_balance
    cdef Money _account_balance
    cdef Currency _account_currency
    cdef object _daily_returns
    cdef object _realized_pnls

    cpdef void calculate_statistics(self, Account account, list positions) except *
    cpdef void add_positions(self, list positions) except *
    cpdef void add_trade(self, PositionId position_id, Money realized_pnl) except *
    cpdef void add_return(self, datetime timestamp, double value) except *
    cpdef void reset(self) except *
    cpdef object get_daily_returns(self)
    cpdef object get_realized_pnls(self)
    cpdef double total_pnl(self) except *
    cpdef double total_pnl_percentage(self) except *
    cpdef double max_winner(self) except *
    cpdef double max_loser(self) except *
    cpdef double min_winner(self) except *
    cpdef double min_loser(self) except *
    cpdef double avg_winner(self) except *
    cpdef double avg_loser(self) except *
    cpdef double win_rate(self) except *
    cpdef double expectancy(self) except *
    cpdef double annual_return(self) except *
    cpdef double cum_return(self) except *
    cpdef double max_drawdown_return(self) except *
    cpdef double annual_volatility(self) except *
    cpdef double sharpe_ratio(self) except *
    cpdef double calmar_ratio(self) except *
    cpdef double sortino_ratio(self) except *
    cpdef double omega_ratio(self) except *
    cpdef double stability_of_timeseries(self) except *
    cpdef double returns_mean(self) except *
    cpdef double returns_variance(self) except *
    cpdef double returns_skew(self) except *
    cpdef double returns_kurtosis(self) except *
    cpdef double returns_tail_ratio(self) except *
    cpdef double alpha(self) except *
    cpdef double beta(self) except *
    cpdef dict get_performance_stats(self)

    cdef list get_performance_stats_formatted(self, Currency account_currency)
