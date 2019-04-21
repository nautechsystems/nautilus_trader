#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="analyzers.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from collections import deque
from decimal import Decimal
from typing import List, Deque

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.objects cimport Tick


cdef class SpreadAnalyzer:
    """
    Provides a means of analyzing the spread of a market and track various metrics.
    """

    def __init__(self, int decimal_precision, int average_spread_capacity=100):
        """
        Initializes a new instance of the SpreadAnalyzer class.
        """
        self._decimal_precision = decimal_precision
        self._average_spread_capacity = average_spread_capacity
        self._spreads = []                                                   # type: List[Decimal]
        self._average_spreads = deque(maxlen=self._average_spread_capacity)  # type: Deque[Decimal]
        self.initialized = False
        self.average = Decimal(0)

    cpdef void update(self, Tick tick):
        """
        Update the analyzer with the given tick.
        
        :param tick: The tick to update with.
        """
        self._spreads.append(tick.ask - tick.bid)

        if not self.initialized:
            self._calculate_average()

    cpdef void snapshot_average(self):
        """
        Take a snapshot of the average spread from the current list of spreads.
        """
        self._calculate_average()
        self._average_spreads.append(self.average)
        self._spreads = []  # type: List[Decimal]

        if not self.initialized:
            if self.average != Decimal(0):
                self.initialized = True

    cpdef list get_average_spreads(self):
        """
        Return a list of average spread snapshots.

        :return: List[Decimal].
        """
        return list(self._average_spreads)

    cpdef void reset(self):
        """
        Reset the spread analyzer by clearing all internally held values and 
        returning it to a fresh state.
        """
        self._spreads = []                                                   # type: List[Decimal]
        self._average_spreads = deque(maxlen=self._average_spread_capacity)  # type: Deque[Decimal]
        self.initialized = False
        self.average = Decimal(0)

    cdef void _calculate_average(self):
        """
        Calculate and set the average spread then reset the list of spreads. 
        """
        self.average = Decimal(round(sum(self._spreads) / max(1, len(self._spreads)), self._decimal_precision))


cdef class LiquidityAnalyzer:
    """
    Provides a means of analyzing the liquidity of a market and track various metrics.
    """

    def __init__(self, float liquidity_threshold=2.0):
        """
        Initializes a new instance of the LiquidityAnalyzer class.

        :param liquidity_threshold: The multiple of spread to average volatility
        which constitutes a liquid market (> 0) (default=2.0).
        :raises ValueError: If the liquidity threshold is not positive (> 0).
        """
        Precondition.positive(liquidity_threshold, 'liquidity_threshold')

        self.liquidity_threshold = liquidity_threshold
        self.value = 0.0
        self.initialized = False
        self.is_liquid = False
        self.is_not_liquid = True

    cpdef void update(self, average_spread, float volatility):
        """
        Update the analyzer with the current average spread and volatility
        measurement.
        
        Note: The suggested value for volatility is the current average true range (ATR).
        :param average_spread: The current average spread of the market.
        :param volatility: The current volatility of the market.
        :raises ValueError: If the volatility is not positive (> 0).
        """
        Precondition.positive(volatility, 'volatility')

        self.value = volatility / float(average_spread)

        if self.value >= self.liquidity_threshold:
            self.is_liquid = True
            self.is_not_liquid = False
        else:
            self.is_liquid = False
            self.is_not_liquid = True

        if not self.initialized:
            self.initialized = True

    cpdef void reset(self):
        """
        Reset the spread analyzer by clearing all internally held values and 
        returning it to a fresh state.
        """
        self.value = 0.0
        self.initialized = False
        self.is_liquid = False
        self.is_not_liquid = True
