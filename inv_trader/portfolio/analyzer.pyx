#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="analyzer.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import numpy as np
import pandas as pd

from math import log
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

from inv_trader.enums.order_side cimport OrderSide
from inv_trader.model.events cimport AccountEvent
from inv_trader.model.objects cimport Money


cdef class Analyzer:
    """
    Represents a trading portfolio analyzer for generating performance metrics
    and statistics.
    """

    def __init__(self, log_returns=False):
        """
        Initializes a new instance of the Analyzer class.

        :param log_returns: A boolean flag indicating whether log returns will be used.
        """
        self._log_returns = log_returns
        self._returns = pd.Series()
        self._positions = pd.DataFrame(columns=['cash'])
        self._transactions = pd.DataFrame(columns=['amount'])
        #self._transactions = pd.DataFrame(columns=['amount', 'price', 'symbol'])
        self._equity_curve = pd.DataFrame(columns=['capital', 'pnl'])

    cpdef void add_return(self, datetime time, float value):
        """
        Add return data to the analyzer.
        
        :param time: The timestamp for the returns entry.
        :param value: The return value to add.
        """
        if self._log_returns:
            value = log(value)

        cdef date index_date = pd.to_datetime(time.date())
        if index_date not in self._returns:
            self._returns.loc[index_date] = 0.0

        self._returns.loc[index_date] += value

    cpdef void add_positions(
            self,
            datetime time,
            list positions,
            Money cash_balance):
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

        # TODO: Cash not being added??
        self._positions.loc[index_date]['cash'] = cash_balance.value

    cpdef void add_transaction(self, datetime time, Money account_capital, Money pnl):
        """
        Add a transaction to the analyzer.
        
        :param time: The timestamp for the transaction entry.
        :param account_capital: The account capital after the transaction.
        :param pnl: The profit/loss for the transaction.
        """
        cdef datetime index = pd.to_datetime(time)

        if time not in self._equity_curve:
            self._equity_curve.loc[time] = 0

        self._equity_curve.loc[time]['capital'] = account_capital.value
        self._equity_curve.loc[time]['pnl'] = pnl.value

    # cpdef void add_transaction(self, OrderEvent event):
    #     """
    #     Add transaction data to the analyzer.
    #
    #     :param event: The transaction event.
    #     """
    #     cdef datetime index_datetime = pd.to_datetime(event.timestamp)
    #     if index_datetime in self._transactions:
    #         index_datetime += timedelta(milliseconds=1)
    #
    #     cdef int quantity
    #     if event.order_side == OrderSide.BUY:
    #         quantity = event.filled_quantity.value
    #     else:
    #         quantity = -event.filled_quantity.value
    #
    #     self._transactions.loc[index_datetime] = [quantity, str(event.average_price), str(event.symbol)]

    cpdef object get_returns(self):
        """
        Return the returns data.
        
        :return: Pandas.Series.
        """
        return self._returns

    cpdef object get_positions(self):
        """
        Return the positions data.
        
        :return: Pandas.DataFrame.
        """
        return self._positions

    cpdef object get_transactions(self):
        """
        Return the transactions data.
        
        :return: Pandas.DataFrame.
        """
        return self._transactions

    cpdef object get_equity_curve(self):
        """
        Return the transactions data.
        
        :return: Pandas.DataFrame.
        """
        return self._equity_curve

    cpdef float annual_return(self):
        """
        Get the annual return for the portfolio.
        
        :return: float.
        """
        return annual_return(returns=self._returns)

    cpdef float cum_return(self):
        """
        Get the cumulative return for the portfolio.
        
        :return: float.
        """
        return cum_returns_final(returns=self._returns)

    cpdef float max_drawdown_return(self):
        """
        Get the maximum return drawdown for the portfolio.
        
        :return: float.
        """
        return max_drawdown(returns=self._returns)

    cpdef float annual_volatility(self):
        """
        Get the annual volatility for the portfolio.
        
        :return: float.
        """
        return annual_volatility(returns=self._returns)

    cpdef float sharpe_ratio(self):
        """
        Get the sharpe ratio for the portfolio.
        
        :return: float.
        """
        return sharpe_ratio(returns=self._returns)

    cpdef float calmar_ratio(self):
        """
        Get the calmar ratio for the portfolio.
        
        :return: float.
        """
        return calmar_ratio(returns=self._returns)

    cpdef float sortino_ratio(self):
        """
        Get the sortino ratio for the portfolio.
        
        :return: float.
        """
        return sortino_ratio(returns=self._returns)

    cpdef float omega_ratio(self):
        """
        Get the omega ratio for the portfolio.
        
        :return: float.
        """
        return omega_ratio(returns=self._returns)

    cpdef float stability_of_timeseries(self):
        """
        Get the stability of timeseries for the portfolio.
        
        :return: float.
        """
        return stability_of_timeseries(returns=self._returns)

    cpdef float returns_mean(self):
        """
        Get the returns mean for the portfolio.
        
        :return: float.
        """
        return np.mean(self._returns)

    cpdef float returns_variance(self):
        """
        Get the returns variance for the portfolio.
        
        :return: float.
        """
        return np.var(self._returns)

    cpdef float returns_skew(self):
        """
        Get the returns skew for the portfolio.
        
        :return: float.
        """
        return skew(self._returns)

    cpdef float returns_kurtosis(self):
        """
        Get the returns kurtosis for the portfolio.
        
        :return: float.
        """
        return kurtosis(self._returns)

    cpdef float returns_tail_ratio(self):
        """
        Get the returns nail ratio for the portfolio.
        
        :return: float.
        """
        return tail_ratio(self._returns)

    cpdef float alpha(self):
        """
        Get the alpha for the portfolio.
        
        :return: float.
        """
        return alpha(returns=self._returns, factor_returns=self._returns)

    cpdef float beta(self):
        """
        Get the beta for the portfolio.
    
        :return: float.
        """
        return beta(returns=self._returns, factor_returns=self._returns)

    cpdef void create_returns_tear_sheet(self):
        """
        Create a pyfolio returns tear sheet based on analyzer data from the last run.
        """
        # Do nothing

    cpdef void create_full_tear_sheet(self):
        """
        Create a pyfolio full tear sheet based on analyzer data from the last run.
        """
        # Do nothing
