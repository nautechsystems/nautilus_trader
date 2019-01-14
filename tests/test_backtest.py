#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from inv_trader.model.enums import Resolution
from inv_trader.model.objects import BarType
from inv_trader.backtest.data import BacktestDataClient
from test_kit.strategies import TestStrategy1
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs


class BacktestDataClientTests(unittest.TestCase):

    def test_can_initialize_client_with_data(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        client = BacktestDataClient(instruments=instruments,
                                    bid_data=bid_data,
                                    ask_data=ask_data)

        # Act

        # Assert