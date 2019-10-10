# -------------------------------------------------------------------------------------------------
# <copyright file="data.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import pandas as pd

from pandas import Series, DataFrame

from nautilus_trader.data.loaders import CSVTickDataLoader, CSVBarDataLoader
from test_kit.__info__ import PACKAGE_ROOT



class TestDataProvider:

    @staticmethod
    def usdjpy_test_ticks() -> DataFrame:
        return CSVTickDataLoader.load(PACKAGE_ROOT + '/data/USDJPY_ticks.csv')

    @staticmethod
    def gbpusd_1min_bid() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + '/data/GBPUSD_1 Min_Bid.csv')

    @staticmethod
    def usdjpy_1min_bid() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + '/data/USDJPY_1 Min_Bid.csv')

    @staticmethod
    def usdjpy_1min_ask() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + '/data/USDJPY_1 Min_Ask.csv')
