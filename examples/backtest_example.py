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

from datetime import datetime, timezone

from inv_trader.model.enums import Resolution, Currency
from inv_trader.backtest.models import FillModel
from inv_trader.backtest.config import BacktestConfig
from inv_trader.backtest.engine import BacktestEngine
from test_kit.strategies import EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs


if __name__ == "__main__":
    usdjpy = TestStubs.instrument_usdjpy()
    bid_data_1min = TestDataProvider.usdjpy_1min_bid()
    ask_data_1min = TestDataProvider.usdjpy_1min_ask()

    instruments = [TestStubs.instrument_usdjpy()]
    tick_data = {usdjpy.symbol: pd.DataFrame()}
    bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
    ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

    strategies = [EMACross(
        order_id_tag='001',
        flatten_on_sl_reject=True,
        flatten_on_stop=True,
        cancel_all_orders_on_stop=True,
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
        bypass_logging=False,
        level_console=logging.INFO,
        level_store=logging.WARNING,
        log_thread=False,
        log_to_file=False)

    fill_model = FillModel(
        prob_fill_at_limit=0.2,
        prob_fill_at_stop=0.95,
        prob_slippage=0.5,
        random_seed=None)

    engine = BacktestEngine(
        instruments=instruments,
        data_ticks=tick_data,
        data_bars_bid=bid_data,
        data_bars_ask=ask_data,
        strategies=strategies,
        fill_model=fill_model,
        config=config)

    input("Press Enter to continue...")

    start = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
    stop = datetime(2013, 3, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

    engine.run(start, stop)

    input("Press Enter to continue...")

    engine.reset()

    input("Press Enter to continue...")
    engine.run(start, stop)
