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
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderRejected, OrderWorking, OrderModified, OrderFilled
from inv_trader.backtest.data import BacktestDataClient
from inv_trader.backtest.execution import BacktestExecClient
from inv_trader.backtest.engine import BacktestConfig, BacktestEngine
from test_kit.objects import ObjectStorer
from test_kit.strategies import EmptyStrategy, TestStrategy1, EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = Symbol('USDJPY', Venue.FXCM)


# -- DATA ---------------------------------------------------------------------------------------- #
class BacktestDataClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = TestStubs.instrument_usdjpy()
        self.bid_data_1min = TestDataProvider.usdjpy_1min_bid().iloc[:2000]
        self.ask_data_1min = TestDataProvider.usdjpy_1min_ask().iloc[:2000]

        self.instruments = [TestStubs.instrument_usdjpy()]
        self.data_ticks = {self.usdjpy.symbol: pd.DataFrame()}
        self.data_bars_bid = {self.usdjpy.symbol: {Resolution.MINUTE: self.bid_data_1min}}
        self.data_bars_ask = {self.usdjpy.symbol: {Resolution.MINUTE: self.ask_data_1min}}

        self.client = BacktestDataClient(
            instruments=self.instruments,
            dataframes_ticks=self.data_ticks,
            dataframes_bars_bid=self.data_bars_bid,
            dataframes_bars_ask=self.data_bars_ask,
            clock=TestClock(),
            logger=Logger())

    def test_can_initialize_client_with_data(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(all(self.bid_data_1min), all(self.client.dataframes_bars_bid[self.usdjpy.symbol][Resolution.MINUTE]))
        self.assertEqual(all(self.ask_data_1min), all(self.client.dataframes_bars_bid[self.usdjpy.symbol][Resolution.MINUTE]))
        self.assertEqual(all(self.bid_data_1min.index), all(self.client.data_minute_index))

    def test_can_get_one_minute_bid_bars(self):
        # Arrange
        # Act
        bars = self.client.get_minute_bid_bars()

        # Assert
        self.assertEqual(2000, len(bars[self.usdjpy.symbol]))

    def test_can_get_one_minute_ask_bars(self):
        # Arrange
        # Act
        bars = self.client.get_minute_ask_bars()

        # Assert
        self.assertEqual(2000, len(bars[self.usdjpy.symbol]))

    def test_can_set_initial_iteration(self):
        # Arrange
        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        dummy = []

        # Act
        self.client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), dummy.append)
        self.client.set_initial_iteration(start, timedelta(minutes=1))

        # Assert
        self.assertEqual(1440, self.client.iteration)
        self.assertEqual(start, self.client.time_now())
        self.assertEqual(1440, self.client.data_providers[self.usdjpy.symbol].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(start, self.client.data_providers[self.usdjpy.symbol].bars[TestStubs.bartype_usdjpy_1min_bid()][1440].timestamp)

    def test_can_iterate_bar_data(self):
        # Arrange
        receiver = ObjectStorer()
        self.client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), receiver.store_2)

        start_datetime = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        for x in range(1000):
            self.client.iterate(start_datetime + timedelta(minutes=x))

        # Assert
        self.assertEqual(1000, len(receiver.get_store()))
        self.assertTrue(self.client.data_minute_index[0] == self.client.data_providers[self.usdjpy.symbol].bars[TestStubs.bartype_usdjpy_1min_bid()][0].timestamp)


