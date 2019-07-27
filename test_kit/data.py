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

ROOT_DIR = os.path.dirname(os.path.abspath(__file__))


class TestDataProvider:

    @staticmethod
    def usdjpy_test_ticks() -> DataFrame:
        return pd.read_csv(os.path.join(ROOT_DIR, 'USDJPY_ticks.csv'),
                           usecols=[1, 2, 3],
                           index_col=0,
                           header=None,
                           parse_dates=True)

    @staticmethod
    def gbpusd_1min_bid() -> DataFrame:
        return pd.read_csv(os.path.join(ROOT_DIR, 'GBPUSD_1 Min_Bid.csv'),
                           index_col='Time (UTC)',
                           parse_dates=True)

    @staticmethod
    def usdjpy_1min_bid() -> DataFrame:
        return pd.read_csv(os.path.join(ROOT_DIR, 'USDJPY_1 Min_Bid.csv'),
                           index_col='Time (UTC)',
                           parse_dates=True)

    @staticmethod
    def usdjpy_1min_ask() -> DataFrame:
        return pd.read_csv(os.path.join(ROOT_DIR, 'USDJPY_1 Min_Ask.csv'),
                           index_col='Time (UTC)',
                           parse_dates=True)

    # @staticmethod
    # def test_returns() -> Series:
    #     data = read_csv(gzip.open(os.path.join(ROOT_DIR, 'test_returns.csv.gz')),
    #                     index_col=0,
    #                     parse_dates=True)
    #     return to_series(to_utc(data))
    #
    # @staticmethod
    # def test_positions() -> DataFrame:
    #     data = read_csv(gzip.open(os.path.join(ROOT_DIR, 'test_positions.csv.gz')),
    #                     index_col=0,
    #                     parse_dates=True)
    #     return to_utc(data)
