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

from inv_trader.model.objects cimport Tick

cdef class SpreadAnalyzer:
    """
    Provides a means of analyzing the spread in a market and tracking various
    metrics.
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
