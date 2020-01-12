# -------------------------------------------------------------------------------------------------
# <copyright file="performance.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.common.account cimport Account


cdef class PerformanceAnalyzer:
    cdef Account _account
    cdef Money _account_starting_capital
    cdef Money _account_capital
    cdef object _returns
    cdef object _positions
    cdef object _transactions
    cdef object _equity_curve

    cpdef void calculate_statistics(self, Account account, dict positions) except *
    cpdef void handle_transaction(self, AccountStateEvent event)  except *
    cpdef void add_return(self, datetime time, double value)  except *
    cpdef void add_positions(self, datetime time, list positions, Money cash_balance)  except *
    cpdef void reset(self)  except *
    cpdef object get_returns(self)
    cpdef object get_positions(self)
    cpdef object get_transactions(self)
    cpdef object get_equity_curve(self)
    cpdef Money total_pnl(self)
    cpdef double total_pnl_percentage(self)
    cpdef Money max_winner(self)
    cpdef Money max_loser(self)
    cpdef Money min_winner(self)
    cpdef Money min_loser(self)
    cpdef Money avg_winner(self)
    cpdef Money avg_loser(self)
    cpdef double win_rate(self)
    cpdef Money expectancy(self)
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

    cdef list get_performance_stats_formatted(self)
    cdef str _format_stat(self, double value, int decimals=*)
