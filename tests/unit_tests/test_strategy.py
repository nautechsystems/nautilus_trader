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

from inv_trader.common.clock import TestClock
from inv_trader.model.enums import Venue, Resolution, QuoteType, OrderSide, TimeInForce, OrderStatus
from inv_trader.model.enums import MarketPosition
from inv_trader.model.objects import Symbol, Price, Tick, BarType, Bar
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

    def test_can_get_current_time(self):
        # Arrange
        strategy = TradeStrategy()

        # Act
        result = strategy.time_now()

        # Assert
        self.assertEqual(timezone.utc, result.tzinfo)

    def test_can_get_indicators(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        # Act
        result = strategy.indicators(strategy.bar_type)

        # Assert
        self.assertTrue(2, len(result))
        print(result)

    def test_can_call_all_indicators_initialized(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        # Act
        result = strategy.all_indicators_initialized()

        # Assert
        self.assertFalse(result)

    def test_getting_indicators_for_unknown_bar_type_raises_exception(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        unknown_bar_type = BarType(
            AUDUSD_FXCM,
            5,
            Resolution.MINUTE,
            QuoteType.BID)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.indicators, unknown_bar_type)

    def test_getting_bars_for_unknown_bar_type_raises_exception(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        unknown_bar_type = BarType(
            AUDUSD_FXCM,
            5,
            Resolution.MINUTE,
            QuoteType.BID)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.bars, unknown_bar_type)

    def test_can_get_bars(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Price('1.00001'),
            Price('1.00004'),
            Price('1.00002'),
            Price('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy._update_bars(bar_type, bar)

        # Act
        result = strategy.bars(bar_type)

        # Assert
        self.assertTrue(bar, result[0])

    def test_getting_bar_for_unknown_bar_type_raises_exception(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        unknown_bar_type = BarType(
            AUDUSD_FXCM,
            5,
            Resolution.MINUTE,
            QuoteType.BID)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.bar, unknown_bar_type, 0)

    def test_getting_bar_at_out_of_range_index_raises_exception(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Price('1.00001'),
            Price('1.00004'),
            Price('1.00002'),
            Price('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy._update_bars(bar_type, bar)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.bar, bar_type, -2)

    def test_can_get_bar(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Price('1.00001'),
            Price('1.00004'),
            Price('1.00002'),
            Price('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy._update_bars(bar_type, bar)

        # Act
        result = strategy.bar(bar_type, 0)

        # Assert
        self.assertEqual(bar, result)

    def test_can_get_last_bar(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Price('1.00001'),
            Price('1.00004'),
            Price('1.00002'),
            Price('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy._update_bars(bar_type, bar)

        # Act
        result = strategy.last_bar(bar_type)

        # Assert
        self.assertEqual(bar, result)

    def test_getting_last_tick_with_unknown_symbol_raises_exception(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.last_tick, AUDUSD_FXCM)

    def test_can_get_last_tick(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        tick = Tick(Symbol('AUDUSD', Venue.FXCM),
                    Price('1.00000'),
                    Price('1.00001'),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        strategy._update_ticks(tick)

        # Act
        result = strategy.last_tick(AUDUSD_FXCM)

        # Assert
        self.assertEqual(tick, result)

    # def test_getting_order_with_unknown_id_raises_exception(self):
    #     # Arrange
    #     bar_type = TestStubs.bartype_gbpusd_1sec_mid()
    #     strategy = TestStrategy1(bar_type)
    #
    #     # Act
    #     # Assert
    #     self.assertRaises(ValueError, strategy.order, OrderId('unknown_order_id'))

    def test_can_get_order(self):
        # Arrange
        strategy = TradeStrategy()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        strategy.submit_order(order, PositionId('some-position'))

        # Act
        result = strategy.order(order.id)

        # Assert
        self.assertEqual(order, result)

    # def test_getting_position_with_unknown_id_raises_exception(self):
    #     # Arrange
    #     bar_type = TestStubs.bartype_audusd_1min_bid()
    #     strategy = TestStrategy1(bar_type)
    #
    #     # Act
    #     # Assert
    #     self.assertRaises(ValueError, strategy.position, PositionId('unknown_position_id'))

    def test_can_get_position(self):
        # Arrange
        strategy = TradeStrategy()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-1-123456'),
            Label('S1'),
            OrderSide.BUY,
            100000)

        position_id = PositionId('AUDUSD-1-123456')

        strategy.submit_order(order, position_id)
        exec_client.fill_last_order()

        # Act
        result = strategy.position(position_id)

        # Assert
        self.assertTrue(type(result) == Position)

    def test_can_start_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        result1 = strategy.is_running
        # Act
        strategy.start()
        result2 = strategy.is_running

        # Assert
        self.assertFalse(result1)
        self.assertTrue(result2)
        self.assertTrue('custom start logic' in strategy.object_storer.get_store())

    def test_can_stop_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        # Act
        strategy.stop()

        # Assert
        self.assertFalse(strategy.is_running)
        self.assertTrue('custom stop logic' in strategy.object_storer.get_store())

    def test_can_reset_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Price('1.00001'),
            Price('1.00004'),
            Price('1.00002'),
            Price('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy._update_bars(bar_type, bar)

        # Act
        strategy.reset()

        # Assert
        self.assertFalse(strategy.is_running)
        self.assertEqual(0, strategy.ema1.count)
        self.assertEqual(0, strategy.ema2.count)
        self.assertTrue('custom reset logic' in strategy.object_storer.get_store())

    def test_can_register_indicator_with_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)

        # Act
        result = strategy.indicators(bar_type)

        # Assert
        self.assertEqual([strategy.ema1, strategy.ema2], result)

    def test_can_register_strategy_with_exec_client(self):
        # Arrange
        exec_client = MockExecClient()
        strategy = TradeStrategy()

        # Act
        exec_client.register_strategy(strategy)


    def test_can_set_time_alert(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
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
        self.assertTrue(isinstance(strategy.object_storer.get_store()[1], TimeEvent))

    def test_can_cancel_time_alert(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
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
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
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
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
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
        self.assertTrue(isinstance(strategy.object_storer.get_store()[1], TimeEvent))
        self.assertTrue(isinstance(strategy.object_storer.get_store()[2], TimeEvent))

    def test_can_set_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer1"), timedelta(milliseconds=100), start_time, None, None)

        # Act
        strategy.start()
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertTrue(isinstance(strategy.object_storer.get_store()[1], TimeEvent))

    def test_can_cancel_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer2"), timedelta(milliseconds=100), start_time, None, None)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.cancel_timer(Label("test_timer2"))
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_stopping_a_strategy_cancels_a_running_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer3"), timedelta(milliseconds=100), start_time, None, None)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_can_set_repeating_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer4"), timedelta(milliseconds=100), start_time, None, repeat=True)

        # Act
        strategy.start()
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertTrue(isinstance(strategy.object_storer.get_store()[1], TimeEvent))
        self.assertTrue(isinstance(strategy.object_storer.get_store()[2], TimeEvent))
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], TimeEvent))

    def test_can_cancel_repeating_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        stop_time = start_time + timedelta(seconds=1)
        strategy.set_timer(Label("test_timer5"), timedelta(milliseconds=100), start_time, stop_time, repeat=True)

        # Act
        strategy.start()
        time.sleep(0.55)
        strategy.cancel_timer(Label("test_timer5"))
        strategy.stop()

        # Assert
        self.assertEqual(6, strategy.object_storer.count)

    def test_can_set_two_repeating_timers(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.set_timer(Label("test_timer6"), timedelta(milliseconds=100), start_time, None, True)
        strategy.set_timer(Label("test_timer7"), timedelta(milliseconds=100), start_time, None, True)

        # Act
        strategy.start()
        time.sleep(0.55)
        strategy.stop()

        # Assert
        self.assertEqual(10, strategy.object_storer.count)

    def test_can_generate_order_id(self):
        # Arrange
        strategy = TradeStrategy(clock=TestClock())

        # Act
        result = strategy.generate_order_id(AUDUSD_FXCM)

        # Assert
        self.assertEqual(OrderId('19700101-000000-001-001-AUDUSD-FXCM-1'), result)

    def test_get_opposite_side_returns_expected_sides(self):
        # Arrange
        strategy = TradeStrategy()

        # Act
        result1 = strategy.get_opposite_side(OrderSide.BUY)
        result2 = strategy.get_opposite_side(OrderSide.SELL)

        # Assert
        self.assertEqual(OrderSide.SELL, result1)
        self.assertEqual(OrderSide.BUY, result2)

    def test_get_flatten_side_with_flat_market_position_raises_exception(self):
        # Arrange
        strategy = TradeStrategy()

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.get_flatten_side, MarketPosition.FLAT)

    def test_get_flatten_side_with_long_or_short_market_position_returns_expected_sides(self):
        # Arrange
        strategy = TradeStrategy()

        # Act
        result1 = strategy.get_flatten_side(MarketPosition.LONG)
        result2 = strategy.get_flatten_side(MarketPosition.SHORT)

        # Assert
        self.assertEqual(OrderSide.SELL, result1)
        self.assertEqual(OrderSide.BUY, result2)

    def test_can_change_clock(self):
        # Arrange
        clock = TestClock()
        strategy = TradeStrategy()

        # Act
        strategy.change_clock(clock)

        # Assert
        self.assertEqual(clock.unix_epoch(), strategy.time_now())
        self.assertEqual(OrderId('19700101-000000-001-001-AUDUSD-FXCM-1'), strategy.generate_order_id(AUDUSD_FXCM))

    # def test_submitting_order_with_identical_id_raises_ex(self):
    #     # Arrange
    #     strategy = TradeStrategy()
    #     exec_client = MockExecClient()
    #     exec_client.register_strategy(strategy)
    #
    #     order = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderId('AUDUSD-123456-1'),
    #         Label('S1'),
    #         OrderSide.BUY,
    #         100000)
    #
    #     position_id = PositionId(str(order.id))
    #     strategy.submit_order(order, position_id)
    #
    #     # Act
    #     # Assert
    #     self.assertRaises(ValueError, strategy.submit_order, order, position_id)

    def test_strategy_can_submit_order(self):
        # Arrange
        strategy = TradeStrategy()
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
        self.assertEqual(order, strategy.orders_all()[order.id])
        self.assertEqual(OrderStatus.WORKING, strategy.orders_all()[order.id].status)
        self.assertTrue(order.id in strategy.orders_active())
        self.assertTrue(order.id not in strategy.orders_completed())

    # def test_cancelling_order_which_does_not_exist_raises_ex(self):
    #     # Arrange
    #     strategy = TradeStrategy()
    #     exec_client = MockExecClient()
    #     exec_client.register_strategy(strategy)
    #
    #     order = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderId('AUDUSD-123456-1'),
    #         Label('S1'),
    #         OrderSide.BUY,
    #         100000)
    #
    #     # Act
    #     # Assert
    #     self.assertRaises(ValueError, strategy.cancel_order, order, 'NONE')

    def test_can_cancel_order(self):
        # Arrange
        strategy = TradeStrategy()
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
        self.assertEqual(order, strategy.orders_all()[order.id])
        self.assertEqual(OrderStatus.CANCELLED, strategy.orders_all()[order.id].status)
        self.assertTrue(order.id in strategy.orders_completed())
        self.assertTrue(order.id not in strategy.orders_active())

    # def test_modifying_order_which_does_not_exist_raises_ex(self):
    #     # Arrange
    #     strategy = TradeStrategy()
    #     exec_client = MockExecClient()
    #     exec_client.register_strategy(strategy)
    #
    #     order = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderId('AUDUSD-123456-1'),
    #         Label('S1'),
    #         OrderSide.BUY,
    #         100000)
    #
    #     # Act
    #     # Assert
    #     self.assertRaises(ValueError, strategy.modify_order, order, Price(1.00001, 5))

    def test_can_modify_order(self):
        # Arrange
        strategy = TradeStrategy()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5),
            TimeInForce.DAY,
            None)

        strategy.submit_order(order, PositionId(str(order.id)))

        # Act
        strategy.modify_order(order, Price(1.00001, 5))

        # Assert
        self.assertEqual(order, strategy.orders_all()[order.id])
        self.assertEqual(OrderStatus.WORKING, strategy.orders_all()[order.id].status)
        self.assertEqual(Price(1.00001, 5), strategy.orders_all()[order.id].price)
        self.assertTrue(strategy.is_flat())

    def test_can_cancel_all_orders(self):
        # Arrange
        strategy = TradeStrategy()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        order1 = self.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5),
            TimeInForce.DAY,
            None)

        order2 = self.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-2'),
            Label('S1'),
            OrderSide.BUY,
            100000,
            Price(1.00010, 5),
            TimeInForce.DAY,
            None)

        strategy.submit_order(order1, PositionId('some-position'))
        strategy.submit_order(order2, PositionId('some-position'))

        # Act
        strategy.cancel_all_orders('TEST')

        # Assert
        self.assertEqual(order1, strategy.orders_all()[order1.id])
        self.assertEqual(order2, strategy.orders_all()[order2.id])
        self.assertEqual(OrderStatus.CANCELLED, strategy.orders_all()[order1.id].status)
        self.assertEqual(OrderStatus.CANCELLED, strategy.orders_all()[order2.id].status)
        self.assertTrue(order1.id in strategy.orders_completed())
        self.assertTrue(order2.id in strategy.orders_completed())

    def test_can_flatten_position(self):
        # Arrange
        strategy = TradeStrategy()
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
        self.assertEqual(order, strategy.orders_all()[order.id])
        self.assertEqual(OrderStatus.FILLED, strategy.orders_all()[order.id].status)
        self.assertEqual(MarketPosition.FLAT, strategy.positions_all()[PositionId('some-position')].market_position)
        self.assertTrue(strategy.positions_all()[PositionId('some-position')].is_exited)
        self.assertTrue(PositionId('some-position') in strategy.positions_closed())
        self.assertTrue(strategy.is_flat())

    # def test_flatten_position_which_does_not_exist_raises_exception(self):
    #     # Arrange
    #     strategy = TradeStrategy()
    #     exec_client = MockExecClient()
    #     exec_client.register_strategy(strategy)
    #
    #     # Act
    #     # Assert
    #     self.assertRaises(ValueError, strategy.flatten_position, PositionId('some-position'))

    def test_can_flatten_all_positions(self):
        # Arrange
        strategy = TradeStrategy()
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
        self.assertEqual(order1, strategy.orders_all()[order1.id])
        self.assertEqual(order2, strategy.orders_all()[order2.id])
        self.assertEqual(OrderStatus.FILLED, strategy.orders_all()[order1.id].status)
        self.assertEqual(OrderStatus.FILLED, strategy.orders_all()[order2.id].status)
        self.assertEqual(MarketPosition.FLAT, strategy.positions_all()[PositionId('some-position1')].market_position)
        self.assertEqual(MarketPosition.FLAT, strategy.positions_all()[PositionId('some-position2')].market_position)
        self.assertTrue(strategy.positions_all()[PositionId('some-position1')].is_exited)
        self.assertTrue(strategy.positions_all()[PositionId('some-position2')].is_exited)
        self.assertTrue(PositionId('some-position1') in strategy.positions_closed())
        self.assertTrue(PositionId('some-position2') in strategy.positions_closed())
        self.assertTrue(strategy.is_flat())

    def test_can_update_bars_and_indicators(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Price('1.00001'),
            Price('1.00004'),
            Price('1.00002'),
            Price('1.00003'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        # Act
        strategy._update_bars(bar_type, bar)

        # Assert
        self.assertEqual(1, len(strategy.bars(bar_type)))
        self.assertEqual(1, strategy.ema1.count)
        self.assertEqual(1, strategy.ema2.count)
        self.assertEqual(0, len(strategy.object_storer.get_store()))

    def test_can_track_orders_for_an_opened_position(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
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
        self.assertTrue(OrderId('AUDUSD-123456-1') in strategy.orders_all())
        self.assertTrue(PositionId('AUDUSD-123456-1') in strategy.positions_all())
        self.assertEqual(0, len(strategy.orders_active()))
        self.assertEqual(order, strategy.orders_completed()[order.id])
        self.assertEqual(0, len(strategy.positions_closed()))
        self.assertTrue(OrderId('AUDUSD-123456-1') in strategy.orders_completed())
        self.assertTrue(PositionId('AUDUSD-123456-1') in strategy.positions_active())
        self.assertFalse(strategy.is_flat())

    def test_can_track_orders_for_a_closing_position(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
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
        self.assertEqual(0, len(strategy.orders_active()))
        self.assertEqual(order1, strategy.orders_completed()[order1.id])
        self.assertEqual(order2, strategy.orders_completed()[order2.id])
        self.assertEqual(1, len(strategy.positions_closed()))
        self.assertFalse(PositionId('position1') in strategy.positions_active())
        self.assertTrue(PositionId('position1') in strategy.positions_closed())
        self.assertTrue(strategy.is_flat())
