# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_data.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import pandas as pd

from datetime import datetime, timezone, timedelta
from pandas import Timestamp

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.model.enums import BarStructure
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.backtest.data import BacktestDataClient

from test_kit.mocks import ObjectStorer
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = TestStubs.instrument_usdjpy().symbol


class BacktestDataClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = TestStubs.instrument_usdjpy()
        self.bid_data_1min = TestDataProvider.usdjpy_1min_bid().iloc[:2000]
        self.ask_data_1min = TestDataProvider.usdjpy_1min_ask().iloc[:2000]
        self.test_clock = TestClock()
