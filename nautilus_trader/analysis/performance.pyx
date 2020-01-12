# -------------------------------------------------------------------------------------------------
# <copyright file="performance.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import numpy as np
import pandas as pd

from cpython.datetime cimport date, datetime
from scipy.stats import kurtosis, skew
from empyrical.stats import (
    annual_return,
    cum_returns_final,
    annual_volatility,
    sharpe_ratio,
    calmar_ratio,
    sortino_ratio,
    omega_ratio,
    stability_of_timeseries,
    max_drawdown,
    alpha,
    beta,
    tail_ratio)

from nautilus_trader.model.c_enums.currency cimport currency_to_string
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.common.account cimport Account


cdef class PerformanceAnalyzer:
    """
    Provides a performance analyzer for tracking and generating performance metrics
    and statistics.
    """

    def __init__(self):
        """
        Initializes a new instance of the PerformanceAnalyzer class.
        """
        self._account = None
        self._account_starting_capital = None
        self._account_capital = None
        self._returns = pd.Series()
        self._positions = pd.DataFrame(columns=['cash'])
        self._transactions = pd.DataFrame(columns=['amount'])
        self._equity_curve = pd.DataFrame(columns=['capital', 'pnl'])

    cpdef void calculate_statistics(self, Account account, dict positions)  except *:
        """
        Calculate performance metrics from the given data.
        """
        self._account = account

        for event in self._account.get_events():
            self.handle_transaction(event)

        for position_id, position in positions.items():
            if position.is_closed:
                self.add_return(position.closed_time, position.realized_return)

    cpdef void handle_transaction(self, AccountStateEvent event)  except *:
        """
        Handle the transaction associated with the given account event.

        :param event: The event to handle.
        """
        if self._account_capital is None:
            # Initialize account data
            self._account_starting_capital = event.cash_balance
            self._account_capital = event.cash_balance
            return  # No transaction to handle

        if self._account_capital == event.cash_balance:
            return  # No transaction to handle

        # Calculate transaction data
        cdef Money pnl = event.cash_balance.subtract(self._account_capital)
        self._account_capital = event.cash_balance

        # Set index if it does not exist
        if event.timestamp not in self._equity_curve:
            self._equity_curve.loc[event.timestamp] = 0

        self._equity_curve.loc[event.timestamp]['capital'] = self._account_capital.value
        self._equity_curve.loc[event.timestamp]['pnl'] = pnl.value

    cpdef void add_return(self, datetime time, float value)  except *:
        """
        Add return data to the analyzer.
        
        :param time: The timestamp for the returns entry.
        :param value: The return value to add.
        """
        cdef date index_date = pd.to_datetime(time.date())
        if index_date not in self._returns:
            self._returns.loc[index_date] = 0.0

        self._returns.loc[index_date] += value

    cpdef void add_positions(
            self,
            datetime time,
            list positions,
            Money cash_balance)  except *:
        """
        Add end of day positions data to the analyzer.

        :param time: The timestamp for the positions entry.
        :param positions: The end of day positions.
        :param cash_balance: The end of day cash balance of the account.
        """
        cdef date index_date = pd.to_datetime(time.date())
        if index_date not in self._positions:
            self._positions.loc[index_date] = 0

        cdef str symbol
        cdef list columns
        for position in positions:
            symbol = str(position.symbol)
            columns = list(self._positions.columns.values)
            if symbol not in columns:
                self._positions[symbol] = 0
            self._positions.loc[index_date][symbol] += position.relative_quantity

    cpdef void reset(self)  except *:
        """
        Reset the analyzer by returning all stateful values to their initial value.
        """
        self._returns = pd.Series()
        self._positions = pd.DataFrame(columns=['cash'])
        self._transactions = pd.DataFrame(columns=['amount'])
        self._equity_curve = pd.DataFrame(columns=['capital', 'pnl'])

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
        return self._equity_curve

    cpdef Money total_pnl(self):
        """
        Return the total PNL for the portfolio.
        
        :return Money. 
        """
        return self._account_capital.subtract(self._account_starting_capital)

    cpdef float total_pnl_percentage(self):
        """
        Return the percentage change of the total PNL for the portfolio.
        
        :return float. 
        """
        if self._account_starting_capital == Money.zero():  # Protect divide by zero
            return 0.0
        return ((self._account_capital - self._account_starting_capital) / self._account_starting_capital.as_float()) * 100

    cpdef Money max_winner(self):
        """
        Return the maximum winner for the portfolio.
        
        :return Money.
        """
        return Money(self._equity_curve['pnl'].max())

    cpdef Money max_loser(self):
        """
        Return the maximum loser for the portfolio.
        
        :return Money.
        """
        return Money(self._equity_curve['pnl'].min())

    cpdef Money min_winner(self):
        """
        Return the minimum winner for the portfolio.
        
        :return Money.
        """
        return Money(self._equity_curve['pnl'][self._equity_curve['pnl'] > 0].min())

    cpdef Money min_loser(self):
        """
        Return the minimum loser for the portfolio.
        
        :return Money.
        """
        return Money(self._equity_curve['pnl'][self._equity_curve['pnl'] < 0].max())

    cpdef Money avg_winner(self):
        """
        Return the average winner for the portfolio.
        
        :return Money.
        """
        return Money(self._equity_curve['pnl'][self._equity_curve['pnl'] > 0].mean())

    cpdef Money avg_loser(self):
        """
        Return the average loser for the portfolio.
        
        :return Money.
        """
        return Money(self._equity_curve['pnl'][self._equity_curve['pnl'] < 0].mean())

    cpdef float win_rate(self):
        """
        Return the win rate (after commissions) for the portfolio.
        
        :return float. 
        """
        cdef object winners = self._equity_curve['pnl'][self._equity_curve['pnl'] > 0]
        cdef object losers = self._equity_curve['pnl'][self._equity_curve['pnl'] <= 0]

        return len(winners) / max(1.0, (len(winners) + len(losers)))

    cpdef Money expectancy(self):
        """
        Return the expectancy for the portfolio.
        
        :return float. 
        """
        cdef float win_rate = self.win_rate()
        cdef float loss_rate = 1.0 - win_rate

        return Money((self.avg_winner().as_float() * win_rate) - (-self.avg_loser().as_float() * loss_rate))

    cpdef float annual_return(self):
        """
        Get the annual return for the portfolio.
        
        :return float.
        """
        return annual_return(returns=self._returns)

    cpdef float cum_return(self):
        """
        Get the cumulative return for the portfolio.
        
        :return float.
        """
        return cum_returns_final(returns=self._returns)

    cpdef float max_drawdown_return(self):
        """
        Get the maximum return drawdown for the portfolio.
        
        :return float.
        """
        return max_drawdown(returns=self._returns)

    cpdef float annual_volatility(self):
        """
        Get the annual volatility for the portfolio.
        
        :return float.
        """
        return annual_volatility(returns=self._returns)

    cpdef float sharpe_ratio(self):
        """
        Get the sharpe ratio for the portfolio.
        
        :return float.
        """
        return sharpe_ratio(returns=self._returns)

    cpdef float calmar_ratio(self):
        """
        Get the calmar ratio for the portfolio.
        
        :return float.
        """
        return calmar_ratio(returns=self._returns)

    cpdef float sortino_ratio(self):
        """
        Get the sortino ratio for the portfolio.
        
        :return float.
        """
        return sortino_ratio(returns=self._returns)

    cpdef float omega_ratio(self):
        """
        Get the omega ratio for the portfolio.
        
        :return float.
        """
        return omega_ratio(returns=self._returns)

    cpdef float stability_of_timeseries(self):
        """
        Get the stability of time series for the portfolio.
        
        :return float.
        """
        return stability_of_timeseries(returns=self._returns)

    cpdef float returns_mean(self):
        """
        Get the returns mean for the portfolio.
        
        :return float.
        """
        return np.mean(self._returns)

    cpdef float returns_variance(self):
        """
        Get the returns variance for the portfolio.
        
        :return float.
        """
        return np.var(self._returns)

    cpdef float returns_skew(self):
        """
        Get the returns skew for the portfolio.
        
        :return float.
        """
        return skew(self._returns)

    cpdef float returns_kurtosis(self):
        """
        Get the returns kurtosis for the portfolio.
        
        :return float.
        """
        return kurtosis(self._returns)

    cpdef float returns_tail_ratio(self):
        """
        Get the returns tail ratio for the portfolio.
        
        :return float.
        """
        return tail_ratio(self._returns)

    cpdef float alpha(self):
        """
        Get the alpha for the portfolio.
        
        :return float.
        """
        return alpha(returns=self._returns, factor_returns=self._returns)

    cpdef float beta(self):
        """
        Get the beta for the portfolio.
    
        :return float.
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
        
        :return Dict[str, float].
        """
        return {
            'PNL': self.total_pnl().as_float(),
            'PNL%': self.total_pnl_percentage(),
            'MaxWinner': self.max_winner().value,
            'AvgWinner': self.avg_winner().value,
            'MinWinner': self.min_winner().value,
            'MinLoser': self.min_loser().value,
            'AvgLoser': self.avg_loser().value,
            'MaxLoser': self.max_loser().value,
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

    cdef list get_performance_stats_formatted(self):
        """
        Return the performance statistics from the last backtest run formatted 
        for printing in the backtest run footer.
        
        :return List[str].
        """
        cdef str account_currency = currency_to_string(self._account.currency) if self._account is not None else '?'

        return [
            f"PNL:               {self.total_pnl()} {account_currency}",
            f"PNL %:             {self._format_stat(self.total_pnl_percentage())}%",
            f"Max Winner:        {self.max_winner()} {account_currency}",
            f"Avg Winner:        {self.avg_winner()} {account_currency}",
            f"Min Winner:        {self.min_winner()} {account_currency}",
            f"Min Loser:         {self.min_loser()} {account_currency}",
            f"Avg Loser:         {self.avg_loser()} {account_currency}",
            f"Max Loser:         {self.max_loser()} {account_currency}",
            f"Win Rate:          {self._format_stat(self.win_rate(), decimals=4)}",
            f"Expectancy:        {self.expectancy()} {account_currency}",
            f"Annual return:     {self._format_stat(self.annual_return() * 100)}%",
            f"Cum returns:       {self._format_stat(self.cum_return() * 100)}%",
            f"Max drawdown:      {self._format_stat(self.max_drawdown_return() * 100)}%",
            f"Annual vol:        {self._format_stat(self.annual_volatility() * 100)}%",
            f"Sharpe ratio:      {self._format_stat(self.sharpe_ratio())}",
            f"Calmar ratio:      {self._format_stat(self.calmar_ratio())}",
            f"Sortino ratio:     {self._format_stat(self.sortino_ratio())}",
            f"Omega ratio:       {self._format_stat(self.omega_ratio())}",
            f"Stability:         {self._format_stat(self.stability_of_timeseries())}",
            f"Returns Mean:      {self._format_stat(self.returns_mean(), decimals=5)}",
            f"Returns Variance:  {self._format_stat(self.returns_variance(), decimals=8)}",
            f"Returns Skew:      {self._format_stat(self.returns_skew())}",
            f"Returns Kurtosis:  {self._format_stat(self.returns_kurtosis())}",
            f"Tail ratio:        {self._format_stat(self.returns_tail_ratio())}",
            f"Alpha:             {self._format_stat(self.alpha())}",
            f"Beta:              {self._format_stat(self.beta())}"
        ]

    cdef str _format_stat(self, float value, int decimals=2):
        return f'{value:.{decimals}f}'
