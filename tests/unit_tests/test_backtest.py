#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import unittest

from datetime import datetime, timezone, timedelta

from inv_trader.core.decimal import Decimal
from inv_trader.common.clock import TestClock
from inv_trader.common.logger import Logger
from inv_trader.model.enums import Resolution
from inv_trader.model.enums import Venue, OrderSide, OrderStatus, TimeInForce
from inv_trader.model.identifiers import Label, OrderId, PositionId
from inv_trader.model.objects import Price, Symbol
from inv_trader.model.objects import Symbol, Money
from inv_trader.strategy import TradeStrategy
from inv_trader.backtest.data import BacktestDataClient
from inv_trader.backtest.execution import BacktestExecClient
from inv_trader.backtest.engine import BacktestConfig, BacktestEngine
from test_kit.objects import ObjectStorer
from test_kit.strategies import TestStrategy1, EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = Symbol('USDJPY', Venue.FXCM)


# -- DATA ---------------------------------------------------------------------------------------- #
class BacktestDataClientTests(unittest.TestCase):

    def test_can_initialize_client_with_data(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        # Act
        client = BacktestDataClient(instruments=instruments,
                                    tick_data=tick_data,
                                    bar_data_bid=bid_data,
                                    bar_data_ask=ask_data,
                                    clock=TestClock(),
                                    logger=Logger())

        # Assert
        self.assertEqual(all(bid_data_1min), all(client.bar_data_bid[usdjpy.symbol][Resolution.MINUTE]))
        self.assertEqual(all(ask_data_1min), all(client.bar_data_bid[usdjpy.symbol][Resolution.MINUTE]))

    def test_can_iterate_bar_data(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        client = BacktestDataClient(instruments=instruments,
                                    tick_data=tick_data,
                                    bar_data_bid=bid_data,
                                    bar_data_ask=ask_data,
                                    clock=TestClock(),
                                    logger=Logger())

        receiver = ObjectStorer()
        client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), receiver.store_2)

        start_datetime = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        for x in range(1000):
            client.iterate(start_datetime + timedelta(minutes=x))

        # Assert
        self.assertEqual(1000, len(receiver.get_store()))


# -- EXECUTION ----------------------------------------------------------------------------------- #
class BacktestExecClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        self.instruments = [TestStubs.instrument_usdjpy()]
        self.tick_data = {self.usdjpy.symbol: pd.DataFrame()}
        self.bid_data = {self.usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        self.ask_data = {self.usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        self.strategies = [TestStrategy1(TestStubs.bartype_usdjpy_1min_bid())]

        self.client = BacktestExecClient(instruments=self.instruments,
                                         tick_data=self.tick_data,
                                         bar_data_bid=self.bid_data,
                                         bar_data_ask=self.ask_data,
                                         starting_capital=Money.create(1000000),
                                         slippage_ticks=1,
                                         clock=TestClock(),
                                         logger=Logger())

    def test_can_initialize_client_with_data(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(all(self.bid_data), all(self.client.bar_data_bid[self.usdjpy.symbol][Resolution.MINUTE]))
        self.assertEqual(all(self.ask_data), all(self.client.bar_data_bid[self.usdjpy.symbol][Resolution.MINUTE]))
        self.assertEqual(Decimal(1000000), self.client.account.cash_balance)
        self.assertEqual(Decimal(1000000), self.client.account.free_equity)
        self.assertEqual(Decimal('0.001'), self.client.slippage_index[self.usdjpy.symbol])

    def test_can_send_collateral_inquiry(self):
        # Arrange
        # Act
        self.client.collateral_inquiry()

        # Assert
        self.assertEqual(2, self.client.account.event_count)

    def test_can_submit_market_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.client.register_strategy(strategy)
        strategy.start()

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderId('123456'),
            Label('S1_E'),
            OrderSide.BUY,
            100000)

        # Act
        strategy.submit_order(order, PositionId(str(order.id)))

        # Assert
        self.assertEqual(4, strategy.object_storer.count)
        self.assertEqual(Decimal('86.711'), order.average_price)


# -- ENGINE -------------------------------------------------------------------------------------- #
class BacktestEngineTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        self.instruments = [TestStubs.instrument_usdjpy()]
        self.tick_data = {usdjpy.symbol: pd.DataFrame()}
        self.bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        self.ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        self.strategies = [TestStrategy1(TestStubs.bartype_usdjpy_1min_bid())]

        self.engine = BacktestEngine(instruments=self.instruments,
                                     tick_data=self.tick_data,
                                     bar_data_bid=self.bid_data,
                                     bar_data_ask=self.ask_data,
                                     strategies=self.strategies)

    def test_can_initialize_engine_with_data(self):
        # Arrange
        # Act
        # Assert
        # Does not throw exception
        self.assertEqual(all(self.bid_data), all(self.engine.data_client.bar_data_bid))
        self.assertEqual(all(self.ask_data), all(self.engine.data_client.bar_data_bid))

    def test_can_run(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EMACross(label='EMACross_Test',
                               order_id_tag='01',
                               instrument=usdjpy,
                               bar_type=TestStubs.bartype_usdjpy_1min_bid(),
                               position_size=100000,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        config = BacktestConfig(console_prints=True)
        engine = BacktestEngine(instruments=instruments,
                                tick_data=tick_data,
                                bar_data_bid=bid_data,
                                bar_data_ask=ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(1440, engine.data_client.data_providers[usdjpy.symbol].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1440, strategies[0].fast_ema.count)
