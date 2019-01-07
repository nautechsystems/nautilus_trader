#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_strategy.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid
import datetime
import time

from datetime import datetime, timezone, timedelta

from inv_trader.core.decimal import Decimal
from inv_trader.common.logger import LoggerAdapter
from inv_trader.model.enums import Venue, Resolution, QuoteType, OrderSide, TimeInForce, OrderStatus
from inv_trader.model.enums import MarketPosition
from inv_trader.model.objects import Price, Symbol, Tick, BarType, Bar
from inv_trader.model.order import OrderFactory
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import TimeEvent
from inv_trader.model.identifiers import GUID, Label, OrderId, PositionId
from inv_trader.model.position import Position
from inv_trader.data import LiveDataClient
from inv_trader.strategy import TradeStrategy
from inv_trader.tools import IndicatorUpdater
from inv_indicators.average.ema import ExponentialMovingAverage
from inv_indicators.intrinsic_network import IntrinsicNetwork
from test_kit.stubs import TestStubs
from test_kit.mocks import MockExecClient
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('audusd', Venue.FXCM)
GBPUSD_FXCM = Symbol('gbpusd', Venue.FXCM)


class TradeStrategyTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_factory = OrderFactory()
        print('\n')

    def test_strategy_equality(self):
        # Arrange
        strategy1 = TradeStrategy()
        strategy2 = TradeStrategy('AUDUSD-001')
        strategy3 = TradeStrategy('AUDUSD-002')

        # Act
        result1 = strategy1 == strategy1
        result2 = strategy1 == strategy2
        result3 = strategy2 == strategy3
        result4 = strategy1 != strategy1
        result5 = strategy1 != strategy2
        result6 = strategy2 != strategy3

        # Assert
        self.assertTrue(result1)
        self.assertFalse(result2)
        self.assertFalse(result3)
        self.assertFalse(result4)
        self.assertTrue(result5)
        self.assertTrue(result6)

    def test_strategy_is_hashable(self):
        # Arrange
        strategy = TradeStrategy('Test')

        # Act
        result = strategy.__hash__()

        # Assert
        # If this passes then result must be an int.
        self.assertTrue(result != 0)

    def test_strategy_str_and_repr(self):
        # Arrange
        strategy = TradeStrategy('GBPUSD-MM')

        # Act
        result1 = str(strategy)
        result2 = repr(strategy)

        # Assert
        self.assertEqual('TradeStrategy-GBPUSD-MM', result1)
        self.assertTrue(result2.startswith('<TradeStrategy-GBPUSD-MM object at'))
        self.assertTrue(result2.endswith('>'))

    def test_can_get_strategy_name(self):
        # Arrange
        strategy = TradeStrategy()

        # Act
        result = strategy.name

        # Assert
        self.assertEqual('TradeStrategy', result)

    def test_can_get_strategy_label(self):
        # Arrange
        strategy1 = TradeStrategy()
        strategy2 = TradeStrategy('EURUSD-Scalper')

        # Act
        result1 = strategy1.label
        result2 = strategy2.label

        # Assert
        self.assertEqual('0', result1)
        self.assertEqual('EURUSD-Scalper', result2)
        self.assertEqual('TradeStrategy-0', str(strategy1))
        self.assertEqual('TradeStrategy-EURUSD-Scalper', str(strategy2))

    def test_can_get_strategy_id(self):
        # Arrange
        strategy = TradeStrategy()

        # Act
        result = strategy.id

        # Assert
        self.assertTrue(isinstance(result, GUID))
        print(result)

    def test_can_get_logger(self):
        strategy = TestStrategy1()

        # Act
        result = strategy.log

        # Assert
        self.assertTrue(isinstance(result, LoggerAdapter))
        print(result)

    def test_can_get_indicators(self):
        strategy = TestStrategy1()

        # Act
        result = strategy.indicators(strategy.gbpusd_1sec_mid)

        # Assert
        self.assertTrue(2, len(result))
        print(result)

    def test_getting_indicators_for_unknown_bar_type_raises_exception(self):
        strategy = TestStrategy1()

        unknown_bar_type = BarType(
            AUDUSD_FXCM,
            5,
            Resolution.MINUTE,
            QuoteType.BID)

        # Act
        # Assert
        self.assertRaises(KeyError, strategy.indicators, unknown_bar_type)

    def test_getting_indicator_for_unknown_label_raises_exception(self):
        strategy = TestStrategy1()

        # Act
        # Assert
        self.assertRaises(KeyError, strategy.indicator, 'unknown_bar_type')

    def test_getting_bars_for_unknown_bar_type_raises_exception(self):
        strategy = TestStrategy1()

        unknown_bar_type = BarType(
            AUDUSD_FXCM,
            5,
            Resolution.MINUTE,
            QuoteType.BID)

        # Act
        # Assert
        self.assertRaises(KeyError, strategy.bars, unknown_bar_type)

    def test_can_get_bars(self):
        strategy = TestStrategy1()

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00002'),
            Decimal('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy._update_bars(bar_type, bar)

        # Act
        result = strategy.bars(bar_type)

        # Assert
        self.assertTrue(bar, result[0])

    def test_getting_bar_for_unknown_bar_type_raises_exception(self):
        strategy = TestStrategy1()

        unknown_bar_type = BarType(
            AUDUSD_FXCM,
            5,
            Resolution.MINUTE,
            QuoteType.BID)

        # Act
        # Assert
        self.assertRaises(KeyError, strategy.bar, unknown_bar_type, 0)

    def test_getting_bar_at_out_of_range_index_raises_exception(self):
        strategy = TestStrategy1()

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00002'),
            Decimal('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy._update_bars(bar_type, bar)

        # Act
        # Assert
        self.assertRaises(IndexError, strategy.bar, bar_type, -2)

    def test_can_get_bar(self):
        strategy = TestStrategy1()

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00002'),
            Decimal('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy._update_bars(bar_type, bar)

        # Act
        result = strategy.bar(bar_type, 0)

        # Assert
        self.assertEqual(bar, result)

    def test_getting_last_tick_with_unknown_symbol_raises_exception(self):
        strategy = TestStrategy1()

        # Act
        # Assert
        self.assertRaises(KeyError, strategy.last_tick, AUDUSD_FXCM)

    def test_can_get_last_tick(self):
        strategy = TestStrategy1()

        tick = Tick(Symbol('AUDUSD', Venue.FXCM),
                    Decimal('1.00000'),
                    Decimal('1.00001'),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        strategy._update_ticks(tick)

        # Act
        result = strategy.last_tick(AUDUSD_FXCM)

        # Assert
        self.assertEqual(tick, result)

    def test_getting_order_with_unknown_id_raises_exception(self):
        # Arrange
        strategy = TestStrategy1()

        # Act
        # Assert
        self.assertRaises(KeyError, strategy.order, OrderId('unknown_order_id'))

    def test_can_get_order(self):
        # Arrange
        strategy = TestStrategy1()

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        strategy._order_book[order.id] = order

        # Act
        result = strategy.order(order.id)

        # Assert
        self.assertEqual(order, result)

    def test_getting_position_with_unknown_id_raises_exception(self):
        # Arrange
        strategy = TestStrategy1()

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.position, PositionId('unknown_position_id'))

    def test_can_get_position(self):
        # Arrange
        strategy = TestStrategy1()

        position = Position(
            AUDUSD_FXCM,
            PositionId('AUDUSD-123456-1'),
            TestStubs.unix_epoch())

        strategy._position_book[position.id] = position

        # Act
        result = strategy.position(position.id)

        # Assert
        self.assertEqual(position, result)

    def test_can_start_strategy(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        result1 = strategy.is_running
        # Act
        strategy.start()
        result2 = strategy.is_running

        # Assert
        self.assertFalse(result1)
        self.assertTrue(result2)
        self.assertTrue('custom start logic' in strategy.object_storer.get_store)

    def test_can_stop_strategy(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        # Act
        strategy.stop()

        # Assert
        self.assertFalse(strategy.is_running)
        self.assertTrue('custom stop logic' in strategy.object_storer.get_store)

    def test_can_reset_strategy(self):
        # Arrange
        strategy = TestStrategy1()

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00002'),
            Decimal('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy._update_bars(bar_type, bar)

        # Act
        strategy.reset()

        # Assert
        self.assertFalse(strategy.is_running)
        self.assertEqual(0, strategy.ema1.count)
        self.assertEqual(0, strategy.ema2.count)
        self.assertTrue('custom reset logic' in strategy.object_storer.get_store)

    def test_can_register_indicator_with_strategy(self):
        # Arrange
        strategy = TestStrategy1()

        # Act
        result1 = strategy.all_indicators[strategy.gbpusd_1sec_mid][0]
        result2 = strategy.all_indicators[strategy.gbpusd_1sec_mid][1]

        # Assert
        self.assertEqual(strategy.ema1, result1)
        self.assertEqual(strategy.ema2, result2)

    def test_can_set_time_alert(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        alert_time = datetime.now(timezone.utc) + timedelta(milliseconds=300)
        strategy.set_time_alert(Label("test_alert1"), alert_time)

        # Act
        strategy.start()
        time.sleep(0.8)
        strategy.stop()

        # Assert
        self.assertEqual(3, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store[1], TimeEvent))

    def test_can_cancel_time_alert(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        alert_time = datetime.now(timezone.utc) + timedelta(seconds=1)
        strategy.set_time_alert(Label("test_alert1"), alert_time)

        # Act
        strategy.start()
        time.sleep(0.5)
        strategy.cancel_time_alert(Label("test_alert1"))
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_stopping_a_strategy_cancels_a_running_time_alert(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        alert_time = datetime.now(timezone.utc) + timedelta(milliseconds=200)
        strategy.set_time_alert(Label("test_alert1"), alert_time)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_can_set_multiple_time_alerts(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        alert_time1 = datetime.now(timezone.utc) + timedelta(milliseconds=200)
        alert_time2 = datetime.now(timezone.utc) + timedelta(milliseconds=300)

        # Act
        strategy.set_time_alert(Label("test_alert1"), alert_time1)
        strategy.set_time_alert(Label("test_alert2"), alert_time2)
        strategy.start()
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertTrue(isinstance(strategy.object_storer.get_store[1], TimeEvent))
        self.assertTrue(isinstance(strategy.object_storer.get_store[2], TimeEvent))

    def test_can_set_timer(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer1"), timedelta(milliseconds=100), start_time, None, None)

        # Act
        strategy.start()
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertTrue(isinstance(strategy.object_storer.get_store[1], TimeEvent))

    def test_can_cancel_timer(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer1"), timedelta(milliseconds=100), start_time, None, None)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.cancel_timer(Label("test_timer1"))
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_stopping_a_strategy_cancels_a_running_timer(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer1"), timedelta(milliseconds=100), start_time, None, None)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_can_set_repeating_timer(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer1"), timedelta(milliseconds=100), start_time, None, repeat=True)

        # Act
        strategy.start()
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertTrue(isinstance(strategy.object_storer.get_store[1], TimeEvent))
        self.assertTrue(isinstance(strategy.object_storer.get_store[2], TimeEvent))
        self.assertTrue(isinstance(strategy.object_storer.get_store[3], TimeEvent))

    def test_can_cancel_repeating_timer(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        stop_time = start_time + timedelta(seconds=1)
        strategy.set_timer(Label("test_timer1"), timedelta(milliseconds=100), start_time, stop_time, repeat=True)

        # Act
        strategy.start()
        time.sleep(0.55)
        strategy.cancel_timer(Label("test_timer1"))
        strategy.stop()

        # Assert
        self.assertEqual(6, strategy.object_storer.count)

    def test_can_set_two_repeating_timers(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer1"), timedelta(milliseconds=100), start_time, None, True)
        strategy.set_timer(Label("test_timer2"), timedelta(milliseconds=100), start_time, None, True)

        # Act
        strategy.start()
        time.sleep(0.55)
        strategy.stop()

        # Assert
        self.assertEqual(10, strategy.object_storer.count)

    def test_can_generate_order_id(self):
        # Arrange
        strategy = TestStrategy1()

        # Act
        result = strategy.generate_order_id(AUDUSD_FXCM)

        # Assert
        self.assertTrue(result.value.startswith('AUDUSD-FXCM-1-TS01-'))

    def test_get_opposite_side_returns_expected_sides(self):
        # Arrange
        strategy = TestStrategy1()

        # Act
        result1 = strategy.get_opposite_side(OrderSide.BUY)
        result2 = strategy.get_opposite_side(OrderSide.SELL)

        # Assert
        self.assertEqual(OrderSide.SELL, result1)
        self.assertEqual(OrderSide.BUY, result2)

    def test_get_flatten_side_with_flat_market_position_raises_exception(self):
        # Arrange
        strategy = TestStrategy1()

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.get_flatten_side, MarketPosition.FLAT)

    def test_get_flatten_side_with_long_or_short_market_position_returns_expected_sides(self):
        # Arrange
        strategy = TestStrategy1()

        # Act
        result1 = strategy.get_flatten_side(MarketPosition.LONG)
        result2 = strategy.get_flatten_side(MarketPosition.SHORT)

        # Assert
        self.assertEqual(OrderSide.SELL, result1)
        self.assertEqual(OrderSide.BUY, result2)

    def test_submitting_order_with_identical_id_raises_ex(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        position_id = PositionId(str(order.id))
        strategy.submit_order(order, position_id)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.submit_order, order, position_id)

    def test_strategy_can_submit_order(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        # Act
        strategy.submit_order(order, PositionId(str(order.id)))

        # Assert
        self.assertEqual(order, strategy.orders[order.id])
        self.assertEqual(OrderStatus.WORKING, strategy.orders[order.id].status)
        self.assertTrue(order.id in strategy.active_orders)
        self.assertTrue(order.id not in strategy.completed_orders)

    def test_cancelling_order_which_does_not_exist_raises_ex(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.cancel_order, order, 'NONE')

    def test_can_cancel_order(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        strategy.submit_order(order, PositionId(str(order.id)))

        # Act
        strategy.cancel_order(order, 'NONE')

        # Assert
        self.assertEqual(order, strategy.orders[order.id])
        self.assertEqual(OrderStatus.CANCELLED, strategy.orders[order.id].status)
        self.assertTrue(order.id in strategy.completed_orders)
        self.assertTrue(order.id not in strategy.active_orders)

    def test_modifying_order_which_does_not_exist_raises_ex(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.modify_order, order, Price.create(1.00001, 5))

    def test_can_modify_order(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000,
            Price.create(1.00000, 5),
            TimeInForce.DAY,
            None)

        strategy.submit_order(order, PositionId(str(order.id)))

        # Act
        strategy.modify_order(order, Price.create(1.00001, 5))

        # Assert
        self.assertEqual(order, strategy.orders[order.id])
        self.assertEqual(OrderStatus.WORKING, strategy.orders[order.id].status)
        self.assertEqual(Price.create(1.00001, 5), strategy.orders[order.id].price)
        self.assertTrue(strategy.is_flat)

    def test_can_cancel_all_orders(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order1 = self.order_factory.stop(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000,
            Price.create(1.00000, 5),
            TimeInForce.DAY,
            None)

        order2 = self.order_factory.stop(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-2'),
            Label('S1'),
            OrderSide.BUY,
            100000,
            Price.create(1.00010, 5),
            TimeInForce.DAY,
            None)

        strategy.submit_order(order1, PositionId('some-position'))
        strategy.submit_order(order2, PositionId('some-position'))

        # Act
        strategy.cancel_all_orders('TEST')

        # Assert
        self.assertEqual(order1, strategy.orders[order1.id])
        self.assertEqual(order2, strategy.orders[order2.id])
        self.assertEqual(OrderStatus.CANCELLED, strategy.orders[order1.id].status)
        self.assertEqual(OrderStatus.CANCELLED, strategy.orders[order2.id].status)
        self.assertTrue(order1.id in strategy.completed_orders)
        self.assertTrue(order2.id in strategy.completed_orders)

    def test_can_flatten_position(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        strategy.submit_order(order, PositionId('some-position'))
        exec_client.fill_last_order()

        # Act
        strategy.flatten_position(PositionId('some-position'))
        exec_client.fill_last_order()

        # Assert
        self.assertEqual(order, strategy.orders[order.id])
        self.assertEqual(OrderStatus.FILLED, strategy.orders[order.id].status)
        self.assertEqual(MarketPosition.FLAT, strategy.position(PositionId('some-position')).market_position)
        self.assertTrue(strategy.position(PositionId('some-position')).is_exited)
        self.assertTrue(PositionId('some-position') in strategy.completed_positions)
        self.assertTrue(strategy.is_flat)

    def test_flatten_position_which_does_not_exist_raises_exception(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.flatten_position, PositionId('some-position'))

    def test_can_flatten_all_positions(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-2'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        strategy.submit_order(order1, PositionId('some-position1'))
        strategy.submit_order(order2, PositionId('some-position2'))
        exec_client.fill_last_order()
        exec_client.fill_last_order()

        # Act
        strategy.flatten_all_positions()
        exec_client.fill_last_order()
        exec_client.fill_last_order()

        # Assert
        self.assertEqual(order1, strategy.orders[order1.id])
        self.assertEqual(order2, strategy.orders[order2.id])
        self.assertEqual(OrderStatus.FILLED, strategy.orders[order1.id].status)
        self.assertEqual(OrderStatus.FILLED, strategy.orders[order2.id].status)
        self.assertEqual(MarketPosition.FLAT, strategy.position(PositionId('some-position1')).market_position)
        self.assertEqual(MarketPosition.FLAT, strategy.position(PositionId('some-position2')).market_position)
        self.assertTrue(strategy.position(PositionId('some-position1')).is_exited)
        self.assertTrue(strategy.position(PositionId('some-position2')).is_exited)
        self.assertTrue(PositionId('some-position1') in strategy.completed_positions)
        self.assertTrue(PositionId('some-position2') in strategy.completed_positions)
        self.assertTrue(strategy.is_flat)

    # def test_registering_execution_client_with_none_raises_exception(self):
    #     # Arrange
    #     strategy = TestStrategy1()
    #
    #     # Act
    #     # Assert
    #     self.assertRaises(ValueError, strategy._register_execution_client, None)

    def test_registering_execution_client_of_wrong_type_raises_exception(self):
        # Arrange
        strategy = TestStrategy1()

        # Act
        # Assert
        self.assertRaises(TypeError, strategy._register_execution_client, LiveDataClient())

    def test_can_update_bars_and_indicators(self):
        # Arrange
        strategy = TestStrategy1()

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00002'),
            Decimal('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        # Act
        strategy._update_bars(bar_type, bar)

        # Assert
        self.assertEqual(1, len(strategy.all_bars[bar_type]))
        self.assertEqual(1, len(strategy.bars(bar_type)))
        self.assertEqual(1, strategy.ema1.count)
        self.assertEqual(1, strategy.ema2.count)
        self.assertEqual(0, len(strategy.object_storer.get_store))

    def test_can_update_order_events(self):
        # Arrange
        strategy = TestStrategy1()
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        event = OrderSubmitted(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        strategy._order_book[order.id] = order

        # Act
        strategy._update_events(event)

        # Assert
        self.assertEqual(OrderStatus.SUBMITTED, strategy.orders[order.id].status)

    def test_can_track_orders_for_an_opened_position(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        strategy.submit_order(order, PositionId('AUDUSD-123456-1'))
        exec_client.fill_last_order()

        # Act
        # Assert
        self.assertTrue(OrderId('AUDUSD-123456-1') in strategy._order_position_index)
        self.assertTrue(PositionId('AUDUSD-123456-1') in strategy._position_book)
        self.assertEqual(0, len(strategy.active_orders))
        self.assertEqual(order, strategy.completed_orders[order.id])
        self.assertEqual(0, len(strategy.completed_positions))
        self.assertTrue(OrderId('AUDUSD-123456-1') in strategy.completed_orders)
        self.assertTrue(PositionId('AUDUSD-123456-1') in strategy.active_positions)
        self.assertFalse(strategy.is_flat)

    def test_can_track_orders_for_a_closing_position(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        position1 = PositionId('position1')
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-2'),
            Label('S1'),
            OrderSide.SELL,
            100000)

        strategy.submit_order(order1, position1)
        exec_client.fill_last_order()
        strategy.submit_order(order2, position1)
        exec_client.fill_last_order()

        # Act
        # Assert
        self.assertEqual(position1, strategy._order_position_index[order1.id])
        self.assertEqual(position1, strategy._order_position_index[order2.id])
        self.assertEqual(0, len(strategy.active_orders))
        self.assertEqual(order1, strategy.completed_orders[order1.id])
        self.assertEqual(order2, strategy.completed_orders[order2.id])
        self.assertEqual(1, len(strategy.completed_positions))
        self.assertFalse(PositionId('position1') in strategy.active_positions)
        self.assertTrue(PositionId('position1') in strategy.completed_positions)
        self.assertTrue(strategy.is_flat)
