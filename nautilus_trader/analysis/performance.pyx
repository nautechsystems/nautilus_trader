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
cimport numpy as np
import pandas as pd
from scipy.stats import kurtosis
from scipy.stats import skew

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport nanos_to_unix_dt
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
        Initialize a new instance of the ``PerformanceAnalyzer`` class.
        """
        self._account_balances_starting = {}  # type: dict[Currency, Money]
        self._account_balances = {}           # type: dict[Currency, Money]
        self._realized_pnls = {}              # type: dict[Currency, list[float]]
        self._daily_returns = pd.Series(dtype=float64)

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

        self._account_balances_starting = account.starting_balances()
        self._account_balances = account.balances_total()
        self._realized_pnls = {}
        self._daily_returns = pd.Series(dtype=float64)

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
            self.add_trade(position.id, position.realized_pnl)
            self.add_return(nanos_to_unix_dt(position.ts_closed_ns), position.realized_return)

    cpdef void add_trade(self, PositionId position_id, Money realized_pnl) except *:
        """
        Handle the transaction associated with the given account event.

        Parameters
        ----------
        position_id : PositionId
            The position ID for the trade.
        realized_pnl : Money
            The realized PnL for the trade.

        """
        Condition.not_none(position_id, "position_id")
        Condition.not_none(realized_pnl, "realized_pnl")

        cdef Currency currency = realized_pnl.currency
        realized_pnls = self._realized_pnls.get(currency, pd.Series(dtype=float64))
        realized_pnls.loc[position_id.value] = realized_pnl.as_double()
        self._realized_pnls[currency] = realized_pnls

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

        cdef date index_date = timestamp.date()
        if index_date not in self._daily_returns:
            self._daily_returns.loc[index_date] = 0

        self._daily_returns.loc[index_date] += value

    cpdef void reset(self) except *:
        """
        Reset the analyzer.

        All stateful fields are reset to their initial value.
        """
        self._account_balances_starting = {}
        self._account_balances = {}
        self._realized_pnls = {}
        self._daily_returns = pd.Series(dtype=float64)

    cpdef object realized_pnls(self, Currency currency=None):
        """
        Return the realized PnL for the portfolio.

        For multi-currency portfolios, specify the currency for the result.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the result.

        Returns
        -------
        pd.Series or None

        Raises
        ------
        ValueError
            If currency is None when analyzing multi-currency portfolios.

        """
        if not self._realized_pnls:
            return None
        if currency is None:
            Condition.true(len(self._account_balances) == 1, "currency was None for multi-currency portfolio")
            currency = next(iter(self._account_balances.keys()))

        return self._realized_pnls.get(currency)

    cpdef double total_pnl(self, Currency currency=None) except *:
        """
        Return the total PnL for the portfolio.

        For multi-currency portfolios, specify the currency for the result.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the result.

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If currency is None when analyzing multi-currency portfolios.
        ValueError
            If currency is not contained in the tracked account balances.

        """
        if not self._account_balances:
            return 0.0
        if currency is None:
            Condition.true(len(self._account_balances) == 1, "currency was None for multi-currency portfolio")
            currency = next(iter(self._account_balances.keys()))
        Condition.is_in(currency, self._account_balances, "currency", "self._account_balances")

        account_balance = self._account_balances.get(currency)
        account_balance_starting = self._account_balances_starting.get(currency, Money(0, currency))

        if account_balance is None:
            return 0.0

        return float(account_balance - account_balance_starting)

    cpdef double total_pnl_percentage(self, Currency currency=None) except *:
        """
        Return the percentage change of the total PnL for the portfolio.

        For multi-currency accounts, specify the currency for the result.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the result.

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If currency is None when analyzing multi-currency portfolios.
        ValueError
            If currency is not contained in the tracked account balances.

        """
        if not self._account_balances:
            return 0.0
        if currency is None:
            Condition.true(len(self._account_balances) == 1, "currency was None for multi-currency portfolio")
            currency = next(iter(self._account_balances.keys()))
        Condition.is_in(currency, self._account_balances, "currency", "self._account_balances")

        account_balance = self._account_balances.get(currency)
        account_balance_starting = self._account_balances_starting.get(currency, Money(0, currency))

        if account_balance is None:
            return 0.0

        if account_balance_starting.as_decimal() == 0:
            # Protect divide by zero
            return 0.0

        current = account_balance
        starting = account_balance_starting
        difference = current - starting

        return (difference / starting) * 100

    cpdef double max_winner(self, Currency currency=None) except *:
        """
        Return the maximum winner for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        double

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        return max(realized_pnls)

    cpdef double max_loser(self, Currency currency=None) except *:
        """
        Return the maximum loser for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        double

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        return min(realized_pnls)

    cpdef double min_winner(self, Currency currency=None) except *:
        """
        Return the minimum winner for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        double

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        cdef list winners = [x for x in realized_pnls if x > 0.0]
        if realized_pnls is None or not winners:
            return 0.0

        return min(np.asarray(winners, dtype=np.float64))

    cpdef double min_loser(self, Currency currency=None) except *:
        """
        Return the minimum loser for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        double

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        cdef list losers = [x for x in realized_pnls if x <= 0.0]
        if not losers:
            return 0.0

        return max(np.asarray(losers, dtype=np.float64))  # max is least loser

    cpdef double avg_winner(self, Currency currency=None) except *:
        """
        Return the average winner for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        double

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        cdef np.ndarray pnls = realized_pnls.to_numpy()
        cdef np.ndarray winners = pnls[pnls > 0.0]
        if len(winners) == 0:
            return 0.0
        else:
            return winners.mean()

    cpdef double avg_loser(self, Currency currency=None) except *:
        """
        Return the average loser for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        double

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        cdef np.ndarray pnls = realized_pnls.to_numpy()
        cdef np.ndarray losers = pnls[pnls <= 0.0]
        if len(losers) == 0:
            return 0.0
        else:
            return losers.mean()

    cpdef double win_rate(self, Currency currency=None) except *:
        """
        Return the win rate (after commission) for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        double

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        cdef list winners = [x for x in realized_pnls if x > 0.0]
        cdef list losers = [x for x in realized_pnls if x <= 0.0]

        return len(winners) / float(max(1, (len(winners) + len(losers))))

    cpdef double expectancy(self, Currency currency=None) except *:
        """
        Return the expectancy for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        double

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        cdef double win_rate = self.win_rate(currency)
        cdef double loss_rate = 1.0 - win_rate

        return (self.avg_winner(currency) * win_rate) + (self.avg_loser(currency) * loss_rate)

    cpdef object daily_returns(self):
        """
        Return the returns data.

        Returns
        -------
        pd.Series

        """
        return self._daily_returns

    cpdef double annual_return(self) except *:
        """
        Return the mean annual growth rate of returns.

        Returns
        -------
        double

        Notes
        -----
        This is equivalent to the compound annual growth rate.

        """
        return annual_return(returns=self._daily_returns)

    cpdef double cum_return(self) except *:
        """
        Return the cumulative return for the portfolio.

        Returns
        -------
        double

        """
        return cum_returns_final(returns=self._daily_returns)

    cpdef double max_drawdown_return(self) except *:
        """
        Return the maximum return drawdown for the portfolio.

        Returns
        -------
        double

        """
        return max_drawdown(returns=self._daily_returns)

    cpdef double annual_volatility(self) except *:
        """
        Return the annual volatility for the portfolio.

        Returns
        -------
        double

        """
        return annual_volatility(returns=self._daily_returns)

    cpdef double sharpe_ratio(self) except *:
        """
        Return the sharpe ratio for the portfolio.

        Returns
        -------
        double

        """
        return sharpe_ratio(returns=self._daily_returns)

    cpdef double calmar_ratio(self) except *:
        """
        Return the calmar ratio for the portfolio.

        Returns
        -------
        double

        """
        return calmar_ratio(returns=self._daily_returns)

    cpdef double sortino_ratio(self) except *:
        """
        Return the sortino ratio for the portfolio.

        Returns
        -------
        double

        """
        return sortino_ratio(returns=self._daily_returns)

    cpdef double omega_ratio(self) except *:
        """
        Return the omega ratio for the portfolio.

        Returns
        -------
        double

        """
        return omega_ratio(returns=self._daily_returns)

    cpdef double stability_of_timeseries(self) except *:
        """
        Return the stability of time series for the portfolio.

        Returns
        -------
        double

        """
        return stability_of_timeseries(returns=self._daily_returns)

    cpdef double returns_mean(self) except *:
        """
        Return the returns mean for the portfolio.

        Returns
        -------
        double

        """
        return np.mean(self._daily_returns)

    cpdef double returns_variance(self) except *:
        """
        Return the returns variance for the portfolio.

        Returns
        -------
        double

        """
        return np.var(self._daily_returns)

    cpdef double returns_skew(self) except *:
        """
        Return the returns skew for the portfolio.

        Returns
        -------
        double

        """
        return skew(self._daily_returns)

    cpdef double returns_kurtosis(self) except *:
        """
        Return the returns kurtosis for the portfolio.

        Returns
        -------
        double

        """
        return kurtosis(self._daily_returns)

    cpdef double returns_tail_ratio(self) except *:
        """
        Return the returns tail ratio for the portfolio.

        Returns
        -------
        double

        """
        return tail_ratio(self._daily_returns)

    cpdef double alpha(self) except *:
        """
        Return the alpha for the portfolio.

        Returns
        -------
        double

        """
        return alpha(returns=self._daily_returns, factor_returns=self._daily_returns)

    cpdef double beta(self) except *:
        """
        Return the beta for the portfolio.

        Returns
        -------
        double

        """
        return beta(returns=self._daily_returns, factor_returns=self._daily_returns)

    cpdef dict get_performance_stats_pnls(self, Currency currency=None):
        """
        Return the performance statistics for PnL from the last backtest run.

        Money objects are converted to floats.

        Parameters
        ----------
        currency : Currency
            The currency for the performance.

        Returns
        -------
        dict[str, double]

        """
        return {
            "PnL": self.total_pnl(currency),
            "PnL%": self.total_pnl_percentage(currency),
            "MaxWinner": self.max_winner(currency),
            "AvgWinner": self.avg_winner(currency),
            "MinWinner": self.min_winner(currency),
            "MinLoser": self.min_loser(currency),
            "AvgLoser": self.avg_loser(currency),
            "MaxLoser": self.max_loser(currency),
            "WinRate": self.win_rate(currency),
            "Expectancy": self.expectancy(currency),
        }

    cpdef list get_performance_stats_pnls_formatted(self, Currency currency=None):
        """
        Return the performance statistics from the last backtest run formatted
        for printing in the backtest run footer.

        Parameters
        ----------
        currency : Currency
            The currency for the performance.

        Returns
        -------
        list[str]

        """
        return [
            f"PnL:               {round(self.total_pnl(currency), currency.precision):,} {currency}",
            f"PnL%:              {round(self.total_pnl_percentage(currency), 4)}%",
            f"Max Winner:        {round(self.max_winner(currency), currency.precision):,} {currency}",
            f"Avg Winner:        {round(self.avg_winner(currency), currency.precision):,} {currency}",
            f"Min Winner:        {round(self.min_winner(currency), currency.precision):,} {currency}",
            f"Min Loser:         {round(self.min_loser(currency), currency.precision):,} {currency}",
            f"Avg Loser:         {round(self.avg_loser(currency), currency.precision):,} {currency}",
            f"Max Loser:         {round(self.max_loser(currency), currency.precision):,} {currency}",
            f"Win Rate:          {round(self.win_rate(currency), 4)}",
            f"Expectancy:        {round(self.expectancy(currency), currency.precision):,} {currency}",
        ]

    cpdef dict get_performance_stats_returns(self):
        """
        Return the performance statistics from the last backtest run.

        Returns
        -------
        dict[str, double]

        """
        return {
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

    cpdef list get_performance_stats_returns_formatted(self):
        """
        Return the performance statistics for returns from the last backtest run
        formatted for printing in the backtest run footer.

        Returns
        -------
        list[str]

        """
        return [
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