# -- EXECUTION ----------------------------------------------------------------------------------- #
class BacktestExecClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = TestStubs.instrument_usdjpy()
        self.bid_data_1min = TestDataProvider.usdjpy_1min_bid()[:2000]
        self.ask_data_1min = TestDataProvider.usdjpy_1min_ask()[:2000]

        self.instruments = [TestStubs.instrument_usdjpy()]
        self.data_ticks = {self.usdjpy.symbol: pd.DataFrame()}
        self.data_bars_bid = {self.usdjpy.symbol: {Resolution.MINUTE: self.bid_data_1min}}
        self.data_bars_ask = {self.usdjpy.symbol: {Resolution.MINUTE: self.ask_data_1min}}

        self.strategies = [TestStrategy1(TestStubs.bartype_usdjpy_1min_bid())]

        self.data_client = BacktestDataClient(
            instruments=self.instruments,
            dataframes_ticks=self.data_ticks,
            dataframes_bars_bid=self.data_bars_bid,
            dataframes_bars_ask=self.data_bars_ask,
            clock=TestClock(),
            logger=Logger())

        self.client = BacktestExecClient(instruments=self.instruments,
                                         data_ticks=self.data_ticks,
                                         data_bars_bid=self.data_client.get_minute_bid_bars(),
                                         data_bars_ask=self.data_client.get_minute_ask_bars(),
                                         data_minute_index=self.data_client.data_minute_index,
                                         starting_capital=1000000,
                                         slippage_ticks=1,
                                         clock=TestClock(),
                                         logger=Logger())

    def test_can_initialize_client_with_data(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(all(self.data_bars_bid), all(self.client.data_bars_bid[self.usdjpy.symbol]))
        self.assertEqual(all(self.data_bars_ask), all(self.client.data_bars_ask[self.usdjpy.symbol]))
        self.assertEqual(all(self.bid_data_1min.index), all(self.client.data_minute_index))
        self.assertEqual(Decimal(1000000), self.client.account.cash_balance)
        self.assertEqual(Decimal(1000000), self.client.account.free_equity)
        self.assertEqual(Decimal('0.001'), self.client.slippage_index[self.usdjpy.symbol])

    def test_can_set_initial_iteration(self):
        # Arrange
        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        self.client.set_initial_iteration(start, timedelta(minutes=1))

        # Assert
        self.assertEqual(1440, self.client.iteration)
        self.assertEqual(start, self.client.time_now())

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
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderFilled))
        self.assertEqual(Decimal('86.710'), order.average_price)

    def test_can_submit_limit_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.client.register_strategy(strategy)
        strategy.start()

        order = strategy.order_factory.limit(
            USDJPY_FXCM,
            OrderId('123456'),
            Label('S1_E'),
            OrderSide.BUY,
            100000,
            Decimal('80.000'))

        # Act
        strategy.submit_order(order, PositionId(str(order.id)))

        # Assert
        print(strategy.object_storer.get_store())
        self.assertEqual(4, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderWorking))
        self.assertEqual(Decimal('80.000'), order.price)

    def test_can_modify_stop_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.client.register_strategy(strategy)
        strategy.start()

        order = strategy.order_factory.stop_market(
            USDJPY_FXCM,
            OrderId('123456'),
            Label('S1_E'),
            OrderSide.BUY,
            100000,
            Decimal('86.711'))

        strategy.submit_order(order, PositionId(str(order.id)))

        # Act
        strategy.modify_order(order, Decimal('86.712'))

        # Assert
        self.assertEqual(Decimal('86.712'), order.price)
        self.assertEqual(5, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[4], OrderModified))

    def test_order_with_invalid_price_gets_rejected(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.client.register_strategy(strategy)
        strategy.start()

        order = strategy.order_factory.stop_market(
            USDJPY_FXCM,
            OrderId('123456'),
            Label('S1_E'),
            OrderSide.BUY,
            100000,
            Decimal('80.000'))

        # Act
        strategy.submit_order(order, PositionId(str(order.id)))

        # Assert
        self.assertEqual(4, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderRejected))


# -- ENGINE -------------------------------------------------------------------------------------- #
class BacktestEngineTests(unittest.TestCase):

    def test_can_run_empty_strategy(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EmptyStrategy()]

        config = BacktestConfig(console_prints=True)
        engine = BacktestEngine(instruments=instruments,
                                data_ticks=tick_data,
                                data_bars_bid=bid_data,
                                data_bars_ask=ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(44640, engine.data_client.iteration)
        self.assertEqual(44640, engine.exec_client.iteration)

    def test_can_run(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EMACross(label='001',
                               order_id_tag='01',
                               instrument=usdjpy,
                               bar_type=TestStubs.bartype_usdjpy_1min_bid(),
                               position_size=100000,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        config = BacktestConfig(bypass_logging=False,
                                console_prints=True)
        engine = BacktestEngine(instruments=instruments,
                                data_ticks=tick_data,
                                data_bars_bid=bid_data,
                                data_bars_ask=ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(2880, engine.data_client.data_providers[usdjpy.symbol].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1440, strategies[0].fast_ema.count)
