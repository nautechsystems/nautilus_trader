#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import pandas as pd

from pandas import DataFrame

ROOT_DIR = os.path.dirname(os.path.abspath(__file__))


class TestDataProvider:

    @staticmethod
    def gbpusd_1min_bid() -> DataFrame:
        return pd.read_csv(os.path.join(ROOT_DIR, 'GBPUSD_1 Min_Bid.csv'),
                           index_col='Time (UTC)')

    @staticmethod
    def usdjpy_1min_bid() -> DataFrame:
        return pd.read_csv(os.path.join(ROOT_DIR, 'USDJPY_1 Min_Bid.csv'),
                           index_col='Time (UTC)')

    @staticmethod
    def usdjpy_1min_ask() -> DataFrame:
        return pd.read_csv(os.path.join(ROOT_DIR, 'USDJPY_1 Min_Ask.csv'),
                           index_col='Time (UTC)')
