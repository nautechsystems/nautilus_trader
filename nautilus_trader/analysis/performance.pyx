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

import numpy as np
import pandas as pd
from cpython.datetime cimport date, datetime
from numpy import float64
from scipy.stats import kurtosis, skew

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport fast_round
from nautilus_trader.model.c_enums.currency cimport currency_to_string
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.model.position cimport Position
from nautilus_trader.common.account cimport Account
from nautilus_trader.analysis.stats import annual_return, cum_returns_final, annual_volatility
from nautilus_trader.analysis.stats import sharpe_ratio, calmar_ratio, sortino_ratio, omega_ratio
from nautilus_trader.analysis.stats import stability_of_timeseries, max_drawdown, alpha, beta
from nautilus_trader.analysis.stats import tail_ratio


cdef class PerformanceAnalyzer:
    """
    Provides a performance analyzer for tracking and generating performance
    metrics and statistics.
    """

    def __init__(self):
        """
        Initializes a new instance of the PerformanceAnalyzer class.
        """
        self._account_starting_capital = None
        self._account_capital = None
        self._account_currency = Currency.UNDEFINED
        self._returns = pd.Series(dtype=float64)
        self._positions = pd.DataFrame(columns=['cash'])
        self._transactions = pd.DataFrame(columns=['capital', 'pnl'])

    cpdef void calculate_statistics(self, Account account, dict positions) except *:
        """
        Calculate performance metrics from the given data.
        
        :param account: The account for the calculations.
        :param positions: The positions for the calculations.
        """
        Condition.not_none(account, 'account')
        Condition.not_none(positions, 'positions')

        cdef AccountStateEvent event
        for event in account.get_events():
            self.add_transaction(event)

        cdef Position position
        for position in positions.values():
            if position.is_closed:
                self.add_return(position.closed_time, position.realized_return)

    cpdef void add_transaction(self, AccountStateEvent event) except *:
        """
        Handle the transaction associated with the given account event.

        :param event: The event to handle.
        """
        Condition.not_none(event, 'event')

        if self._account_capital is None:
            # Initialize account data
            self._account_starting_capital = event.cash_balance
            self._account_capital = event.cash_balance
            self._account_currency = event.currency
            return  # No transaction to handle

        if self._account_capital.equals(event.cash_balance):
            return  # No transaction to handle

        # Calculate transaction data
        cdef Money pnl = event.cash_balance.subtract(self._account_capital)
        self._account_capital = event.cash_balance

        # Set index if it does not exist
        if event.timestamp not in self._transactions:
            self._transactions.loc[event.timestamp] = 0

        self._transactions.loc[event.timestamp]['capital'] = self._account_capital.as_double()
        self._transactions.loc[event.timestamp]['pnl'] = pnl.as_double()

    cpdef void add_return(self, datetime timestamp, double value) except *:
        """
        Add return data to the analyzer.

        :param timestamp: The timestamp for the returns entry.
        :param value: The return value to add.
        """
        Condition.not_none(timestamp, 'time')

        cdef date index_date = timestamp.date()
        if index_date not in self._returns:
            self._returns.loc[index_date] = 0.0

        self._returns.loc[index_date] += value

    cpdef void add_positions(
            self,
            datetime timestamp,
            list positions,
            Money cash_balance) except *:
        """
        Add end of day positions data to the analyzer.

        :param timestamp: The timestamp for the positions entry.
        :param positions: The end of day positions.
        :param cash_balance: The end of day cash balance of the account.
        """
        Condition.not_none(timestamp, 'time')
        Condition.not_none(positions, 'positions')
        Condition.not_none(cash_balance, 'cash_balance')

        cdef date index_date = timestamp.date()
        if index_date not in self._positions:
            self._positions.loc[index_date] = 0

        cdef Position position
        cdef str symbol
        cdef list columns
        for position in positions:
            symbol = position.symbol.to_string()
            columns = list(self._positions.columns.values)
            if symbol not in columns:
                self._positions[symbol] = 0

            # noinspection PyProtectedMember
            # direct access to protected member ok here
            self._positions.loc[index_date][symbol] += position._relative_quantity

    cpdef void reset(self) except *:
        """
        Reset the analyzer by returning all stateful values to their initial value.
        """
        self._account_starting_capital = None
        self._account_capital = None
        self._account_currency = Currency.UNDEFINED
        self._returns = pd.Series(dtype=float64)
        self._positions = pd.DataFrame(columns=['cash'])
        self._transactions = pd.DataFrame(columns=['capital', 'pnl'])

    cpdef object get_returns(self):
        """
        Return the returns data.

        :return Pandas.Series.
        """
        return self._returns

    cpdef object get_positions(self):
        """
        Return the positions data.

        :return Pandas.DataFrame.
        """
        return self._positions

    cpdef object get_transactions(self):
        """
        Return the transactions data.

        :return Pandas.DataFrame.
        """
        return self._transactions

    cpdef object get_equity_curve(self):
        """
        Return the transactions data.

        :return Pandas.DataFrame.
        """
        return self._transactions['capital']

    cpdef double total_pnl(self):
        """
        Return the total PNL for the portfolio.

        :return double.
        """
        return self._account_capital.subtract(self._account_starting_capital).as_double()

    cpdef double total_pnl_percentage(self):
        """
        Return the percentage change of the total PNL for the portfolio.

        :return double.
        """
        if self._account_starting_capital.as_double() == 0:  # Protect divide by zero
            return 0.0
        cdef double current = self._account_capital.as_double()
        cdef double starting = self._account_starting_capital.as_double()
        cdef double difference = current - starting

        return (difference / starting) * 100

    cpdef double max_winner(self):
        """
        Return the maximum winner for the portfolio.

        :return double.
        """
        return self._transactions['pnl'].max()

    cpdef double max_loser(self):
        """
        Return the maximum loser for the portfolio.

        :return double.
        """
        return self._transactions['pnl'].min()

    cpdef double min_winner(self):
        """
        Return the minimum winner for the portfolio.

        :return double.
        """
        return self._transactions['pnl'][self._transactions['pnl'] > 0].min()

    cpdef double min_loser(self):
        """
        Return the minimum loser for the portfolio.

        :return double.
        """
        return self._transactions['pnl'][self._transactions['pnl'] <= 0].max()

    cpdef double avg_winner(self):
        """
        Return the average winner for the portfolio.

        :return double.
        """
        return self._transactions['pnl'][self._transactions['pnl'] > 0].mean()

    cpdef double avg_loser(self):
        """
        Return the average loser for the portfolio.

        :return double.
        """
        return self._transactions['pnl'][self._transactions['pnl'] <= 0].mean()

    cpdef double win_rate(self):
        """
        Return the win rate (after commissions) for the portfolio.

        :return double.
        """
        cdef list winners = list(self._transactions['pnl'][self._transactions['pnl'] > 0])
        cdef list losers = list(self._transactions['pnl'][self._transactions['pnl'] <= 0])

        return len(winners) / max(1.0, (len(winners) + len(losers)))

    cpdef double expectancy(self):
        """
        Return the expectancy for the portfolio.

        :return double.
        """
        cdef double win_rate = self.win_rate()
        cdef double loss_rate = 1.0 - win_rate

        return (self.avg_winner() * win_rate) + (self.avg_loser() * loss_rate)

    cpdef double annual_return(self):
        """
        Determines the mean annual growth rate of returns. This is equivalent
        to the compound annual growth rate.

        :return double.
        """
        return annual_return(returns=self._returns)

    cpdef double cum_return(self):
        """
        Get the cumulative return for the portfolio.

        :return double.
        """
        return cum_returns_final(returns=self._returns)

    cpdef double max_drawdown_return(self):
        """
        Get the maximum return drawdown for the portfolio.

        :return double.
        """
        return max_drawdown(returns=self._returns)

    cpdef double annual_volatility(self):
        """
        Get the annual volatility for the portfolio.

        :return double.
        """
        return annual_volatility(returns=self._returns)

    cpdef double sharpe_ratio(self):
        """
        Get the sharpe ratio for the portfolio.

        :return double.
        """
        return sharpe_ratio(returns=self._returns)

    cpdef double calmar_ratio(self):
        """
        Get the calmar ratio for the portfolio.
        
        :return double.
        """
        return calmar_ratio(returns=self._returns)

    cpdef double sortino_ratio(self):
        """
        Get the sortino ratio for the portfolio.

        :return double.
        """
        return sortino_ratio(returns=self._returns)

    cpdef double omega_ratio(self):
        """
        Get the omega ratio for the portfolio.

        :return double.
        """
        return omega_ratio(returns=self._returns)

    cpdef double stability_of_timeseries(self):
        """
        Get the stability of time series for the portfolio.

        :return double.
        """
        return stability_of_timeseries(returns=self._returns)

    cpdef double returns_mean(self):
        """
        Get the returns mean for the portfolio.

        :return double.
        """
        return np.mean(self._returns)

    cpdef double returns_variance(self):
        """
        Get the returns variance for the portfolio.

        :return double.
        """
        return np.var(self._returns)

    cpdef double returns_skew(self):
        """
        Get the returns skew for the portfolio.

        :return double.
        """
        return skew(self._returns)

    cpdef double returns_kurtosis(self):
        """
        Get the returns kurtosis for the portfolio.

        :return double.
        """
        return kurtosis(self._returns)

    cpdef double returns_tail_ratio(self):
        """
        Get the returns tail ratio for the portfolio.

        :return double.
        """
        return tail_ratio(self._returns)

    cpdef double alpha(self):
        """
        Get the alpha for the portfolio.

        :return double.
        """
        return alpha(returns=self._returns, factor_returns=self._returns)

    cpdef double beta(self):
        """
        Get the beta for the portfolio.

        :return double.
        """
        return beta(returns=self._returns, factor_returns=self._returns)

    cpdef dict get_performance_stats(self):
        """
        Return the performance statistics from the last backtest run.
        Note: Money objects are converted to floats.

        Statistics Keys
        ---------------
        - PNL
        - PNL%
        - MaxWinner
        - AvgWinner
        - MinWinner
        - MinLoser
        - AvgLoser
        - MaxLoser
        - WinRate
        - Expectancy
        - AnnualReturn
        - CumReturn
        - MaxDrawdown
        - AnnualVol
        - SharpeRatio
        - CalmarRatio
        - SortinoRatio
        - OmegaRatio
        - Stability
        - ReturnsMean
        - ReturnsVariance
        - ReturnsSkew
        - ReturnsKurtosis
        - TailRatio
        - Alpha
        - Beta
        
        :return Dict[str, double].
        """
        return {
            'PNL': self.total_pnl(),
            'PNL%': self.total_pnl_percentage(),
            'MaxWinner': self.max_winner(),
            'AvgWinner': self.avg_winner(),
            'MinWinner': self.min_winner(),
            'MinLoser': self.min_loser(),
            'AvgLoser': self.avg_loser(),
            'MaxLoser': self.max_loser(),
            'WinRate': self.win_rate(),
            'Expectancy': self.expectancy(),
            'AnnualReturn': self.annual_return(),
            'CumReturn': self.cum_return(),
            'MaxDrawdown': self.max_drawdown_return(),
            'AnnualVol': self.annual_volatility(),
            'SharpeRatio': self.sharpe_ratio(),
            'CalmarRatio': self.calmar_ratio(),
            'SortinoRatio': self.sortino_ratio(),
            'OmegaRatio': self.omega_ratio(),
            'Stability': self.stability_of_timeseries(),
            'ReturnsMean': self.returns_mean(),
            'ReturnsVariance': self.returns_variance(),
            'ReturnsSkew': self.returns_skew(),
            'ReturnsKurtosis': self.returns_kurtosis(),
            'TailRatio': self.returns_tail_ratio(),
            'Alpha': self.alpha(),
            'Beta': self.beta()
        }

    cdef list get_performance_stats_formatted(self, Currency account_currency):
        """
        Return the performance statistics from the last backtest run formatted
        for printing in the backtest run footer.
        
        :return List[str].
        """
        cdef str currency = currency_to_string(account_currency)

        return [
            f"PNL:               {fast_round(self.total_pnl(), precision=2):,} {currency}",
            f"PNL %:             {fast_round(self.total_pnl_percentage(), precision=2)}%",
            f"Max Winner:        {fast_round(self.max_winner(), precision=2):,} {currency}",
            f"Avg Winner:        {fast_round(self.avg_winner(), precision=2):,} {currency}",
            f"Min Winner:        {fast_round(self.min_winner(), precision=2):,} {currency}",
            f"Min Loser:         {fast_round(self.min_loser(), precision=2):,} {currency}",
            f"Avg Loser:         {fast_round(self.avg_loser(), precision=2):,} {currency}",
            f"Max Loser:         {fast_round(self.max_loser(), precision=2):,} {currency}",
            f"Win Rate:          {fast_round(self.win_rate(), precision=2)}",
            f"Expectancy:        {fast_round(self.expectancy(), precision=2):,} {currency}",
            f"Annual return:     {fast_round(self.annual_return() * 100, precision=2)}%",
            f"Cum returns:       {fast_round(self.cum_return() * 100, precision=2)}%",
            f"Max drawdown:      {fast_round(self.max_drawdown_return() * 100, precision=2)}%",
            f"Annual vol:        {fast_round(self.annual_volatility() * 100, precision=2)}%",
            f"Sharpe ratio:      {fast_round(self.sharpe_ratio(), precision=2)}",
            f"Calmar ratio:      {fast_round(self.calmar_ratio(), precision=2)}",
            f"Sortino ratio:     {fast_round(self.sortino_ratio(), precision=2)}",
            f"Omega ratio:       {fast_round(self.omega_ratio(), precision=2)}",
            f"Stability:         {fast_round(self.stability_of_timeseries(), precision=2)}",
            f"Returns Mean:      {fast_round(self.returns_mean(), precision=5)}",
            f"Returns Variance:  {fast_round(self.returns_variance(), precision=8)}",
            f"Returns Skew:      {fast_round(self.returns_skew(), precision=2)}",
            f"Returns Kurtosis:  {fast_round(self.returns_kurtosis(), precision=2)}",
            f"Tail ratio:        {fast_round(self.returns_tail_ratio(), precision=2)}",
            f"Alpha:             {fast_round(self.alpha(), precision=2)}",
            f"Beta:              {fast_round(self.beta(), precision=2)}"
        ]
