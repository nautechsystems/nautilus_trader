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

from typing import List, Dict
from cpython.datetime cimport date
from pyfolio.tears import create_full_tear_sheet

from inv_trader.model.objects import Symbol


cdef class Analyzer:
    """
    Represents a trading portfolio analyzer for generating performance metrics
    and statistics.
    """

    def __init__(self):
        """
        Initializes a new instance of the Analyzer class.
        """
        self._returns = pd.Series()
        self._positions_symbols = []        # type: List[Symbol]
        self._positions_columns = ['cash']  # type: List[str]
        self._positions = pd.DataFrame(columns=self._positions_columns)
        self._transactions = pd.DataFrame(columns=['amount', 'price', 'symbol'])
        self._last_day_analyzed = 0

    cpdef void initialize_day(self, date d):
        """
        Initialize the analyzer last day analyzed.
        
        :param d: The current date.
        """
        self._last_day_analyzed = d.day

    cpdef void add_daily_returns(self, date d, float returns):
        """
        Add daily returns data to the analyzer.
        
        :param d: The date for the returns entry.
        :param returns: The return for the day.
        """
        self._returns.loc[d] = returns

    cpdef void add_daily_positions(self, date d, list positions, float cash):
        """
        Add daily positions to the analyzer.

        :param d: The date for the positions entry.
        :param positions: The active positions.
        :param cash: The cash balance at the end of day.
        """
        cdef dict symbol_quantities = {}  # type: Dict[Symbol, int]

        for position in positions:
            if position.symbol not in self._positions_symbols:
                self._positions_symbols.append(position.symbol)
                self._positions_columns.append(str(position.symbol))
            if position.symbol not in symbol_quantities:
                symbol_quantities[position.symbol] = 0
            symbol_quantities[position.symbol] += position.relative_quantity

        self._positions_columns.sort()
        self._positions = self._positions[self._positions_columns]

        self._positions.loc[d]['cash'] = cash

        for symbol, quantity in symbol_quantities.items():
            self._positions.loc[d][str(symbol)] = quantity

    cpdef object get_returns(self):
        return self._returns

    cpdef object get_positions(self):
        return self._positions

    cpdef object get_transactions(self):
        return self._transactions
