#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_trader.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import unittest

from inv_trader.common.account import Account
from inv_trader.common.brokerage import CommissionCalculator
from inv_trader.common.clock import TestClock
from inv_trader.common.guid import TestGuidFactory
from inv_trader.common.logger import TestLogger
from inv_trader.model.enums import Resolution
from inv_trader.model.objects import Quantity, Symbol, Price, Money
from inv_trader.backtest.execution import BacktestExecClient
from inv_trader.backtest.models import FillModel
from inv_trader.backtest.data import BacktestDataClient
from inv_trader.portfolio.portfolio import Portfolio
from inv_trader.trader import Trader

from test_kit.strategies import EmptyStrategy
from test_kit.stubs import TestStubs
from test_kit.data import TestDataProvider

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = TestStubs.instrument_usdjpy().symbol


class TraderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        bid_data_1min = TestDataProvider.usdjpy_1min_bid().iloc[:2000]
        ask_data_1min = TestDataProvider.usdjpy_1min_ask().iloc[:2000]
        test_clock = TestClock()

        data_client = BacktestDataClient(
            instruments=[TestStubs.instrument_usdjpy()],
            data_ticks={USDJPY_FXCM: pd.DataFrame()},
            data_bars_bid={USDJPY_FXCM: {Resolution.MINUTE: bid_data_1min}},
            data_bars_ask={USDJPY_FXCM: {Resolution.MINUTE: ask_data_1min}},
            clock=test_clock,
            logger=TestLogger())

        account = Account()
        portfolio = Portfolio(
            clock=TestClock(),
            guid_factory=TestGuidFactory(),
            logger=TestLogger())

        exec_client = BacktestExecClient(
            instruments=[TestStubs.instrument_usdjpy()],
            frozen_account=False,
            starting_capital=Money(1000000),
            fill_model=FillModel(),
            commission_calculator=CommissionCalculator(),
            account=account,
            portfolio=portfolio,
            clock=TestClock(),
            guid_factory=TestGuidFactory(),
            logger=TestLogger())

        strategies = [EmptyStrategy('001'),
                      EmptyStrategy('002')]

        self.trader = Trader(
            'BT',
            strategies=strategies,
            data_client=data_client,
            exec_client=exec_client,
            account=account,
            portfolio=portfolio,
            clock=test_clock,
            logger=TestLogger())

    def test_trader_detects_none_unique_identifiers(self):
        # Arrange
        strategies = [EmptyStrategy('001'),
                      EmptyStrategy('002')]

        # Act
        #self.assertRaises(ValueError, self.trader.change_strategies, strategies)

