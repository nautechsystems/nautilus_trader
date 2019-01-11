#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from inv_trader.model.objects import BarType
from inv_trader.backtest.data import BacktestDataClient
from test_kit.strategies import TestStrategy1
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs


class BacktestDataClientTests(unittest.TestCase):

    def test_can_initialize_client_with_data(self):
        # Arrange
        data_bid = TestDataProvider.usdjpy_1min_bid()
        data_ask = TestDataProvider.usdjpy_1min_ask()
        bartype_bid = TestStubs.bartype_usdjpy_1min_bid()
        bartype_ask = TestStubs.bartype_usdjpy_1min_ask()

        instrument = TestStubs.instrument_usdjpy()
        data = {bartype_bid: data_bid,
                bartype_ask: data_ask}

        client = BacktestDataClient([instrument], data)

        # Act

        # Assert