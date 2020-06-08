# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from pandas import DataFrame

from nautilus_trader.backtest.loaders import CSVTickDataLoader, CSVBarDataLoader

from tests.test_kit import PACKAGE_ROOT


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
