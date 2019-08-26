#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------

# <copyright file="backtest_example.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd

from datetime import datetime

from nautilus_trader.common.logger import LogLevel
from nautilus_trader.model.enums import Resolution, Currency
from nautilus_trader.model.identifiers import Venue, TraderId
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.engine import BacktestEngine
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs
from test_kit.strategies import EMACross


if __name__ == "__main__":
    usdjpy = TestStubs.instrument_usdjpy()
    bid_data_1min = TestDataProvider.usdjpy_1min_bid()
    ask_data_1min = TestDataProvider.usdjpy_1min_ask()

    instruments = [TestStubs.instrument_usdjpy()]
    tick_data = {usdjpy.symbol: pd.DataFrame()}
    bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
    ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

    strategies = [EMACross(
        instrument=usdjpy,
        bar_type=TestStubs.bartype_usdjpy_1min_bid(),
        risk_bp=10,
        fast_ema=10,
        slow_ema=20,
        atr_period=20,
        sl_atr_multiple=2.0)]

    config = BacktestConfig(
        frozen_account=False,
        starting_capital=1000000,
        account_currency=Currency.USD,
        bypass_logging=False,
        level_console=LogLevel.DEBUG,
        level_store=LogLevel.WARNING,
        log_thread=False,
        log_to_file=False)

    fill_model = FillModel(
        prob_fill_at_limit=0.2,
        prob_fill_at_stop=0.95,
        prob_slippage=0.5,
        random_seed=None)

    engine = BacktestEngine(
        trader_id=TraderId('BACKTESTER', '001'),
        venue=Venue('FXCM'),
        instruments=instruments,
        data_ticks=tick_data,
        data_bars_bid=bid_data,
        data_bars_ask=ask_data,
        strategies=strategies,
        fill_model=fill_model,
        config=config)

    input("Press Enter to continue...")

    start = datetime(2013, 2, 1, 0, 0, 0, 0)
    stop = datetime(2013, 3, 1, 1, 0, 0, 0)

    engine.run(start, stop)
    print(engine.get_order_fills_report())
    print(engine.get_positions_report())

    input("Press Enter to continue...")

    engine.reset()

    input("Press Enter to continue...")
    engine.run(start, stop)
