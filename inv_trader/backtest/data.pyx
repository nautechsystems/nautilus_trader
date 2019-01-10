#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from inv_trader.common.data cimport DataClient


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for the BacktestEngine.
    """

    def __init__(self,
                 list instruments,
                 dict data):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param instruments: The instruments needed for the backtest.
        :param data: The historical market data needed for the backtest.
        """
        self.instruments = instruments
        self.historical_data = data

