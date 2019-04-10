#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="backtest_example.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import logging
import matplotlib.pyplot as plt
from pandas.plotting import register_matplotlib_converters
from datetime import datetime, timezone

from inv_trader.model.enums import Resolution, Currency
from inv_trader.backtest.engine import BacktestConfig, BacktestEngine
from test_kit.strategies import EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

register_matplotlib_converters()

if __name__ == "__main__":
    usdjpy = TestStubs.instrument_usdjpy()
    bid_data_1min = TestDataProvider.usdjpy_1min_bid()
    ask_data_1min = TestDataProvider.usdjpy_1min_ask()

    instruments = [TestStubs.instrument_usdjpy()]
    tick_data = {usdjpy.symbol: pd.DataFrame()}
    bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
    ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

    strategies = [EMACross(
        label='001',
        id_tag_trader='001',
        id_tag_strategy='001',
        instrument=usdjpy,
        bar_type=TestStubs.bartype_usdjpy_1min_bid(),
        risk_bp=10,
        fast_ema=10,
        slow_ema=20,
        atr_period=20,
        sl_atr_multiple=2.0)]

    config = BacktestConfig(
        starting_capital=1000000,
        account_currency=Currency.USD,
        slippage_ticks=1,
        level_console=logging.INFO,
        log_thread=False,
        log_to_file=False)

    engine = BacktestEngine(
        instruments=instruments,
        data_ticks=tick_data,
        data_bars_bid=bid_data,
        data_bars_ask=ask_data,
        strategies=strategies,
        config=config)

    start = datetime(2013, 11, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
    stop = datetime(2013, 12, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

    engine.run(start, stop)
    #engine.create_full_tear_sheet()

    #equity_curve = engine.portfolio.analyzer.get_equity_curve()

    #plt.plot(equity_curve['capital'])
    #plt.show()

    input("Press Enter to continue...")

    engine.reset()

    input("Press Enter to continue...")
    engine.run(start, stop)
