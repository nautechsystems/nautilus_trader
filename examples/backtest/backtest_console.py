#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import pandas as pd
from datetime import datetime

from nautilus_trader.model.enums import BarStructure, Currency, PriceType
from nautilus_trader.model.objects import BarSpecification
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.backtest.data import BacktestDataContainer
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.engine import BacktestEngine

from tests.test_kit.data import TestDataProvider
from tests.test_kit.stubs import TestStubs

from examples.strategies.ema_cross import EMACross


if __name__ == "__main__":
    USDJPY = TestStubs.instrument_usdjpy()

    data = BacktestDataContainer()
    data.add_instrument(USDJPY)
    data.add_bars(
        USDJPY.symbol,
        BarStructure.MINUTE,
        PriceType.BID,
        TestDataProvider.usdjpy_1min_bid())
    data.add_bars(
        USDJPY.symbol,
        BarStructure.MINUTE,
        PriceType.ASK,
        TestDataProvider.usdjpy_1min_ask())

    strategies = [EMACross(
        symbol=USDJPY.symbol,
        bar_spec=BarSpecification(1, BarStructure.MINUTE, PriceType.BID),
        risk_bp=10,
        fast_ema=10,
        slow_ema=20,
        atr_period=20,
        sl_atr_multiple=2.0)]

    config = BacktestConfig(
        exec_db_type='in-memory',
        exec_db_flush=False,
        frozen_account=False,
        starting_capital=1000000,
        account_currency=Currency.USD,
        short_term_interest_csv_path='default',
        commission_rate_bp=0.20,
        bypass_logging=False,
        level_console=LogLevel.INFO,
        level_file=LogLevel.DEBUG,
        level_store=LogLevel.WARNING,
        log_thread=False,
        log_to_file=False)

    fill_model = FillModel(
        prob_fill_at_limit=0.2,
        prob_fill_at_stop=0.95,
        prob_slippage=0.5,
        random_seed=None)

    engine = BacktestEngine(
        data=data,
        strategies=strategies,
        config=config,
        fill_model=fill_model)

    input("Press Enter to continue...")

    start = datetime(2013, 2, 1, 0, 0, 0, 0)
    stop = datetime(2013, 3, 1, 0, 0, 0, 0)

    engine.run(start, stop)

    with pd.option_context(
            'display.max_rows',
            100,
            'display.max_columns',
            None,
            'display.width', 300):
        pass
        print(engine.trader.generate_account_report())
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    engine.dispose()
