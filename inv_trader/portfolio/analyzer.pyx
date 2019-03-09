#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="analyzer.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import pandas as pd

from math import log
from typing import List, Dict
from cpython.datetime cimport datetime, timedelta
from pyfolio.tears import create_simple_tear_sheet, create_returns_tear_sheet, create_full_tear_sheet

from inv_trader.enums.order_side cimport OrderSide
from inv_trader.model.objects cimport Symbol
from inv_trader.model.position cimport Position
from inv_trader.model.events cimport OrderEvent


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
        self._positions_symbols = []        # type: List[Symbol]
        self._positions_columns = ['cash']  # type: List[str]
        self._positions = pd.DataFrame(columns=self._positions_columns)
        self._transactions = pd.DataFrame(columns=['amount', 'price', 'symbol'])

    cpdef void add_return(self, datetime time, float value):
        """
        Add return data to the analyzer.
        
        :param time: The timestamp for the returns entry.
        :param value: The return value to add.
        """
        if self._log_returns:
            value = log(value)

        timestamp = pd.to_datetime(time.date())
        if timestamp not in self._returns:
            self._returns.loc[timestamp] = 0.0

        self._returns.loc[timestamp] += value

    # cpdef void add_position(self, datetime time, Position position):
    #     """
    #     Add position data to the analyzer.
    #
    #     :param time: The timestamp for the position entry.
    #     :param position: The position.
    #     """
    #     cdef dict symbol_quantities = {}  # type: Dict[Symbol, int]
    #     pandas_timestamp = pd.to_datetime(time.date())
    #     if pandas_timestamp not in self._positions:
    #         self._positions.loc[pandas_timestamp] = 0.0
    #
    #     for position in positions:
    #         if position.symbol not in self._positions_symbols:
    #             self._positions_symbols.append(position.symbol)
    #             self._positions_columns.append(str(position.symbol))
    #         if position.symbol not in symbol_quantities:
    #             symbol_quantities[position.symbol] = 0
    #         symbol_quantities[position.symbol] += position.relative_quantity
    #
    #     self._positions_columns.sort()
    #     self._positions = self._positions[self._positions_columns]
    #
    #     self._positions.loc[pandas_timestamp]['cash'] = cash
    #
    #     for symbol, quantity in symbol_quantities.items():
    #         self._positions.loc[pandas_timestamp][str(symbol)] = quantity

    cpdef void add_transaction(self, OrderEvent event):
        """
        Add transaction data to the analyzer.

        :param event: The transaction event.
        """
        timestamp = pd.to_datetime(event.timestamp)

        if timestamp in self._transactions:
            timestamp += timedelta(milliseconds=1)

        cdef int quantity
        if event.order_side == OrderSide.BUY:
            quantity = event.filled_quantity.value
        else:
            quantity = -event.filled_quantity.value

        self._transactions.loc[timestamp] = [quantity, str(event.average_price), str(event.symbol)]

    cpdef object get_returns(self):
        return self._returns

    cpdef object get_positions(self):
        return self._positions

    cpdef object get_transactions(self):
        return self._transactions

    cpdef void create_returns_tear_sheet(self):
        """
        Create a pyfolio returns tear sheet based on analyzer data from the last run.
        """
        create_returns_tear_sheet(returns=self._returns,
                                  transactions=self._transactions,
                                  benchmark_rets=self._returns,
                                  bootstrap=True,
                                  cone_std=1)

    cpdef void create_full_tear_sheet(self):
        """
        Create a pyfolio full tear sheet based on analyzer data from the last run.
        """
        create_full_tear_sheet(returns=self._returns,
                               transactions=self._transactions,
                               benchmark_rets=self._returns,
                               bootstrap=True,
                               cone_std=1)
