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

from cpython.datetime cimport date
from cpython.datetime cimport datetime

from empyrical import alpha
from empyrical import annual_return
from empyrical import annual_volatility
from empyrical import beta
from empyrical import calmar_ratio
from empyrical import cum_returns_final
from empyrical import max_drawdown
from empyrical import omega_ratio
from empyrical import sharpe_ratio
from empyrical import sortino_ratio
from empyrical import stability_of_timeseries
from empyrical import tail_ratio
import numpy as np
from numpy import float64
import pandas as pd
from scipy.stats import kurtosis
from scipy.stats import skew

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.core.datetime cimport datetime_date
from nautilus_trader.core.functions cimport fast_mean
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.account cimport Account


cdef class PerformanceAnalyzer:
    """
    Provides a performance analyzer for tracking and generating performance
    metrics and statistics.
    """

    def __init__(self):
        """
        Initialize a new instance of the `PerformanceAnalyzer` class.
        """
        self._account_starting_balance = None
        self._account_balance = None
        self._account_currency = None
        self._daily_returns = pd.Series(dtype=float64)
        self._realized_pnls = pd.Series(dtype=float64)

    cpdef void calculate_statistics(self, Account account, list positions) except *:
        """
        Calculate performance metrics from the given data.

        Parameters
        ----------
        account : Account
            The account for the calculations.
        positions : dict[PositionId, Position]
            The positions for the calculations.

        """
        Condition.not_none(account, "account")
        Condition.not_none(positions, "positions")

        self._account_currency = account.currency
        self._account_balance = account.balance()

        cdef AccountState initial = account.events_c()[0]
        self._account_starting_balance = initial.balance

        self._daily_returns = pd.Series(dtype=float64)
        self._realized_pnls = pd.Series(dtype=float64)

        self.add_positions(positions)

    cpdef void add_positions(self, list positions) except *:
        """
        Add end of day positions data to the analyzer.

        Parameters
        ----------
        positions : list[Position]
            The positions for analysis.

        """
        Condition.not_none(positions, "positions")

        cdef Position position
        for position in positions:
            if position.is_closed_c():
                self.add_trade(position.id, position.realized_pnl)
                self.add_return(position.closed_time, position.realized_return)

    cpdef void add_trade(self, PositionId position_id, Money realized_pnl) except *:
        """
        Handle the transaction associated with the given account event.

        Parameters
        ----------
        position_id : PositionId
            The position identifier for the trade.
        realized_pnl : Money
            The realized PNL for the trade.

        """
        Condition.not_none(position_id, "position_id")
        Condition.not_none(realized_pnl, "realized_pnl")

        self._realized_pnls.loc[position_id.value] = realized_pnl.as_double()

    cpdef void add_return(self, datetime timestamp, double value) except *:
        """
        Add return data to the analyzer.

        Parameters
        ----------
        timestamp : datetime
            The timestamp for the returns entry.
        value : double
            The return value to add.

        """
        Condition.not_none(timestamp, "time")

        cdef date index_date = datetime_date(timestamp)
        if index_date not in self._daily_returns:
            self._daily_returns.loc[index_date] = 0

        self._daily_returns.loc[index_date] += value

    cpdef void reset(self) except *:
        """
        Reset the analyzer.

        All stateful values are reset to their initial value.
        """
        self._account_starting_balance = None
        self._account_balance = None
        self._account_currency = None
        self._daily_returns = pd.Series(dtype=float64)
        self._realized_pnls = pd.Series(dtype=float64)

    cpdef object get_daily_returns(self):
        """
        Return the returns data.
        Returns
        -------
        pd.Series
        """
        return self._daily_returns

    cpdef object get_realized_pnls(self):
        """
        Return the returns data.
        Returns
        -------
        pd.Series
        """
        return self._realized_pnls

    cpdef double total_pnl(self) except *:
        """
        Return the total PNL for the portfolio.

        Returns
        -------
        double

        """
        return float(self._account_balance - self._account_starting_balance)

    cpdef double total_pnl_percentage(self) except *:
        """
        Return the percentage change of the total PNL for the portfolio.

        Returns
        -------
        double

        """
        if self._account_starting_balance is None or self._account_starting_balance.as_decimal() == 0:
            # Protect divide by zero
            return 0.
        cdef Decimal current = self._account_balance
        cdef Decimal starting = self._account_starting_balance
        cdef Decimal difference = current - starting

        return (difference / starting) * 100

    cpdef double max_winner(self) except *:
        """
        Return the maximum winner for the portfolio.

        Returns
        -------
        double

        """
        if len(self._realized_pnls.index) == 0:
            return 0

        return max(self._realized_pnls)

    cpdef double max_loser(self) except *:
        """
        Return the maximum loser for the portfolio.

        Returns
        -------
        double

        """
        if len(self._realized_pnls.index) == 0:
            return 0

        return min(self._realized_pnls)

    cpdef double min_winner(self) except *:
        """
        Return the minimum winner for the portfolio.

        Returns
        -------
        double

        """
        cdef list winners = [x for x in self._realized_pnls if x > 0]
        if len(winners) == 0:
            return 0

        return min(winners)

    cpdef double min_loser(self) except *:
        """
        Return the minimum loser for the portfolio.

        Returns
        -------
        double

        """
        cdef list losers = [x for x in self._realized_pnls if x <= 0]
        if len(losers) == 0:
            return 0

        return max(losers)  # max is least loser

    cpdef double avg_winner(self) except *:
        """
        Return the average winner for the portfolio.

        Returns
        -------
        double

        """
        cdef list winners = [x for x in self._realized_pnls if x > 0]
        if len(winners) == 0:
            return 0

        return fast_mean(winners)

    cpdef double avg_loser(self) except *:
        """
        Return the average loser for the portfolio.

        Returns
        -------
        double

        """
        cdef list losers = [x for x in self._realized_pnls if x <= 0]
        if len(losers) == 0:
            return 0

        return fast_mean(losers)

    cpdef double win_rate(self) except *:
        """
        Return the win rate (after commission) for the portfolio.

        Returns
        -------
        double

        """
        if len(self._realized_pnls) == 0:
            return 0

        cdef list winners = [x for x in self._realized_pnls if x > 0]
        cdef list losers = [x for x in self._realized_pnls if x <= 0]

        return len(winners) / max(1, (len(winners) + len(losers)))

    cpdef double expectancy(self) except *:
        """
        Return the expectancy for the portfolio.

        Returns
        -------
        double

        """
        if len(self._realized_pnls.index) == 0:
            return 0

        cdef double win_rate = self.win_rate()
        cdef double loss_rate = 1 - win_rate

        return (self.avg_winner() * win_rate) + (self.avg_loser() * loss_rate)

    cpdef double annual_return(self) except *:
        """
        Determines the mean annual growth rate of returns. This is equivalent
        to the compound annual growth rate.

        Returns
        -------
        double

        """
        return annual_return(returns=self._daily_returns)

    cpdef double cum_return(self) except *:
        """
        Get the cumulative return for the portfolio.

        Returns
        -------
        double

        """
        return cum_returns_final(returns=self._daily_returns)

    cpdef double max_drawdown_return(self) except *:
        """
        Get the maximum return drawdown for the portfolio.

        Returns
        -------
        double

        """
        return max_drawdown(returns=self._daily_returns)

    cpdef double annual_volatility(self) except *:
        """
        Get the annual volatility for the portfolio.

        Returns
        -------
        double

        """
        return annual_volatility(returns=self._daily_returns)

    cpdef double sharpe_ratio(self) except *:
        """
        Get the sharpe ratio for the portfolio.

        Returns
        -------
        double

        """
        return sharpe_ratio(returns=self._daily_returns)

    cpdef double calmar_ratio(self) except *:
        """
        Get the calmar ratio for the portfolio.

        Returns
        -------
        double

        """
        return calmar_ratio(returns=self._daily_returns)

    cpdef double sortino_ratio(self) except *:
        """
        Get the sortino ratio for the portfolio.

        Returns
        -------
        double

        """
        return sortino_ratio(returns=self._daily_returns)

    cpdef double omega_ratio(self) except *:
        """
        Get the omega ratio for the portfolio.

        Returns
        -------
        double

        """
        return omega_ratio(returns=self._daily_returns)

    cpdef double stability_of_timeseries(self) except *:
        """
        Get the stability of time series for the portfolio.

        Returns
        -------
        double

        """
        return stability_of_timeseries(returns=self._daily_returns)

    cpdef double returns_mean(self) except *:
        """
        Get the returns mean for the portfolio.

        Returns
        -------
        double

        """
        return np.mean(self._daily_returns)

    cpdef double returns_variance(self) except *:
        """
        Get the returns variance for the portfolio.

        Returns
        -------
        double

        """
        return np.var(self._daily_returns)

    cpdef double returns_skew(self) except *:
        """
        Get the returns skew for the portfolio.

        Returns
        -------
        double

        """
        return skew(self._daily_returns)

    cpdef double returns_kurtosis(self) except *:
        """
        Get the returns kurtosis for the portfolio.

        Returns
        -------
        double

        """
        return kurtosis(self._daily_returns)

    cpdef double returns_tail_ratio(self) except *:
        """
        Get the returns tail ratio for the portfolio.

        Returns
        -------
        double

        """
        return tail_ratio(self._daily_returns)

    cpdef double alpha(self) except *:
        """
        Get the alpha for the portfolio.

        Returns
        -------
        double

        """
        return alpha(returns=self._daily_returns, factor_returns=self._daily_returns)

    cpdef double beta(self) except *:
        """
        Get the beta for the portfolio.

        Returns
        -------
        double

        """
        return beta(returns=self._daily_returns, factor_returns=self._daily_returns)

    cpdef dict get_performance_stats(self):
        """
        Return the performance statistics from the last backtest run.

        Money objects are converted to floats.

        Returns
        -------
        dict[str, double]

        """
        return {
            "PNL": self.total_pnl(),
            "PNL%": self.total_pnl_percentage(),
            "MaxWinner": self.max_winner(),
            "AvgWinner": self.avg_winner(),
            "MinWinner": self.min_winner(),
            "MinLoser": self.min_loser(),
            "AvgLoser": self.avg_loser(),
            "MaxLoser": self.max_loser(),
            "WinRate": self.win_rate(),
            "Expectancy": self.expectancy(),
            "AnnualReturn": self.annual_return(),
            "CumReturn": self.cum_return(),
            "MaxDrawdown": self.max_drawdown_return(),
            "AnnualVol": self.annual_volatility(),
            "SharpeRatio": self.sharpe_ratio(),
            "CalmarRatio": self.calmar_ratio(),
            "SortinoRatio": self.sortino_ratio(),
            "OmegaRatio": self.omega_ratio(),
            "Stability": self.stability_of_timeseries(),
            "ReturnsMean": self.returns_mean(),
            "ReturnsVariance": self.returns_variance(),
            "ReturnsSkew": self.returns_skew(),
            "ReturnsKurtosis": self.returns_kurtosis(),
            "TailRatio": self.returns_tail_ratio(),
            "Alpha": self.alpha(),
            "Beta": self.beta()
        }

    cdef list get_performance_stats_formatted(self, Currency account_currency):
        """
        Return the performance statistics from the last backtest run formatted
        for printing in the backtest run footer.

        Parameters
        ----------
        account_currency : Currency
            The account currency.

        Returns
        -------
        list[str]

        """
        return [
            f"PNL:               {round(self.total_pnl(), 2):,} {self._account_currency}",
            f"PNL %:             {round(self.total_pnl_percentage(), 2)}%",
            f"Max Winner:        {round(self.max_winner(), 2):,} {self._account_currency}",
            f"Avg Winner:        {round(self.avg_winner(), 2):,} {self._account_currency}",
            f"Min Winner:        {round(self.min_winner(), 2):,} {self._account_currency}",
            f"Min Loser:         {round(self.min_loser(), 2):,} {self._account_currency}",
            f"Avg Loser:         {round(self.avg_loser(), 2):,} {self._account_currency}",
            f"Max Loser:         {round(self.max_loser(), 2):,} {self._account_currency}",
            f"Win Rate:          {round(self.win_rate(), 2)}",
            f"Expectancy:        {round(self.expectancy(), 2):,} {self._account_currency}",
            f"Annual return:     {round(self.annual_return() * 100, 2)}%",
            f"Cum returns:       {round(self.cum_return() * 100, 2)}%",
            f"Max drawdown:      {round(self.max_drawdown_return() * 100, 2)}%",
            f"Annual vol:        {round(self.annual_volatility() * 100, 2)}%",
            f"Sharpe ratio:      {round(self.sharpe_ratio(), 2)}",
            f"Calmar ratio:      {round(self.calmar_ratio(), 2)}",
            f"Sortino ratio:     {round(self.sortino_ratio(), 2)}",
            f"Omega ratio:       {round(self.omega_ratio(), 2)}",
            f"Stability:         {round(self.stability_of_timeseries(), 2)}",
            f"Returns Mean:      {round(self.returns_mean(), 5)}",
            f"Returns Variance:  {round(self.returns_variance(), 8)}",
            f"Returns Skew:      {round(self.returns_skew(), 2)}",
            f"Returns Kurtosis:  {round(self.returns_kurtosis(), 2)}",
            f"Tail ratio:        {round(self.returns_tail_ratio(), 2)}",
            f"Alpha:             {round(self.alpha(), 2)}",
            f"Beta:              {round(self.beta(), 2)}"
        ]
