# -------------------------------------------------------------------------------------------------
# <copyright file="test_model_order.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from decimal import Decimal

from nautilus_trader.core.correctness import ConditionFailed
from nautilus_trader.core.types import GUID, ValidString
from nautilus_trader.common.clock import TestClock
from nautilus_trader.model.enums import OrderSide, OrderType, OrderState, OrderPurpose, TimeInForce
from nautilus_trader.model.objects import Quantity, Price
from nautilus_trader.model.identifiers import (
    Symbol,
    Venue,
    Label,
    IdTag,
    OrderId,
    OrderIdBroker,
    AtomicOrderId,
    AccountId,
    ExecutionId,
    ExecutionTicket)
from nautilus_trader.model.order import Order, OrderFactory
from nautilus_trader.model.events import OrderInitialized, OrderSubmitted, OrderAccepted, OrderRejected
from nautilus_trader.model.events import OrderWorking, OrderExpired, OrderModified, OrderCancelled
from nautilus_trader.model.events import OrderCancelReject, OrderPartiallyFilled, OrderFilled
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))


class OrderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account_id = TestStubs.account_id()
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())

    def test_market_order_with_quantity_zero_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ConditionFailed,
            Order,
            OrderId('O-123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.MARKET,
            Quantity.zero(),
            UNIX_EPOCH)

    def test_market_order_with_order_side_none_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ConditionFailed,
            Order,
            OrderId('O-123456'),
            AUDUSD_FXCM,
            OrderSide.UNKNOWN,
            OrderType.MARKET,
            Quantity(100000),
            UNIX_EPOCH)

    def test_priced_order_with_GTD_time_in_force_and_expire_time_none_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ConditionFailed,
            Order,
            OrderId('O-123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.LIMIT,
            Quantity(100000),
            UNIX_EPOCH,
            price=Price('1.00000'),
            time_in_force=TimeInForce.GTD,
            expire_time=None)

    def test_market_order_with_price_input_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ConditionFailed,
            Order,
            OrderId('O-123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.MARKET,
            Quantity(100000),
            UNIX_EPOCH,
            price=Price('1.00000'))

    def test_stop_order_with_no_price_input_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ConditionFailed,
            Order,
            OrderId('O-123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.STOP_MARKET,
            Quantity(100000),
            UNIX_EPOCH)

    def test_stop_order_with_zero_price_input_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ConditionFailed,
            Order,
            OrderId('O-123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.STOP_MARKET,
            Quantity(100000),
            UNIX_EPOCH,
            price=None)

    def test_can_reset_order_factory(self):
        # Arrange
        self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        # Act
        self.order_factory.reset()

        order2 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        self.assertEqual(OrderId('O-19700101-000000-001-001-1'), order2.id)

    def test_limit_order_can_create_expected_decimal_price(self):
        # Arrange
        # Act
        order1 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        order2 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        order3 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        order4 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00001'))

        # Assert
        self.assertEqual(Price('1.00000'), order1.price)
        self.assertEqual(Price('1.00000'), order2.price)
        self.assertEqual(Price('1.00000'), order3.price)
        self.assertEqual(Price('1.00001'), order4.price)

    def test_can_initialize_buy_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(1, order.event_count)
        self.assertTrue(isinstance(order.last_event, OrderInitialized))
        self.assertFalse(order.is_working)
        self.assertFalse(order.is_completed)
        self.assertTrue(order.is_buy)
        self.assertFalse(order.is_sell)
        self.assertEqual(None, order.filled_timestamp)

    def test_can_initialize_sell_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(1, order.event_count)
        self.assertTrue(isinstance(order.last_event, OrderInitialized))
        self.assertFalse(order.is_working)
        self.assertFalse(order.is_completed)
        self.assertFalse(order.is_buy)
        self.assertTrue(order.is_sell)
        self.assertEqual(None, order.filled_timestamp)

    def test_order_str_and_repr(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),)

        # Assert
        self.assertEqual('Order(id=O-19700101-000000-001-001-1, state=INITIALIZED) BUY 100,000 AUDUSD.FXCM MARKET DAY', str(order))
        self.assertTrue(repr(order).startswith('<Order(id=O-19700101-000000-001-001-1, state=INITIALIZED) BUY 100,000 AUDUSD.FXCM MARKET DAY object at'))

    def test_can_initialize_limit_order(self):
        # Arrange
        # Act
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        # Assert
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(TimeInForce.DAY, order.time_in_force)
        self.assertFalse(order.is_completed)

    def test_can_initialize_limit_order_with_expire_time(self):
        # Arrange
        # Act
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            Label('U1_TP'),
            OrderPurpose.NONE,
            TimeInForce.GTD,
            UNIX_EPOCH)

        # Assert
        self.assertEqual(AUDUSD_FXCM, order.symbol)
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(Price('1.00000'), order.price)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(TimeInForce.GTD, order.time_in_force)
        self.assertEqual(UNIX_EPOCH, order.expire_time)
        self.assertFalse(order.is_completed)

    def test_can_initialize_stop_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        # Assert
        self.assertEqual(OrderType.STOP_MARKET, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(TimeInForce.DAY, order.time_in_force)
        self.assertFalse(order.is_completed)

    def test_can_initialize_stop_limit_order(self):
        # Arrange
        # Act
        order = self.order_factory.stop_limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        # Assert
        self.assertEqual(OrderType.STOP_LIMIT, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_initialize_market_if_touched_order(self):
        # Arrange
        # Act
        order = self.order_factory.market_if_touched(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        # Assert
        self.assertEqual(OrderType.MIT, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_initialize_fill_or_kill_order(self):
        # Arrange
        # Act
        order = self.order_factory.fill_or_kill(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(TimeInForce.FOC, order.time_in_force)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_initialize_immediate_or_cancel_order(self):
        # Arrange
        # Act
        order = self.order_factory.immediate_or_cancel(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(TimeInForce.IOC, order.time_in_force)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_initialize_atomic_order_market_with_no_take_profit_or_label(self):
        # Arrange
        # Act
        atomic_order = self.order_factory.atomic_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('0.99990'))

        # Assert
        self.assertEqual(AUDUSD_FXCM, atomic_order.stop_loss.symbol)
        self.assertFalse(atomic_order.has_take_profit)
        self.assertEqual(OrderId('O-19700101-000000-001-001-1'), atomic_order.entry.id)
        self.assertEqual(OrderId('O-19700101-000000-001-001-2'), atomic_order.stop_loss.id)
        self.assertEqual(OrderSide.SELL, atomic_order.stop_loss.side)
        self.assertEqual(Quantity(100000), atomic_order.entry.quantity)
        self.assertEqual(Quantity(100000), atomic_order.stop_loss.quantity)
        self.assertEqual(Price('0.99990'), atomic_order.stop_loss.price)
        self.assertEqual(None, atomic_order.entry.label)
        self.assertEqual(None, atomic_order.stop_loss.label)
        self.assertEqual(TimeInForce.GTC, atomic_order.stop_loss.time_in_force)
        self.assertEqual(None, atomic_order.entry.expire_time)
        self.assertEqual(None, atomic_order.stop_loss.expire_time)
        self.assertEqual(AtomicOrderId('AO-19700101-000000-001-001-1'), atomic_order.id)
        self.assertEqual(UNIX_EPOCH, atomic_order.timestamp)

    def test_can_initialize_atomic_order_market_with_take_profit_and_label(self):
        # Arrange
        # Act
        atomic_order = self.order_factory.atomic_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('0.99990'),
            Price('1.00010'),
            Label('U1'))

        # Assert
        self.assertEqual(AUDUSD_FXCM, atomic_order.stop_loss.symbol)
        self.assertTrue(atomic_order.has_take_profit)
        self.assertEqual(AUDUSD_FXCM, atomic_order.take_profit.symbol)
        self.assertEqual(OrderId('O-19700101-000000-001-001-1'), atomic_order.entry.id)
        self.assertEqual(OrderId('O-19700101-000000-001-001-2'), atomic_order.stop_loss.id)
        self.assertEqual(OrderId('O-19700101-000000-001-001-3'), atomic_order.take_profit.id)
        self.assertEqual(OrderSide.SELL, atomic_order.stop_loss.side)
        self.assertEqual(OrderSide.SELL, atomic_order.take_profit.side)
        self.assertEqual(Quantity(100000), atomic_order.stop_loss.quantity)
        self.assertEqual(Quantity(100000), atomic_order.take_profit.quantity)
        self.assertEqual(Price('0.99990'), atomic_order.stop_loss.price)
        self.assertEqual(Price('1.00010'), atomic_order.take_profit.price)
        self.assertEqual(Label('U1_E'), atomic_order.entry.label)
        self.assertEqual(Label('U1_SL'), atomic_order.stop_loss.label)
        self.assertEqual(Label('U1_TP'), atomic_order.take_profit.label)
        self.assertEqual(TimeInForce.GTC, atomic_order.stop_loss.time_in_force)
        self.assertEqual(TimeInForce.GTC, atomic_order.take_profit.time_in_force)
        self.assertEqual(None, atomic_order.entry.expire_time)
        self.assertEqual(None, atomic_order.stop_loss.expire_time)
        self.assertEqual(None, atomic_order.take_profit.expire_time)
        self.assertEqual(AtomicOrderId('AO-19700101-000000-001-001-1'), atomic_order.id)
        self.assertEqual(UNIX_EPOCH, atomic_order.timestamp)

    def test_atomic_order_str_and_repr(self):
        # Arrange
        # Act
        atomic_order = self.order_factory.atomic_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('0.99990'),
            Price('1.00010'),
            Label('U1'))

        # Assert
        self.assertEqual('AtomicOrder(id=AO-19700101-000000-001-001-1, EntryOrder(id=O-19700101-000000-001-001-1, state=INITIALIZED, label=U1_E, purpose=ENTRY) BUY 100,000 AUDUSD.FXCM MARKET DAY, SL=0.99990, TP=1.00010)', str(atomic_order))
        self.assertTrue(repr(atomic_order).startswith('<AtomicOrder(id=AO-19700101-000000-001-001-1, EntryOrder(id=O-19700101-000000-001-001-1, state=INITIALIZED, label=U1_E, purpose=ENTRY) BUY 100,000 AUDUSD.FXCM MARKET DAY, SL=0.99990, TP=1.00010) object at'))

    def test_can_apply_order_submitted_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderSubmitted(
            order.id,
            self.account_id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.SUBMITTED, order.state)
        self.assertEqual(2, order.event_count)
        self.assertEqual(event, order.last_event)
        self.assertFalse(order.is_completed)

    def test_can_apply_order_accepted_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderAccepted(
            order.id,
            self.account_id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.ACCEPTED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_apply_order_rejected_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderRejected(
            order.id,
            self.account_id,
            UNIX_EPOCH,
            ValidString('ORDER ID INVALID'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.REJECTED, order.state)
        self.assertTrue(order.is_completed)

    def test_can_apply_order_working_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderWorking(
            order.id,
            OrderIdBroker('SOME_BROKER_ID'),
            self.account_id,
            order.symbol,
            order.label,
            order.side,
            order.type,
            order.quantity,
            Price('1.0'),
            order.time_in_force,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH,
            order.expire_time)

        # Act
        order.apply(event)

        # Assert
        # print(order)
        self.assertEqual(OrderState.WORKING, order.state)
        self.assertEqual(OrderIdBroker('SOME_BROKER_ID'), order.id_broker)
        self.assertFalse(order.is_completed)
        self.assertTrue(order.is_working)
        self.assertEqual(None, order.filled_timestamp)

    def test_can_apply_order_expired_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderExpired(
            order.id,
            self.account_id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.EXPIRED, order.state)
        self.assertTrue(order.is_completed)

    def test_can_apply_order_cancelled_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderCancelled(
            order.id,
            self.account_id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.CANCELLED, order.state)
        self.assertTrue(order.is_completed)

    def test_can_apply_order_cancel_reject_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderCancelReject(
            order.id,
            self.account_id,
            UNIX_EPOCH,
            ValidString('REJECT_RESPONSE'),
            ValidString('ORDER DOES NOT EXIST'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.INITIALIZED, order.state)

    def test_can_apply_order_modified_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order_working = OrderWorking(
            order.id,
            OrderIdBroker('SOME_BROKER_ID_1'),
            self.account_id,
            order.symbol,
            order.label,
            order.side,
            order.type,
            order.quantity,
            Price('1.00000'),
            order.time_in_force,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH,
            order.expire_time)

        order_modified = OrderModified(
            order.id,
            OrderIdBroker('SOME_BROKER_ID_2'),
            self.account_id,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order.apply(order_working)

        # Act
        order.apply(order_modified)

        # Assert
        self.assertEqual(OrderState.WORKING, order.state)
        self.assertEqual(OrderIdBroker('SOME_BROKER_ID_2'), order.id_broker)
        self.assertEqual(Price('1.00001'), order.price)
        self.assertTrue(order.is_working)
        self.assertFalse(order.is_completed)
        self.assertEqual(3, order.event_count)

    def test_can_apply_order_filled_event_to_market_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderFilled(
            order.id,
            self.account_id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order.symbol,
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Quantity(100000), order.filled_quantity)
        self.assertEqual(Price('1.00001'), order.average_price)
        self.assertTrue(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_can_apply_order_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        event = OrderFilled(
            order.id,
            self.account_id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order.symbol,
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Quantity(100000), order.filled_quantity)
        self.assertEqual(Price('1.00000'), order.price)
        self.assertEqual(Price('1.00001'), order.average_price)
        self.assertEqual(Decimal('0.00001'), order.slippage)
        self.assertTrue(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_can_apply_order_partially_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        event = OrderPartiallyFilled(
            order.id,
            self.account_id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order.symbol,
            order.side,
            Quantity(50000),
            Quantity(50000),
            Price('0.99999'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.PARTIALLY_FILLED, order.state)
        self.assertEqual(Quantity(50000), order.filled_quantity)
        self.assertEqual(Price('1.00000'), order.price)
        self.assertEqual(Price('0.99999'), order.average_price)
        self.assertEqual(Decimal('-0.00001'), order.slippage)
        self.assertFalse(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_can_apply_order_overfilled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        event = OrderFilled(
            order.id,
            self.account_id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order.symbol,
            order.side,
            Quantity(150000),
            Price('0.99999'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.OVER_FILLED, order.state)
        self.assertEqual(Quantity(150000), order.filled_quantity)
        self.assertFalse(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)
