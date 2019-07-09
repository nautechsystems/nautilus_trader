#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_strategy.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import datetime
import time

from datetime import datetime, timezone, timedelta

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.account import Account
from nautilus_trader.common.brokerage import CommissionCalculator
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.model.enums import Venue, OrderSide
from nautilus_trader.model.objects import Quantity, Symbol, Price, Money
from nautilus_trader.model.identifiers import OrderId, PositionId
from nautilus_trader.model.position import Position
from nautilus_trader.model.enums import OrderStatus, Currency
from nautilus_trader.model.enums import MarketPosition
from nautilus_trader.model.objects import Tick, Bar
from nautilus_trader.model.events import TimeEvent
from nautilus_trader.model.identifiers import StrategyId, Label
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.trade.portfolio import Portfolio
from nautilus_trader.trade.strategy import TradeStrategy
from test_kit.stubs import TestStubs
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = Symbol('USDJPY', Venue.FXCM)
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)


class TradeStrategyTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup

        self.instruments = [TestStubs.instrument_usdjpy()]
        self.account = Account()

        self.portfolio = Portfolio(
            clock=TestClock(),
            guid_factory=TestGuidFactory(),
            logger=TestLogger())

        self.exec_client = BacktestExecClient(
            instruments=self.instruments,
            frozen_account=False,
            starting_capital=Money(1000000),
            fill_model=FillModel(),
            commission_calculator=CommissionCalculator(),
            account=self.account,
            portfolio=self.portfolio,
            clock=TestClock(),
            guid_factory=TestGuidFactory(),
            logger=TestLogger())
        self.portfolio.register_execution_client(self.exec_client)

        bar = TestStubs.bar_3decimal()
        self.exec_client.process_bars(USDJPY_FXCM, bar, bar)  # Prepare market

        print('\n')

    def test_strategy_equality(self):
        # Arrange
        strategy1 = TradeStrategy(id_tag_strategy='001')
        strategy2 = TradeStrategy(id_tag_strategy='AUDUSD-001')
        strategy3 = TradeStrategy(id_tag_strategy='AUDUSD-002')

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
        strategy = TradeStrategy(id_tag_strategy='001')

        # Act
        result = strategy.__hash__()

        # Assert
        # If this passes then result must be an int.
        self.assertTrue(result != 0)

    def test_strategy_str_and_repr(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='GBPUSD-MM')

        # Act
        result1 = str(strategy)
        result2 = repr(strategy)

        # Assert
        self.assertEqual('TradeStrategy(TradeStrategy-GBPUSD-MM)', result1)
        self.assertTrue(result2.startswith('<TradeStrategy(TradeStrategy-GBPUSD-MM) object at'))
        self.assertTrue(result2.endswith('>'))

    def test_can_get_strategy_id(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')

        # Act
        # Assert
        self.assertEqual(StrategyId('TradeStrategy-001'), strategy.id)

    def test_can_get_current_time(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')

        # Act
        result = strategy.time_now()

        # Assert
        self.assertEqual(timezone.utc, result.tzinfo)

    def test_can_get_indicators(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        # Act
        result = strategy.indicators(strategy.bar_type)

        # Assert
        self.assertTrue(2, len(result))
        print(result)

    def test_indicator_initialization(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        # Act
        result1 = strategy.indicators_initialized(bar_type)
        result2 = strategy.indicators_initialized_all()

        # Assert
        self.assertFalse(result1)
        self.assertFalse(result2)

    def test_getting_indicators_for_unknown_bar_type_raises_exception(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.indicators, bar_type)

    def test_getting_bars_for_unknown_bar_type_raises_exception(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.bars, bar_type)

    def test_can_get_bars(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(Price('1.00001'),
                  Price('1.00004'),
                  Price('1.00002'),
                  Price('1.00003'),
                  100000,
                  datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy.handle_bar(bar_type, bar)

        # Act
        result = strategy.bars(bar_type)

        # Assert
        self.assertTrue(bar, result[0])

    def test_getting_bar_for_unknown_bar_type_raises_exception(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        unknown_bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.bar, unknown_bar_type, 0)

    def test_getting_bar_at_out_of_range_index_raises_exception(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(Price('1.00001'),
                  Price('1.00004'),
                  Price('1.00002'),
                  Price('1.00003'),
                  100000,
                  datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy.handle_bar(bar_type, bar)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.bar, bar_type, -2)

    def test_can_get_bar(self):
        strategy = TradeStrategy(id_tag_strategy='001')
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(Price('1.00001'),
                  Price('1.00004'),
                  Price('1.00002'),
                  Price('1.00003'),
                  100000,
                  datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy.handle_bar(bar_type, bar)

        # Act
        result = strategy.bar(bar_type, 0)

        # Assert
        self.assertEqual(bar, result)

    def test_can_get_last_bar(self):
        strategy = TradeStrategy(id_tag_strategy='001')
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(Price('1.00001'),
                  Price('1.00004'),
                  Price('1.00002'),
                  Price('1.00003'),
                  100000,
                  datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy.handle_bar(bar_type, bar)

        # Act
        result = strategy.last_bar(bar_type)

        # Assert
        self.assertEqual(bar, result)

    def test_getting_last_tick_with_unknown_symbol_raises_exception(self):
        strategy = TradeStrategy(id_tag_strategy='001')

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.last_tick, AUDUSD_FXCM)

    def test_can_get_last_tick(self):
        strategy = TradeStrategy(id_tag_strategy='001')

        tick = Tick(Symbol('AUDUSD', Venue.FXCM),
                    Price('1.00000'),
                    Price('1.00001'),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        strategy.handle_tick(tick)

        # Act
        result = strategy.last_tick(AUDUSD_FXCM)

        # Assert
        self.assertEqual(tick, result)

    def test_getting_order_which_does_not_exist_raises_exception(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.order, OrderId('unknown_order_id'))

    def test_can_get_order(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Act
        result = strategy.order(order.id)

        # Assert
        self.assertTrue(strategy.is_order_exists(order.id))
        self.assertEqual(order, result)

    def test_getting_position_which_does_not_exist_raises_exception(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.position, PositionId('unknown'))
        self.assertFalse(strategy.is_position_exists(PositionId('unknown')))

    def test_can_get_position(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = strategy.position_id_generator.generate()

        strategy.submit_order(order, position_id)

        # Act
        result = strategy.position(position_id)

        # Assert
        self.assertTrue(strategy.is_position_exists(position_id))
        self.assertTrue(type(result) == Position)

    def test_can_start_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        self.exec_client.register_strategy(strategy)

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
        self.exec_client.register_strategy(strategy)

        # Act
        strategy.stop()

        # Assert
        self.assertFalse(strategy.is_running)
        self.assertTrue('custom stop logic' in strategy.object_storer.get_store())

    def test_can_reset_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        bar = Bar(Price('1.00001'),
                  Price('1.00004'),
                  Price('1.00002'),
                  Price('1.00003'),
                  100000,
                  datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        strategy.handle_bar(bar_type, bar)

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
        strategy = TradeStrategy(id_tag_strategy='001')

        # Act
        self.exec_client.register_strategy(strategy)

        # Assert
        self.assertTrue(True)  # No exceptions thrown

    def test_can_get_exchange_rate_with_conversion(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        tick = Tick(Symbol('USDJPY', Venue.FXCM),
                    Price('110.80000'),
                    Price('110.80010'),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        strategy.handle_tick(tick)

        # Act
        result = strategy.get_exchange_rate(Currency.JPY)

        # Assert
        self.assertEqual(0.009025266394019127, result)

    def test_can_get_exchange_rate_with_no_conversion(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        tick = Tick(Symbol('AUDUSD', Venue.FXCM),
                    Price('0.80000'),
                    Price('0.80010'),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        strategy.handle_tick(tick)

        # Act
        result = strategy.get_exchange_rate(Currency.USD)

        # Assert
        self.assertEqual(1.0, result)

    def test_can_set_time_alert(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type, clock=LiveClock())
        self.exec_client.register_strategy(strategy)

        alert_time = datetime.now(timezone.utc) + timedelta(milliseconds=300)
        strategy.clock.set_time_alert(Label("test_alert1"), alert_time)

        # Act
        strategy.start()
        time.sleep(0.9)
        strategy.stop()

        # Assert
        self.assertEqual(3, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[1], TimeEvent))

    def test_can_cancel_time_alert(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        self.exec_client.register_strategy(strategy)

        alert_time = datetime.now(timezone.utc) + timedelta(seconds=1)
        strategy.clock.set_time_alert(Label("test_alert1"), alert_time)

        # Act
        strategy.start()
        time.sleep(0.5)
        strategy.clock.cancel_time_alert(Label("test_alert1"))
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_stopping_a_strategy_cancels_a_running_time_alert(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        self.exec_client.register_strategy(strategy)

        alert_time = datetime.now(timezone.utc) + timedelta(milliseconds=200)
        strategy.clock.set_time_alert(Label("test_alert1"), alert_time)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_can_set_multiple_time_alerts(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type, clock=LiveClock())
        self.exec_client.register_strategy(strategy)

        alert_time1 = datetime.now(timezone.utc) + timedelta(milliseconds=200)
        alert_time2 = datetime.now(timezone.utc) + timedelta(milliseconds=300)

        # Act
        strategy.clock.set_time_alert(Label("test_alert1"), alert_time1)
        strategy.clock.set_time_alert(Label("test_alert2"), alert_time2)
        strategy.start()
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertTrue(isinstance(strategy.object_storer.get_store()[1], TimeEvent))
        self.assertTrue(isinstance(strategy.object_storer.get_store()[2], TimeEvent))

    def test_can_set_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type, clock=LiveClock())
        self.exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.clock.set_timer(Label("test_timer1"), timedelta(milliseconds=100), start_time, stop_time=None)

        # Act
        strategy.start()
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertTrue(isinstance(strategy.object_storer.get_store()[1], TimeEvent))

    def test_can_cancel_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type, clock=LiveClock())
        self.exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.clock.set_timer(Label("test_timer2"), timedelta(milliseconds=100), start_time, stop_time=None)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.clock.cancel_timer(Label("test_timer2"))
        time.sleep(0.5)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_stopping_a_strategy_cancels_a_running_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        self.exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.clock.set_timer(Label("test_timer3"), timedelta(milliseconds=100), start_time, stop_time=None)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_can_set_repeating_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type, clock=LiveClock())
        self.exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.clock.set_timer(Label("test_timer4"), timedelta(milliseconds=100), start_time, stop_time=None)

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
        strategy = TestStrategy1(bar_type, clock=LiveClock())
        self.exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        stop_time = start_time + timedelta(seconds=5)
        strategy.start()
        strategy.clock.set_timer(Label("test_timer5"), timedelta(milliseconds=100), start_time, stop_time)

        # Act
        strategy.clock.cancel_timer(Label("test_timer5"))
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_can_set_two_repeating_timers(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type, clock=LiveClock())
        self.exec_client.register_strategy(strategy)

        start_time = datetime.now(timezone.utc) + timedelta(milliseconds=100)
        strategy.clock.set_timer(Label("test_timer6"), timedelta(milliseconds=100), start_time, stop_time=None)
        strategy.clock.set_timer(Label("test_timer7"), timedelta(milliseconds=100), start_time, stop_time=None)

        # Act
        strategy.start()
        time.sleep(0.55)
        strategy.stop()

        # Assert
        self.assertEqual(10, strategy.object_storer.count)

    def test_can_generate_position_id(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001', clock=TestClock())

        # Act
        result = strategy.position_id_generator.generate()

        # Assert
        self.assertEqual(PositionId('P-19700101-000000-000-001-1'), result)

    def test_get_opposite_side_returns_expected_sides(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')

        # Act
        result1 = strategy.get_opposite_side(OrderSide.BUY)
        result2 = strategy.get_opposite_side(OrderSide.SELL)

        # Assert
        self.assertEqual(OrderSide.SELL, result1)
        self.assertEqual(OrderSide.BUY, result2)

    def test_get_flatten_side_with_long_or_short_market_position_returns_expected_sides(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')

        # Act
        result1 = strategy.get_flatten_side(MarketPosition.LONG)
        result2 = strategy.get_flatten_side(MarketPosition.SHORT)

        # Assert
        self.assertEqual(OrderSide.SELL, result1)
        self.assertEqual(OrderSide.BUY, result2)

    def test_can_change_clock(self):
        # Arrange
        clock = TestClock()
        strategy = TradeStrategy(id_tag_strategy='001')

        # Act
        strategy.change_clock(clock)

        # Assert
        self.assertEqual(UNIX_EPOCH, strategy.time_now())
        self.assertEqual(PositionId('P-19700101-000000-000-001-1'), strategy.position_id_generator.generate())

    def test_strategy_can_submit_order(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Assert
        self.assertEqual(order, strategy.orders_all()[order.id])
        self.assertEqual(OrderStatus.FILLED, strategy.orders_all()[order.id].status)
        self.assertTrue(order.id not in strategy.orders_active())
        self.assertTrue(order.id in strategy.orders_completed())
        self.assertTrue(strategy.is_order_exists(order.id))
        self.assertFalse(strategy.is_order_active(order.id))
        self.assertTrue(strategy.is_order_complete(order.id))

    def test_can_cancel_order(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        order = strategy.order_factory.stop_market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(90.005, 3))

        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Act
        strategy.cancel_order(order, 'NONE')

        # Assert
        self.assertEqual(order, strategy.orders_all()[order.id])
        self.assertEqual(OrderStatus.CANCELLED, strategy.orders_all()[order.id].status)
        self.assertTrue(order.id in strategy.orders_completed())
        self.assertTrue(order.id not in strategy.orders_active())
        self.assertTrue(strategy.is_order_exists(order.id))
        self.assertFalse(strategy.is_order_active(order.id))
        self.assertTrue(strategy.is_order_complete(order.id))

    def test_can_modify_order(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        order = strategy.order_factory.limit(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(90.003, 3))

        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Act
        strategy.modify_order(order, Price(90.005, 3))

        # Assert
        self.assertEqual(order, strategy.orders_all()[order.id])
        self.assertEqual(OrderStatus.WORKING, strategy.orders_all()[order.id].status)
        self.assertEqual(Price(90.005, 3), strategy.orders_all()[order.id].price)
        self.assertTrue(strategy.is_flat())
        self.assertTrue(strategy.is_order_exists(order.id))
        self.assertTrue(strategy.is_order_active(order.id))
        self.assertFalse(strategy.is_order_complete(order.id))

    def test_can_cancel_all_orders(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        order1 = strategy.order_factory.stop_market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(90.003, 3))

        order2 = strategy.order_factory.stop_market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(90.005, 3))

        position_id = strategy.position_id_generator.generate()

        strategy.submit_order(order1, position_id)
        strategy.submit_order(order2, position_id)

        # Act
        strategy.cancel_all_orders()

        # Assert
        self.assertEqual(order1, strategy.orders_all()[order1.id])
        self.assertEqual(order2, strategy.orders_all()[order2.id])
        self.assertEqual(OrderStatus.CANCELLED, strategy.orders_all()[order1.id].status)
        self.assertEqual(OrderStatus.CANCELLED, strategy.orders_all()[order2.id].status)
        self.assertTrue(order1.id in strategy.orders_completed())
        self.assertTrue(order2.id in strategy.orders_completed())

    def test_can_flatten_position(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = strategy.position_id_generator.generate()

        strategy.submit_order(order, position_id)

        # Act
        strategy.flatten_position(position_id)

        # Assert
        self.assertEqual(order, strategy.orders_all()[order.id])
        self.assertEqual(OrderStatus.FILLED, strategy.orders_all()[order.id].status)
        self.assertEqual(MarketPosition.FLAT, strategy.positions_all()[position_id].market_position)
        self.assertTrue(strategy.positions_all()[position_id].is_exited)
        self.assertTrue(position_id in strategy.positions_closed())
        self.assertTrue(strategy.is_flat())

    def test_can_flatten_all_positions(self):
        # Arrange
        strategy = TradeStrategy(id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order2 = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id1 = strategy.position_id_generator.generate()
        position_id2 = strategy.position_id_generator.generate()

        strategy.submit_order(order1, position_id1)
        strategy.submit_order(order2, position_id2)

        # Act
        strategy.flatten_all_positions()

        # Assert
        self.assertEqual(order1, strategy.orders_all()[order1.id])
        self.assertEqual(order2, strategy.orders_all()[order2.id])
        self.assertEqual(OrderStatus.FILLED, strategy.orders_all()[order1.id].status)
        self.assertEqual(OrderStatus.FILLED, strategy.orders_all()[order2.id].status)
        self.assertEqual(MarketPosition.FLAT, strategy.positions_all()[position_id1].market_position)
        self.assertEqual(MarketPosition.FLAT, strategy.positions_all()[position_id2].market_position)
        self.assertTrue(strategy.positions_all()[position_id1].is_exited)
        self.assertTrue(strategy.positions_all()[position_id2].is_exited)
        self.assertTrue(position_id1 in strategy.positions_closed())
        self.assertTrue(position_id2 in strategy.positions_closed())
        self.assertTrue(strategy.is_flat())

    def test_can_update_bars_and_indicators(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)
        bar = Bar(Price('1.00001'),
                  Price('1.00004'),
                  Price('1.00002'),
                  Price('1.00003'),
                  100000,
                  datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc))

        # Act
        strategy.handle_bar(bar_type, bar)

        # Assert
        self.assertEqual(1, len(strategy.bars(bar_type)))
        self.assertEqual(1, strategy.ema1.count)
        self.assertEqual(1, strategy.ema2.count)
        self.assertEqual(0, len(strategy.object_storer.get_store()))

    def test_can_track_orders_for_an_opened_position(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        self.exec_client.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Act
        # Assert
        self.assertTrue(OrderId('O-19700101-000000-000-001-1') in strategy.orders_all())
        self.assertTrue(PositionId('P-19700101-000000-000-001-1') in strategy.positions_all())
        self.assertEqual(0, len(strategy.orders_active()))
        self.assertEqual(order, strategy.orders_completed()[order.id])
        self.assertEqual(0, len(strategy.positions_closed()))
        self.assertTrue(OrderId('O-19700101-000000-000-001-1') in strategy.orders_completed())
        self.assertTrue(PositionId('P-19700101-000000-000-001-1') in strategy.positions_active())
        self.assertFalse(strategy.is_flat())

    def test_can_track_orders_for_a_closing_position(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        self.exec_client.register_strategy(strategy)

        position1 = PositionId('position1')
        order1 = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order2 = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        strategy.submit_order(order1, position1)
        strategy.submit_order(order2, position1)

        # Act
        # Assert
        self.assertEqual(0, len(strategy.orders_active()))
        self.assertEqual(order1, strategy.orders_completed()[order1.id])
        self.assertEqual(order2, strategy.orders_completed()[order2.id])
        self.assertEqual(1, len(strategy.positions_closed()))
        self.assertFalse(PositionId('position1') in strategy.positions_active())
        self.assertTrue(PositionId('position1') in strategy.positions_closed())
        self.assertTrue(strategy.is_flat())
